//! Stable native inspection, backup, and logical-transfer tools.

use crate::construct::{BUILTIN_ROLES, Thing};
use crate::datatype::{Time, TimeType};
use crate::error::{DatabaseError, Result};
use crate::literal::{LiteralFamily, LiteralValue};
use crate::storage::{
    FORMAT_MAJOR, FORMAT_MINOR, LogStore, ReplayedPosit, ReplayedRole, StoreBackend, StoreSnapshot,
    codec_records, posit_record_logical, role_record_parts, sync_parent_directory,
};
use chrono::{Datelike, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

pub const LOGICAL_EXPORT_FORMAT: &str = "positorium.logical-export";
pub const LOGICAL_EXPORT_VERSION: u16 = 1;
pub const IDENTITY_REMAP_FORMAT: &str = "positorium.identity-remap";
pub const IDENTITY_REMAP_VERSION: u16 = 1;

const MAX_LOGICAL_RECORD_SIZE: usize = 16 * 1024 * 1024;
const IMPORT_BATCH_SIZE: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalIdentity {
    pub store_uuid: String,
    pub local: Thing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StoreInspection {
    pub store_uuid: String,
    pub storage_format_major: u16,
    pub storage_format_minor: u16,
    pub committed_length: u64,
    pub physical_log_length: u64,
    pub ignored_tail_bytes: u64,
    pub committed_batches: usize,
    pub roles: usize,
    pub posits: usize,
    pub referenced_things: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportReport {
    pub source_store_uuid: String,
    pub roles: usize,
    pub posits: usize,
    pub bytes_written: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub source_store_uuid: String,
    pub destination_store_uuid: String,
    pub roles: usize,
    pub posits: usize,
    pub remapped_identities: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupReport {
    pub store_uuid: String,
    pub committed_length: u64,
    pub roles: usize,
    pub posits: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityRemap {
    pub source: ExternalIdentity,
    pub destination: ExternalIdentity,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case", deny_unknown_fields)]
enum LogicalRecord {
    Header {
        format: String,
        version: u16,
        source_store_uuid: String,
    },
    Role {
        identity: ExternalIdentity,
        name: String,
        reserved: bool,
    },
    Posit {
        identity: ExternalIdentity,
        appearances: Vec<LogicalAppearance>,
        literal: LogicalLiteral,
        time: LogicalTime,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LogicalAppearance {
    role: ExternalIdentity,
    thing: ExternalIdentity,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LogicalLiteral {
    family: String,
    token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "precision", rename_all = "snake_case", deny_unknown_fields)]
enum LogicalTime {
    BeginningOfTime,
    EndOfTime,
    Year {
        year: i32,
    },
    YearMonth {
        year: i32,
        month: u8,
    },
    Date {
        year: i32,
        month: u8,
        day: u8,
    },
    DateTimeUtc {
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        minute: u8,
        second: u8,
        nanosecond: u32,
    },
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct RemapDocument<'a> {
    format: &'static str,
    version: u16,
    source_store_uuid: &'a str,
    destination_store_uuid: &'a str,
    mappings: &'a [IdentityRemap],
}

pub fn inspect_store(path: impl AsRef<Path>) -> Result<StoreInspection> {
    let (snapshot, replayed) = StoreSnapshot::open_replayed(path)?;
    let mut things = HashSet::new();
    for role in &replayed.roles {
        things.insert(role.identity);
    }
    for posit in &replayed.posits {
        things.insert(posit.identity);
        for &(role, thing) in &posit.appearances {
            things.insert(role);
            things.insert(thing);
        }
    }
    Ok(StoreInspection {
        store_uuid: format_uuid(snapshot.store_uuid()),
        storage_format_major: FORMAT_MAJOR,
        storage_format_minor: FORMAT_MINOR,
        committed_length: snapshot.committed_length(),
        physical_log_length: snapshot.file_length(),
        ignored_tail_bytes: snapshot.file_length() - snapshot.committed_length(),
        committed_batches: snapshot.batch_count(),
        roles: replayed.roles.len(),
        posits: replayed.posits.len(),
        referenced_things: things.len(),
    })
}

pub fn export_store(
    store_path: impl AsRef<Path>,
    output_path: impl AsRef<Path>,
) -> Result<ExportReport> {
    let store_path = store_path.as_ref();
    let output_path = output_path.as_ref();
    reject_path_inside_store(store_path, output_path, "logical export")?;
    let (_snapshot, mut replayed) = StoreSnapshot::open_replayed(store_path)?;
    replayed.roles.sort_unstable_by_key(|role| role.identity);
    replayed.posits.sort_unstable_by_key(|posit| posit.identity);
    let uuid = format_uuid(replayed.store_uuid);

    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output_path)
        .map_err(DatabaseError::from_io)?;
    let mut writer = BufWriter::new(output);
    write_record(
        &mut writer,
        &LogicalRecord::Header {
            format: LOGICAL_EXPORT_FORMAT.to_string(),
            version: LOGICAL_EXPORT_VERSION,
            source_store_uuid: uuid.clone(),
        },
    )?;
    for role in &replayed.roles {
        write_record(
            &mut writer,
            &LogicalRecord::Role {
                identity: external_identity(&uuid, role.identity),
                name: role.name.clone(),
                reserved: role.reserved,
            },
        )?;
    }
    for posit in &replayed.posits {
        write_record(
            &mut writer,
            &LogicalRecord::Posit {
                identity: external_identity(&uuid, posit.identity),
                appearances: posit
                    .appearances
                    .iter()
                    .map(|&(role, thing)| LogicalAppearance {
                        role: external_identity(&uuid, role),
                        thing: external_identity(&uuid, thing),
                    })
                    .collect(),
                literal: LogicalLiteral {
                    family: family_name(posit.value.family()).to_string(),
                    token: posit.value.token().to_string(),
                },
                time: LogicalTime::from_time(&posit.time),
            },
        )?;
    }
    writer.flush().map_err(DatabaseError::from_io)?;
    writer
        .get_ref()
        .sync_all()
        .map_err(DatabaseError::from_io)?;
    let bytes_written = writer
        .get_ref()
        .metadata()
        .map_err(DatabaseError::from_io)?
        .len();
    sync_parent_directory(output_path)?;
    Ok(ExportReport {
        source_store_uuid: uuid,
        roles: replayed.roles.len(),
        posits: replayed.posits.len(),
        bytes_written,
    })
}

pub fn import_store(
    export_path: impl AsRef<Path>,
    destination_path: impl AsRef<Path>,
    remap_path: impl AsRef<Path>,
) -> Result<ImportReport> {
    let destination_path = destination_path.as_ref();
    let remap_path = remap_path.as_ref();
    if destination_path.exists() {
        return Err(DatabaseError::Persistence(format!(
            "import destination '{}' already exists",
            destination_path.display()
        )));
    }
    if remap_path.exists() {
        return Err(DatabaseError::Persistence(format!(
            "identity-remap output '{}' already exists",
            remap_path.display()
        )));
    }
    let imported = read_export(export_path.as_ref())?;
    let mut store = LogStore::open(destination_path)?;
    let destination_uuid = format_uuid(store.store_uuid());
    if destination_uuid == imported.uuid {
        return Err(DatabaseError::Invariant(
            "new destination unexpectedly reused the source store UUID".to_string(),
        ));
    }
    let (identity_map, mappings) = build_identity_remap(&imported, &destination_uuid)?;

    let mut transformed_roles = Vec::with_capacity(imported.roles.len());
    let mut metadata = Vec::with_capacity(imported.roles.len() + 3);
    for role in &imported.roles {
        let identity = mapped(&identity_map, role.identity)?;
        transformed_roles.push(ReplayedRole {
            identity,
            name: role.name.clone(),
            reserved: role.reserved,
        });
        metadata.push(role_record_parts(identity, &role.name, role.reserved));
    }
    metadata.extend(codec_records());
    store.append_batch(&metadata)?;

    let mut transformed_posits = Vec::with_capacity(imported.posits.len());
    let mut pending = Vec::with_capacity(IMPORT_BATCH_SIZE);
    for posit in &imported.posits {
        let identity = mapped(&identity_map, posit.identity)?;
        let mut appearances = posit
            .appearances
            .iter()
            .map(|&(role, thing)| Ok((mapped(&identity_map, role)?, mapped(&identity_map, thing)?)))
            .collect::<Result<Vec<_>>>()?;
        let record = posit_record_logical(identity, &mut appearances, &posit.value, &posit.time)?;
        transformed_posits.push(ReplayedPosit {
            identity,
            appearances,
            value: posit.value.clone(),
            time: posit.time.clone(),
        });
        pending.push(record);
        if pending.len() == IMPORT_BATCH_SIZE {
            store.append_batch(&pending)?;
            pending.clear();
        }
    }
    if !pending.is_empty() {
        store.append_batch(&pending)?;
    }
    store.flush()?;

    transformed_roles.sort_unstable_by_key(|role| role.identity);
    transformed_posits.sort_unstable_by_key(|posit| posit.identity);
    let mut replayed = store.replay_logical()?;
    replayed.roles.sort_unstable_by_key(|role| role.identity);
    replayed.posits.sort_unstable_by_key(|posit| posit.identity);
    if replayed.roles != transformed_roles || replayed.posits != transformed_posits {
        return Err(DatabaseError::Invariant(
            "import verification did not reproduce the transformed logical records".to_string(),
        ));
    }
    drop(store);

    let inspection = inspect_store(destination_path)?;
    if inspection.roles != imported.roles.len() || inspection.posits != imported.posits.len() {
        return Err(DatabaseError::Invariant(
            "imported store count verification failed".to_string(),
        ));
    }
    write_remap(remap_path, &imported.uuid, &destination_uuid, &mappings)?;
    Ok(ImportReport {
        source_store_uuid: imported.uuid,
        destination_store_uuid: destination_uuid,
        roles: inspection.roles,
        posits: inspection.posits,
        remapped_identities: mappings.len(),
    })
}

pub fn backup_store(
    store_path: impl AsRef<Path>,
    destination_path: impl AsRef<Path>,
) -> Result<BackupReport> {
    let store_path = store_path.as_ref();
    let destination_path = destination_path.as_ref();
    reject_path_inside_store(store_path, destination_path, "backup")?;
    let (mut snapshot, source) = StoreSnapshot::open_replayed(store_path)?;
    let committed_length = snapshot.committed_length();
    snapshot.backup_to(destination_path)?;
    drop(snapshot);

    let backup = inspect_store(destination_path)?;
    if backup.store_uuid != format_uuid(source.store_uuid)
        || backup.committed_length != committed_length
        || backup.physical_log_length != committed_length
        || backup.roles != source.roles.len()
        || backup.posits != source.posits.len()
    {
        return Err(DatabaseError::Invariant(
            "physical backup verification failed".to_string(),
        ));
    }
    Ok(BackupReport {
        store_uuid: backup.store_uuid,
        committed_length,
        roles: backup.roles,
        posits: backup.posits,
    })
}

struct ImportedStore {
    uuid: String,
    roles: Vec<ReplayedRole>,
    posits: Vec<ReplayedPosit>,
}

fn read_export(path: &Path) -> Result<ImportedStore> {
    let file = File::open(path).map_err(DatabaseError::from_io)?;
    let mut reader = BufReader::new(file);
    let mut line_number = 0usize;
    let mut uuid = None;
    let mut roles = Vec::new();
    let mut posits = Vec::new();
    let mut saw_posit = false;
    while let Some(line) = read_bounded_line(&mut reader, line_number + 1)? {
        line_number += 1;
        if line.iter().all(u8::is_ascii_whitespace) {
            return data_error(format!("logical export line {line_number} is empty"));
        }
        let record: LogicalRecord =
            serde_json::from_slice(&line).map_err(|error| DatabaseError::DataCorruption {
                message: format!("logical export line {line_number}: {error}"),
            })?;
        match record {
            LogicalRecord::Header {
                format,
                version,
                source_store_uuid,
            } => {
                if line_number != 1 || uuid.is_some() {
                    return data_error("logical export Header must be the first and only header");
                }
                if format != LOGICAL_EXPORT_FORMAT || version != LOGICAL_EXPORT_VERSION {
                    return data_error(format!(
                        "unsupported logical export format/version '{format}'/{version}"
                    ));
                }
                let parsed = parse_uuid(&source_store_uuid)?;
                if format_uuid(parsed) != source_store_uuid {
                    return data_error("source store UUID is not in canonical lowercase form");
                }
                uuid = Some(source_store_uuid);
            }
            LogicalRecord::Role {
                identity,
                name,
                reserved,
            } => {
                let source_uuid = uuid
                    .as_deref()
                    .ok_or_else(|| corruption("logical export is missing its Header"))?;
                if saw_posit {
                    return data_error("Role records must precede Posit records");
                }
                roles.push(ReplayedRole {
                    identity: validate_external(&identity, source_uuid)?,
                    name,
                    reserved,
                });
            }
            LogicalRecord::Posit {
                identity,
                appearances,
                literal,
                time,
            } => {
                let source_uuid = uuid
                    .as_deref()
                    .ok_or_else(|| corruption("logical export is missing its Header"))?;
                saw_posit = true;
                let mut decoded_appearances = appearances
                    .into_iter()
                    .map(|appearance| {
                        Ok((
                            validate_external(&appearance.role, source_uuid)?,
                            validate_external(&appearance.thing, source_uuid)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                decoded_appearances.sort_unstable();
                let family = parse_family(&literal.family)?;
                let value = LiteralValue::new(literal.token, family).map_err(|error| {
                    DatabaseError::DataCorruption {
                        message: format!("invalid logical export literal: {error}"),
                    }
                })?;
                posits.push(ReplayedPosit {
                    identity: validate_external(&identity, source_uuid)?,
                    appearances: decoded_appearances,
                    value,
                    time: time.into_time()?,
                });
            }
        }
    }
    let uuid = uuid.ok_or_else(|| corruption("logical export has no Header"))?;
    validate_imported(&roles, &posits)?;
    Ok(ImportedStore {
        uuid,
        roles,
        posits,
    })
}

fn validate_imported(roles: &[ReplayedRole], posits: &[ReplayedPosit]) -> Result<()> {
    let mut role_ids = HashSet::new();
    let mut role_names = HashSet::new();
    for role in roles {
        if role.identity == 0 {
            return data_error("logical export uses genesis identity 0 for a Role");
        }
        if !role_ids.insert(role.identity) || !role_names.insert(role.name.as_str()) {
            return data_error("logical export contains duplicate Role identity or name");
        }
    }
    for (identity, name) in BUILTIN_ROLES {
        if !roles
            .iter()
            .any(|role| role.identity == identity && role.name == name && role.reserved)
        {
            return data_error(format!(
                "logical export lacks reserved built-in Role '{name}' at identity {identity}"
            ));
        }
    }
    let mut construct_ids = role_ids.clone();
    let mut propositions = HashSet::new();
    for posit in posits {
        if posit.identity == 0 {
            return data_error("logical export uses genesis identity 0 for a Posit");
        }
        if !construct_ids.insert(posit.identity) {
            return data_error("logical export reuses a Role or Posit identity");
        }
        if posit.appearances.is_empty()
            || posit
                .appearances
                .windows(2)
                .any(|pair| pair[0].0 == pair[1].0)
            || posit
                .appearances
                .iter()
                .any(|(role, thing)| *role == 0 || *thing == 0)
        {
            return data_error("logical export contains an invalid appearance set");
        }
        if posit
            .appearances
            .iter()
            .any(|(role, _)| !role_ids.contains(role))
        {
            return data_error("logical export Posit references an unknown Role");
        }
        if !propositions.insert((
            posit.appearances.clone(),
            posit.value.clone(),
            posit.time.clone(),
        )) {
            return data_error("logical export contains a duplicate Posit proposition");
        }
    }
    Ok(())
}

fn build_identity_remap(
    imported: &ImportedStore,
    destination_uuid: &str,
) -> Result<(HashMap<Thing, Thing>, Vec<IdentityRemap>)> {
    let mut foreign = BTreeSet::new();
    for role in &imported.roles {
        foreign.insert(role.identity);
    }
    for posit in &imported.posits {
        foreign.insert(posit.identity);
        for &(role, thing) in &posit.appearances {
            foreign.insert(role);
            foreign.insert(thing);
        }
    }

    let mut used = HashSet::from(BUILTIN_ROLES.map(|(identity, _)| identity));
    let mut map = HashMap::with_capacity(foreign.len());
    for (identity, _) in BUILTIN_ROLES {
        map.insert(identity, identity);
    }
    let mut next = 6u64;
    for &source in &foreign {
        if map.contains_key(&source) {
            continue;
        }
        while used.contains(&next) || next == source {
            next = next.checked_add(1).ok_or_else(|| {
                DatabaseError::Invariant("identity space exhausted during import".to_string())
            })?;
        }
        map.insert(source, next);
        used.insert(next);
        next = next.checked_add(1).ok_or_else(|| {
            DatabaseError::Invariant("identity space exhausted during import".to_string())
        })?;
    }
    let mappings = foreign
        .into_iter()
        .map(|source| {
            let destination = mapped(&map, source)?;
            Ok(IdentityRemap {
                source: external_identity(&imported.uuid, source),
                destination: external_identity(destination_uuid, destination),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok((map, mappings))
}

fn mapped(map: &HashMap<Thing, Thing>, source: Thing) -> Result<Thing> {
    map.get(&source).copied().ok_or_else(|| {
        DatabaseError::Invariant(format!("identity {source} is absent from the import remap"))
    })
}

fn write_remap(
    path: &Path,
    source_uuid: &str,
    destination_uuid: &str,
    mappings: &[IdentityRemap],
) -> Result<()> {
    let output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(DatabaseError::from_io)?;
    let mut writer = BufWriter::new(output);
    serde_json::to_writer_pretty(
        &mut writer,
        &RemapDocument {
            format: IDENTITY_REMAP_FORMAT,
            version: IDENTITY_REMAP_VERSION,
            source_store_uuid: source_uuid,
            destination_store_uuid: destination_uuid,
            mappings,
        },
    )
    .map_err(|error| DatabaseError::Persistence(error.to_string()))?;
    writer.write_all(b"\n").map_err(DatabaseError::from_io)?;
    writer.flush().map_err(DatabaseError::from_io)?;
    writer
        .get_ref()
        .sync_all()
        .map_err(DatabaseError::from_io)?;
    sync_parent_directory(path)
}

fn write_record(writer: &mut BufWriter<File>, record: &LogicalRecord) -> Result<()> {
    let encoded = serde_json::to_vec(record)
        .map_err(|error| DatabaseError::Persistence(error.to_string()))?;
    if encoded.len() > MAX_LOGICAL_RECORD_SIZE {
        return Err(DatabaseError::Persistence(format!(
            "logical export record is {} bytes; maximum is {MAX_LOGICAL_RECORD_SIZE}",
            encoded.len()
        )));
    }
    writer.write_all(&encoded).map_err(DatabaseError::from_io)?;
    writer.write_all(b"\n").map_err(DatabaseError::from_io)
}

fn read_bounded_line(reader: &mut impl BufRead, line_number: usize) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let (chunk, consumed, complete, eof) = {
            let available = reader.fill_buf().map_err(DatabaseError::from_io)?;
            if available.is_empty() {
                (Vec::new(), 0, false, true)
            } else if let Some(position) = available.iter().position(|byte| *byte == b'\n') {
                (available[..position].to_vec(), position + 1, true, false)
            } else {
                (available.to_vec(), available.len(), false, false)
            }
        };
        if eof {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let new_length =
            line.len()
                .checked_add(chunk.len())
                .ok_or_else(|| DatabaseError::DataCorruption {
                    message: format!("logical export line {line_number} length overflow"),
                })?;
        if new_length > MAX_LOGICAL_RECORD_SIZE {
            return data_error(format!(
                "logical export line {line_number} exceeds {MAX_LOGICAL_RECORD_SIZE} bytes"
            ));
        }
        line.extend_from_slice(&chunk);
        reader.consume(consumed);
        if complete {
            return Ok(Some(line));
        }
    }
}

fn reject_path_inside_store(store: &Path, target: &Path, operation: &str) -> Result<()> {
    let canonical_store = std::fs::canonicalize(store).map_err(DatabaseError::from_io)?;
    let target_parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = std::fs::canonicalize(target_parent).map_err(DatabaseError::from_io)?;
    let target_name = target.file_name().ok_or_else(|| {
        DatabaseError::Persistence(format!("{operation} target has no file name"))
    })?;
    let normalized_target: PathBuf = canonical_parent.join(target_name);
    if normalized_target.starts_with(&canonical_store) {
        return Err(DatabaseError::Persistence(format!(
            "{operation} target '{}' cannot be inside store '{}'",
            target.display(),
            store.display()
        )));
    }
    Ok(())
}

fn external_identity(uuid: &str, local: Thing) -> ExternalIdentity {
    ExternalIdentity {
        store_uuid: uuid.to_string(),
        local,
    }
}

fn validate_external(identity: &ExternalIdentity, expected_uuid: &str) -> Result<Thing> {
    if identity.store_uuid != expected_uuid {
        return data_error(format!(
            "external identity {} belongs to store {}, expected {expected_uuid}",
            identity.local, identity.store_uuid
        ));
    }
    Ok(identity.local)
}

fn family_name(family: LiteralFamily) -> &'static str {
    match family {
        LiteralFamily::String => "string",
        LiteralFamily::Integer => "integer",
        LiteralFamily::Decimal => "decimal",
        LiteralFamily::Certainty => "certainty",
        LiteralFamily::Json => "json",
        LiteralFamily::Time => "time",
    }
}

fn parse_family(name: &str) -> Result<LiteralFamily> {
    match name {
        "string" => Ok(LiteralFamily::String),
        "integer" => Ok(LiteralFamily::Integer),
        "decimal" => Ok(LiteralFamily::Decimal),
        "certainty" => Ok(LiteralFamily::Certainty),
        "json" => Ok(LiteralFamily::Json),
        "time" => Ok(LiteralFamily::Time),
        _ => data_error(format!("unknown logical literal family '{name}'")),
    }
}

impl LogicalTime {
    fn from_time(time: &Time) -> Self {
        match time.time_type() {
            TimeType::BeginningOfTime => Self::BeginningOfTime,
            TimeType::EndOfTime => Self::EndOfTime,
            TimeType::Year(year) => Self::Year { year: *year },
            TimeType::YearMonth(year, month) => Self::YearMonth {
                year: *year,
                month: *month,
            },
            TimeType::Date(date) => Self::Date {
                year: date.year(),
                month: date.month() as u8,
                day: date.day() as u8,
            },
            TimeType::DateTime(datetime) => Self::DateTimeUtc {
                year: datetime.year(),
                month: datetime.month() as u8,
                day: datetime.day() as u8,
                hour: datetime.hour() as u8,
                minute: datetime.minute() as u8,
                second: datetime.second() as u8,
                nanosecond: datetime.nanosecond(),
            },
        }
    }

    fn into_time(self) -> Result<Time> {
        let moment = match self {
            Self::BeginningOfTime => TimeType::BeginningOfTime,
            Self::EndOfTime => TimeType::EndOfTime,
            Self::Year { year } => TimeType::Year(year),
            Self::YearMonth { year, month } if (1..=12).contains(&month) => {
                TimeType::YearMonth(year, month)
            }
            Self::YearMonth { .. } => return data_error("invalid logical year-month"),
            Self::Date { year, month, day } => TimeType::Date(
                NaiveDate::from_ymd_opt(year, u32::from(month), u32::from(day))
                    .ok_or_else(|| corruption("invalid logical date"))?,
            ),
            Self::DateTimeUtc {
                year,
                month,
                day,
                hour,
                minute,
                second,
                nanosecond,
            } => {
                let date = NaiveDate::from_ymd_opt(year, u32::from(month), u32::from(day))
                    .ok_or_else(|| corruption("invalid logical datetime date"))?;
                TimeType::DateTime(
                    date.and_hms_nano_opt(
                        u32::from(hour),
                        u32::from(minute),
                        u32::from(second),
                        nanosecond,
                    )
                    .ok_or_else(|| corruption("invalid logical UTC datetime"))?,
                )
            }
        };
        Ok(Time::from_time_type(moment))
    }
}

fn format_uuid(uuid: [u8; 16]) -> String {
    let hex = uuid.map(|byte| format!("{byte:02x}")).concat();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn parse_uuid(value: &str) -> Result<[u8; 16]> {
    let compact: String = value
        .chars()
        .filter(|character| *character != '-')
        .collect();
    let bytes = value.as_bytes();
    if compact.len() != 32
        || bytes.len() != 36
        || [8, 13, 18, 23].iter().any(|index| bytes[*index] != b'-')
    {
        return data_error(format!("invalid store UUID '{value}'"));
    }
    let mut uuid = [0u8; 16];
    for (index, target) in uuid.iter_mut().enumerate() {
        *target = u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16)
            .map_err(|_| corruption(format!("invalid store UUID '{value}'")))?;
    }
    Ok(uuid)
}

fn data_error<T>(message: impl Into<String>) -> Result<T> {
    Err(corruption(message))
}

fn corruption(message: impl Into<String>) -> DatabaseError {
    DatabaseError::DataCorruption {
        message: message.into(),
    }
}
