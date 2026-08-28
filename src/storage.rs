//! Versioned append-only storage primitives for the beta store format.
//!
//! This module is private storage machinery. Its byte contract is specified in
//! `docs/reference/STORAGE.md`.

#![allow(dead_code)] // Includes reserved backup/inspection hooks not yet public.

use crate::construct::{Posit, Role, Thing};
use crate::datatype::{Time, TimeType};
use crate::error::{DatabaseError, Result};
use crate::literal::{LiteralFamily, LiteralValue};
use chrono::{Datelike, NaiveDate, Timelike};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MANIFEST_NAME: &str = "manifest.pmf";
const MANIFEST_NEXT_NAME: &str = "manifest.next";
const LOCK_NAME: &str = "store.lock";
const LOG_NAME: &str = "log-0000000000000001.ptl";
const MANIFEST_MAGIC: &[u8; 8] = b"POSITPMF";
const LOG_MAGIC: &[u8; 8] = b"POSITLOG";
const FRAME_MAGIC: &[u8; 4] = b"PTR1";
pub(crate) const FORMAT_MAJOR: u16 = 1;
pub(crate) const FORMAT_MINOR: u16 = 0;
const MANIFEST_HEADER_LEN: u32 = 48;
const LOG_HEADER_LEN: u32 = 64;
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

pub(crate) const RECORD_ROLE: u8 = 1;
pub(crate) const RECORD_CODEC: u8 = 2;
pub(crate) const RECORD_POSIT: u8 = 3;
const RECORD_COMMIT: u8 = 255;
const RAW_CODEC_IDENTIFIER: u16 = 0;
const RAW_CODEC_VERSION: u16 = 1;
const CANONICAL_I64_CODEC_IDENTIFIER: u16 = 1;
const CANONICAL_I64_CODEC_VERSION: u16 = 1;
const CANONICAL_CERTAINTY_CODEC_IDENTIFIER: u16 = 2;
const CANONICAL_CERTAINTY_CODEC_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BuiltinCodec {
    RawUtf8,
    CanonicalI64,
    CanonicalCertainty,
}

impl BuiltinCodec {
    const ALL: [Self; 3] = [Self::RawUtf8, Self::CanonicalI64, Self::CanonicalCertainty];

    fn identifier(self) -> u16 {
        match self {
            Self::RawUtf8 => RAW_CODEC_IDENTIFIER,
            Self::CanonicalI64 => CANONICAL_I64_CODEC_IDENTIFIER,
            Self::CanonicalCertainty => CANONICAL_CERTAINTY_CODEC_IDENTIFIER,
        }
    }

    fn version(self) -> u16 {
        match self {
            Self::RawUtf8 => RAW_CODEC_VERSION,
            Self::CanonicalI64 => CANONICAL_I64_CODEC_VERSION,
            Self::CanonicalCertainty => CANONICAL_CERTAINTY_CODEC_VERSION,
        }
    }

    fn family(self) -> u8 {
        match self {
            Self::RawUtf8 => 0,
            Self::CanonicalI64 => LiteralFamily::Integer.identifier(),
            Self::CanonicalCertainty => LiteralFamily::Certainty.identifier(),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::RawUtf8 => "raw-utf8-token",
            Self::CanonicalI64 => "canonical-i64",
            Self::CanonicalCertainty => "canonical-certainty",
        }
    }

    fn from_pair(identifier: u16, version: u16) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|codec| codec.identifier() == identifier && codec.version() == version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayedRole {
    pub identity: Thing,
    pub name: String,
    pub reserved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayedPosit {
    pub identity: Thing,
    pub appearances: Vec<(Thing, Thing)>,
    pub value: LiteralValue,
    pub time: Time,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReplayedStore {
    pub store_uuid: [u8; 16],
    pub roles: Vec<ReplayedRole>,
    pub posits: Vec<ReplayedPosit>,
    pub has_required_codecs: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingRecord {
    pub record_type: u8,
    pub version: u8,
    pub flags: u16,
    pub payload: Vec<u8>,
}

impl PendingRecord {
    pub(crate) fn new(record_type: u8, payload: Vec<u8>) -> Self {
        Self {
            record_type,
            version: 1,
            flags: 0,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredRecord {
    pub record_type: u8,
    pub version: u8,
    pub flags: u16,
    pub sequence: u64,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Manifest {
    store_uuid: [u8; 16],
    committed_length: u64,
}

pub(crate) trait StoreBackend {
    fn append_batch(&mut self, records: &[PendingRecord]) -> Result<Vec<StoredRecord>>;
    fn flush(&mut self) -> Result<()>;
}

pub(crate) struct MemoryStore {
    batches: Vec<Vec<StoredRecord>>,
    next_sequence: u64,
}

impl MemoryStore {
    pub(crate) fn new() -> Self {
        Self {
            batches: Vec::new(),
            next_sequence: 1,
        }
    }

    #[cfg(test)]
    fn replay(&self) -> &[Vec<StoredRecord>] {
        &self.batches
    }
}

impl StoreBackend for MemoryStore {
    fn append_batch(&mut self, records: &[PendingRecord]) -> Result<Vec<StoredRecord>> {
        validate_pending_records(records)?;
        let mut stored = Vec::with_capacity(records.len());
        for record in records {
            stored.push(StoredRecord {
                record_type: record.record_type,
                version: record.version,
                flags: record.flags,
                sequence: self.next_sequence,
                payload: record.payload.clone(),
            });
            self.next_sequence = self
                .next_sequence
                .checked_add(1)
                .ok_or_else(|| DatabaseError::Invariant("record sequence exhausted".to_string()))?;
        }
        self.batches.push(stored.clone());
        Ok(stored)
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

pub(crate) struct LogStore {
    directory: PathBuf,
    _lock: File,
    log: File,
    manifest: Manifest,
    batch_count: usize,
    next_sequence: u64,
    next_batch: u64,
    poisoned: bool,
    #[cfg(test)]
    injected_failure: Option<InjectedFailure>,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InjectedFailure {
    LogWrite,
    ManifestCommit,
}

pub(crate) struct StoreSnapshot {
    directory: PathBuf,
    _lock: File,
    log: File,
    manifest: Manifest,
    batch_count: usize,
    file_length: u64,
}

struct ReplayMetadata {
    next_sequence: u64,
    next_batch: u64,
    batch_count: usize,
}

impl LogStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_internal(path, false).map(|(store, _)| store)
    }

    pub(crate) fn open_replayed(path: impl AsRef<Path>) -> Result<(Self, ReplayedStore)> {
        let (store, replayed) = Self::open_internal(path, true)?;
        Ok((
            store,
            replayed.ok_or_else(|| {
                DatabaseError::Invariant("logical replay was not produced during open".to_string())
            })?,
        ))
    }

    fn open_internal(
        path: impl AsRef<Path>,
        decode_logical: bool,
    ) -> Result<(Self, Option<ReplayedStore>)> {
        let directory = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&directory).map_err(DatabaseError::from_io)?;
        let lock_path = directory.join(LOCK_NAME);
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(DatabaseError::from_io)?;
        lock.try_lock().map_err(|error| {
            DatabaseError::Persistence(format!(
                "store '{}' is already owned: {error:?}",
                directory.display()
            ))
        })?;

        let manifest_path = directory.join(MANIFEST_NAME);
        let manifest = if manifest_path.exists() {
            read_manifest(&manifest_path)?
        } else {
            initialize_store(&directory)?
        };
        let log_path = directory.join(LOG_NAME);
        let mut log = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&log_path)
            .map_err(DatabaseError::from_io)?;
        verify_log_header(&mut log, manifest.store_uuid)?;
        let file_length = log.metadata().map_err(DatabaseError::from_io)?.len();
        if manifest.committed_length < u64::from(LOG_HEADER_LEN)
            || manifest.committed_length > file_length
        {
            return Err(DatabaseError::DataCorruption {
                message: format!(
                    "manifest committed length {} is invalid for log length {}",
                    manifest.committed_length, file_length
                ),
            });
        }
        if file_length > manifest.committed_length {
            log.set_len(manifest.committed_length)
                .map_err(DatabaseError::from_io)?;
            log.sync_all().map_err(DatabaseError::from_io)?;
        }
        let (metadata, replayed) = if decode_logical {
            let (metadata, replayed) =
                replay_committed_logical(&mut log, manifest.committed_length, manifest.store_uuid)?;
            (metadata, Some(replayed))
        } else {
            (
                scan_committed(&mut log, manifest.committed_length, |_| Ok(()))?,
                None,
            )
        };

        Ok((
            Self {
                directory,
                _lock: lock,
                log,
                manifest,
                batch_count: metadata.batch_count,
                next_sequence: metadata.next_sequence,
                next_batch: metadata.next_batch,
                poisoned: false,
                #[cfg(test)]
                injected_failure: None,
            },
            replayed,
        ))
    }

    pub(crate) fn store_uuid(&self) -> [u8; 16] {
        self.manifest.store_uuid
    }

    pub(crate) fn committed_length(&self) -> u64 {
        self.manifest.committed_length
    }

    pub(crate) fn replay_logical(&mut self) -> Result<ReplayedStore> {
        replay_committed_logical(
            &mut self.log,
            self.manifest.committed_length,
            self.manifest.store_uuid,
        )
        .map(|(_, replayed)| replayed)
    }

    #[cfg(test)]
    pub(crate) fn committed_batches(&mut self) -> Result<Vec<Vec<StoredRecord>>> {
        let mut batches = Vec::new();
        scan_committed(&mut self.log, self.manifest.committed_length, |batch| {
            batches.push(batch.to_vec());
            Ok(())
        })?;
        Ok(batches)
    }

    #[cfg(test)]
    pub(crate) fn batch_count(&self) -> usize {
        self.batch_count
    }

    #[cfg(test)]
    pub(crate) fn inject_failure(&mut self, failure: InjectedFailure) {
        self.injected_failure = Some(failure);
    }
}

impl StoreSnapshot {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_internal(path, false).map(|(snapshot, _)| snapshot)
    }

    pub(crate) fn open_replayed(path: impl AsRef<Path>) -> Result<(Self, ReplayedStore)> {
        let (snapshot, replayed) = Self::open_internal(path, true)?;
        Ok((
            snapshot,
            replayed.ok_or_else(|| {
                DatabaseError::Invariant("logical replay was not produced for snapshot".to_string())
            })?,
        ))
    }

    fn open_internal(
        path: impl AsRef<Path>,
        decode_logical: bool,
    ) -> Result<(Self, Option<ReplayedStore>)> {
        let directory = path.as_ref().to_path_buf();
        if !directory.is_dir() {
            return Err(DatabaseError::Persistence(format!(
                "store '{}' is not a directory",
                directory.display()
            )));
        }
        let lock_path = directory.join(LOCK_NAME);
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(DatabaseError::from_io)?;
        File::try_lock_shared(&lock).map_err(|error| {
            DatabaseError::Persistence(format!(
                "store '{}' is currently owned by a writer: {error:?}",
                directory.display()
            ))
        })?;

        let manifest = read_manifest(&directory.join(MANIFEST_NAME))?;
        let mut log = File::open(directory.join(LOG_NAME)).map_err(DatabaseError::from_io)?;
        verify_log_header(&mut log, manifest.store_uuid)?;
        let file_length = log.metadata().map_err(DatabaseError::from_io)?.len();
        if manifest.committed_length < u64::from(LOG_HEADER_LEN)
            || manifest.committed_length > file_length
        {
            return Err(DatabaseError::DataCorruption {
                message: format!(
                    "manifest committed length {} is invalid for log length {}",
                    manifest.committed_length, file_length
                ),
            });
        }
        let (metadata, replayed) = if decode_logical {
            let (metadata, replayed) =
                replay_committed_logical(&mut log, manifest.committed_length, manifest.store_uuid)?;
            (metadata, Some(replayed))
        } else {
            (
                scan_committed(&mut log, manifest.committed_length, |_| Ok(()))?,
                None,
            )
        };
        Ok((
            Self {
                directory,
                _lock: lock,
                log,
                manifest,
                batch_count: metadata.batch_count,
                file_length,
            },
            replayed,
        ))
    }

    pub(crate) fn store_uuid(&self) -> [u8; 16] {
        self.manifest.store_uuid
    }

    pub(crate) fn committed_length(&self) -> u64 {
        self.manifest.committed_length
    }

    pub(crate) fn file_length(&self) -> u64 {
        self.file_length
    }

    pub(crate) fn batch_count(&self) -> usize {
        self.batch_count
    }

    pub(crate) fn replay_logical(&mut self) -> Result<ReplayedStore> {
        replay_committed_logical(
            &mut self.log,
            self.manifest.committed_length,
            self.manifest.store_uuid,
        )
        .map(|(_, replayed)| replayed)
    }

    pub(crate) fn backup_to(&mut self, destination: impl AsRef<Path>) -> Result<()> {
        let destination = destination.as_ref();
        if destination.exists() {
            return Err(DatabaseError::Persistence(format!(
                "backup destination '{}' already exists",
                destination.display()
            )));
        }
        std::fs::create_dir(destination).map_err(DatabaseError::from_io)?;

        let backup_lock = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(destination.join(LOCK_NAME))
            .map_err(DatabaseError::from_io)?;
        backup_lock.sync_all().map_err(DatabaseError::from_io)?;

        let mut backup_log = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination.join(LOG_NAME))
            .map_err(DatabaseError::from_io)?;
        self.log
            .seek(SeekFrom::Start(0))
            .map_err(DatabaseError::from_io)?;
        let mut committed = (&mut self.log).take(self.manifest.committed_length);
        let copied =
            std::io::copy(&mut committed, &mut backup_log).map_err(DatabaseError::from_io)?;
        if copied != self.manifest.committed_length {
            return Err(DatabaseError::Persistence(format!(
                "backup copied {copied} log bytes; expected {}",
                self.manifest.committed_length
            )));
        }
        backup_log.sync_all().map_err(DatabaseError::from_io)?;

        let manifest_bytes =
            std::fs::read(self.directory.join(MANIFEST_NAME)).map_err(DatabaseError::from_io)?;
        let mut backup_manifest = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination.join(MANIFEST_NAME))
            .map_err(DatabaseError::from_io)?;
        backup_manifest
            .write_all(&manifest_bytes)
            .and_then(|_| backup_manifest.sync_all())
            .map_err(DatabaseError::from_io)?;
        sync_directory(destination)?;
        sync_parent_directory(destination)?;
        Ok(())
    }
}

pub(crate) fn role_record(role: &Role) -> PendingRecord {
    role_record_parts(role.role(), role.name(), role.reserved())
}

pub(crate) fn role_record_parts(identity: Thing, name: &str, reserved: bool) -> PendingRecord {
    let mut payload = Vec::new();
    payload.extend_from_slice(&identity.to_le_bytes());
    payload.push(u8::from(reserved));
    encode_text(name, &mut payload);
    PendingRecord::new(RECORD_ROLE, payload)
}

fn codec_record(codec: BuiltinCodec) -> PendingRecord {
    let mut payload = Vec::new();
    payload.extend_from_slice(&codec.identifier().to_le_bytes());
    payload.extend_from_slice(&codec.version().to_le_bytes());
    payload.push(codec.family());
    payload.extend_from_slice(&0u16.to_le_bytes());
    encode_text(codec.name(), &mut payload);
    PendingRecord::new(RECORD_CODEC, payload)
}

pub(crate) fn codec_records() -> Vec<PendingRecord> {
    BuiltinCodec::ALL.into_iter().map(codec_record).collect()
}

fn raw_codec_record() -> PendingRecord {
    codec_record(BuiltinCodec::RawUtf8)
}

pub(crate) fn posit_record(posit: &Posit<LiteralValue>) -> Result<PendingRecord> {
    posit_record_parts(
        posit.posit(),
        &posit.appearance_set(),
        posit.value(),
        posit.time(),
    )
}

pub(crate) fn posit_record_parts(
    identity: Thing,
    appearance_set: &crate::construct::AppearanceSet,
    value: &LiteralValue,
    time: &Time,
) -> Result<PendingRecord> {
    let mut appearances: Vec<(Thing, Thing)> = appearance_set
        .appearances()
        .iter()
        .map(|appearance| (appearance.role().role(), appearance.thing()))
        .collect();
    posit_record_logical(identity, &mut appearances, value, time)
}

pub(crate) fn posit_record_logical(
    identity: Thing,
    appearances: &mut [(Thing, Thing)],
    value: &LiteralValue,
    time: &Time,
) -> Result<PendingRecord> {
    appearances.sort_unstable();
    if appearances.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(DatabaseError::InvalidAppearanceSet(
            "a persisted appearance set cannot repeat a role".to_string(),
        ));
    }
    posit_record_logical_with_codec(identity, appearances, value, time, select_codec(value))
}

fn posit_record_logical_with_codec(
    identity: Thing,
    appearances: &[(Thing, Thing)],
    value: &LiteralValue,
    time: &Time,
    codec: BuiltinCodec,
) -> Result<PendingRecord> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&identity.to_le_bytes());
    encode_uleb128(appearances.len() as u64, &mut payload);
    for &(role, thing) in appearances.iter() {
        payload.extend_from_slice(&role.to_le_bytes());
        payload.extend_from_slice(&thing.to_le_bytes());
    }
    payload.push(value.family().identifier());
    payload.extend_from_slice(&codec.identifier().to_le_bytes());
    payload.extend_from_slice(&codec.version().to_le_bytes());
    let encoded = encode_literal(value, codec)?;
    encode_blob(&encoded, &mut payload);
    encode_time(time, &mut payload);
    Ok(PendingRecord::new(RECORD_POSIT, payload))
}

fn select_codec(value: &LiteralValue) -> BuiltinCodec {
    match value.family() {
        LiteralFamily::Integer => value
            .token()
            .parse::<i64>()
            .ok()
            .filter(|parsed| parsed.to_string() == value.token())
            .map_or(BuiltinCodec::RawUtf8, |_| BuiltinCodec::CanonicalI64),
        LiteralFamily::Certainty => value
            .token()
            .strip_suffix('%')
            .and_then(|token| token.parse::<i8>().ok())
            .filter(|percent| (-100..=100).contains(percent))
            .filter(|percent| format!("{percent}%") == value.token())
            .map_or(BuiltinCodec::RawUtf8, |_| BuiltinCodec::CanonicalCertainty),
        _ => BuiltinCodec::RawUtf8,
    }
}

fn encode_literal(value: &LiteralValue, codec: BuiltinCodec) -> Result<Vec<u8>> {
    match codec {
        BuiltinCodec::RawUtf8 => Ok(value.token().as_bytes().to_vec()),
        BuiltinCodec::CanonicalI64 => {
            let integer = value.token().parse::<i64>().map_err(|error| {
                DatabaseError::Invariant(format!(
                    "canonical integer codec selection failed: {error}"
                ))
            })?;
            let zigzag = ((integer as u64) << 1) ^ ((integer >> 63) as u64);
            let mut encoded = Vec::new();
            encode_uleb128(zigzag, &mut encoded);
            Ok(encoded)
        }
        BuiltinCodec::CanonicalCertainty => {
            let percent = value
                .token()
                .strip_suffix('%')
                .and_then(|token| token.parse::<i8>().ok())
                .ok_or_else(|| {
                    DatabaseError::Invariant(
                        "canonical certainty codec selection failed".to_string(),
                    )
                })?;
            Ok(vec![percent as u8])
        }
    }
}

fn encode_time(time: &Time, target: &mut Vec<u8>) {
    match time.time_type() {
        TimeType::BeginningOfTime => target.push(0),
        TimeType::EndOfTime => target.push(1),
        TimeType::Year(year) => {
            target.push(2);
            target.extend_from_slice(&year.to_le_bytes());
        }
        TimeType::YearMonth(year, month) => {
            target.push(3);
            target.extend_from_slice(&year.to_le_bytes());
            target.push(*month);
        }
        TimeType::Date(date) => {
            target.push(4);
            target.extend_from_slice(&date.year().to_le_bytes());
            target.push(date.month() as u8);
            target.push(date.day() as u8);
        }
        TimeType::DateTime(datetime) => {
            target.push(5);
            target.extend_from_slice(&datetime.year().to_le_bytes());
            target.push(datetime.month() as u8);
            target.push(datetime.day() as u8);
            target.push(datetime.hour() as u8);
            target.push(datetime.minute() as u8);
            target.push(datetime.second() as u8);
            target.extend_from_slice(&datetime.nanosecond().to_le_bytes());
        }
    }
}

fn replay_logical(store_uuid: [u8; 16], batches: &[Vec<StoredRecord>]) -> Result<ReplayedStore> {
    let mut replay = LogicalReplay::new(store_uuid);
    for batch in batches {
        replay.apply_batch(batch)?;
    }
    replay.finish()
}

type PropositionKey = (Vec<(Thing, Thing)>, LiteralValue, Time);

struct LogicalReplay {
    store_uuid: [u8; 16],
    roles_by_identity: HashMap<Thing, ReplayedRole>,
    identities_by_name: HashMap<String, Thing>,
    construct_identities: HashSet<Thing>,
    propositions: HashSet<PropositionKey>,
    roles: Vec<ReplayedRole>,
    posits: Vec<ReplayedPosit>,
    codecs: HashSet<BuiltinCodec>,
}

impl LogicalReplay {
    fn new(store_uuid: [u8; 16]) -> Self {
        Self {
            store_uuid,
            roles_by_identity: HashMap::new(),
            identities_by_name: HashMap::new(),
            construct_identities: HashSet::new(),
            propositions: HashSet::new(),
            roles: Vec::new(),
            posits: Vec::new(),
            codecs: HashSet::new(),
        }
    }

    fn apply_batch(&mut self, batch: &[StoredRecord]) -> Result<()> {
        for record in batch {
            self.apply_record(record)?;
        }
        Ok(())
    }

    fn apply_record(&mut self, record: &StoredRecord) -> Result<()> {
        match record.record_type {
            RECORD_ROLE => {
                let role = decode_role(&record.payload)?;
                if self.roles_by_identity.contains_key(&role.identity)
                    || self.identities_by_name.contains_key(&role.name)
                    || !self.construct_identities.insert(role.identity)
                {
                    return record_corruption(record, "duplicate or conflicting Role record");
                }
                self.identities_by_name
                    .insert(role.name.clone(), role.identity);
                self.roles_by_identity.insert(role.identity, role.clone());
                self.roles.push(role);
            }
            RECORD_CODEC => {
                let codec = decode_codec(&record.payload)?;
                if !self.codecs.insert(codec) {
                    return record_corruption(record, "duplicate Codec record");
                }
            }
            RECORD_POSIT => {
                let posit = decode_posit(&record.payload, &self.codecs)
                    .map_err(|error| record_corruption_error(record, error))?;
                if !self.construct_identities.insert(posit.identity) {
                    return record_corruption(record, "duplicate Role or Posit identity");
                }
                if posit
                    .appearances
                    .iter()
                    .any(|(role, _)| !self.roles_by_identity.contains_key(role))
                {
                    return record_corruption(record, "Posit references an unknown Role");
                }
                let proposition = (
                    posit.appearances.clone(),
                    posit.value.clone(),
                    posit.time.clone(),
                );
                if !self.propositions.insert(proposition) {
                    return record_corruption(record, "duplicate Posit proposition");
                }
                self.posits.push(posit);
            }
            other => {
                return record_corruption(
                    record,
                    format!("unsupported logical record type {other}"),
                );
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<ReplayedStore> {
        let has_required_codecs = BuiltinCodec::ALL
            .into_iter()
            .all(|codec| self.codecs.contains(&codec));
        if !self.roles.is_empty() || !self.posits.is_empty() || !self.codecs.is_empty() {
            for (identity, name) in crate::construct::BUILTIN_ROLES {
                validate_builtin_role(
                    &self.roles_by_identity,
                    &self.identities_by_name,
                    identity,
                    name,
                )?;
            }
        }
        if !has_required_codecs
            && (!self.roles.is_empty() || !self.posits.is_empty() || !self.codecs.is_empty())
        {
            return corruption("store lacks one or more mandatory v1 codecs");
        }
        Ok(ReplayedStore {
            store_uuid: self.store_uuid,
            roles: self.roles,
            posits: self.posits,
            has_required_codecs,
        })
    }
}

fn validate_builtin_role(
    roles: &HashMap<Thing, ReplayedRole>,
    names: &HashMap<String, Thing>,
    identity: Thing,
    name: &str,
) -> Result<()> {
    match (roles.get(&identity), names.get(name)) {
        (Some(role), Some(named_identity))
            if role.name == name && role.reserved && *named_identity == identity =>
        {
            Ok(())
        }
        _ => corruption(format!(
            "built-in Role '{name}' must be reserved identity {identity}"
        )),
    }
}

fn decode_role(payload: &[u8]) -> Result<ReplayedRole> {
    use unicode_normalization::UnicodeNormalization;
    let mut cursor = 0;
    let identity = take_u64(payload, &mut cursor, "Role identity")?;
    if identity == 0 {
        return corruption("Role identity 0 is reserved for genesis");
    }
    let reserved = match take_byte(payload, &mut cursor, "Role reserved flag")? {
        0 => false,
        1 => true,
        value => return corruption(format!("invalid Role reserved flag {value}")),
    };
    let name = decode_text(payload, &mut cursor)?;
    if name.is_empty() || name.nfc().collect::<String>() != name || cursor != payload.len() {
        return corruption("invalid canonical Role payload");
    }
    Ok(ReplayedRole {
        identity,
        name,
        reserved,
    })
}

fn decode_codec(payload: &[u8]) -> Result<BuiltinCodec> {
    let mut cursor = 0;
    let identifier = take_u16(payload, &mut cursor, "codec identifier")?;
    let version = take_u16(payload, &mut cursor, "codec version")?;
    let family = take_byte(payload, &mut cursor, "codec family")?;
    let flags = take_u16(payload, &mut cursor, "codec flags")?;
    let name = decode_text(payload, &mut cursor)?;
    let codec = BuiltinCodec::from_pair(identifier, version).ok_or_else(|| {
        DatabaseError::DataCorruption {
            message: "unknown required codec".to_string(),
        }
    })?;
    if family != codec.family() || flags != 0 || name != codec.name() || cursor != payload.len() {
        return corruption("unknown or malformed required codec");
    }
    Ok(codec)
}

fn decode_posit(payload: &[u8], codecs: &HashSet<BuiltinCodec>) -> Result<ReplayedPosit> {
    let mut cursor = 0;
    let identity = take_u64(payload, &mut cursor, "Posit identity")?;
    if identity == 0 {
        return corruption("Posit identity 0 is reserved for genesis");
    }
    let count = usize::try_from(decode_uleb128(payload, &mut cursor)?).map_err(|_| {
        DatabaseError::DataCorruption {
            message: "appearance count exceeds addressable memory".to_string(),
        }
    })?;
    if count == 0 || count > payload.len().saturating_sub(cursor) / 16 {
        return corruption("appearance count exceeds the remaining Posit payload");
    }
    let mut appearances = Vec::with_capacity(count);
    for _ in 0..count {
        let role = take_u64(payload, &mut cursor, "appearance Role")?;
        let thing = take_u64(payload, &mut cursor, "appearing Thing")?;
        if role == 0 || thing == 0 {
            return corruption("appearance identities cannot use genesis identity 0");
        }
        appearances.push((role, thing));
    }
    if appearances.is_empty()
        || appearances.windows(2).any(|pair| pair[0] >= pair[1])
        || appearances.windows(2).any(|pair| pair[0].0 == pair[1].0)
    {
        return corruption("Posit appearances are not canonical");
    }
    let family = LiteralFamily::from_identifier(take_byte(payload, &mut cursor, "literal family")?)
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: "unknown literal family".to_string(),
        })?;
    let codec = take_u16(payload, &mut cursor, "codec identifier")?;
    let codec_version = take_u16(payload, &mut cursor, "codec version")?;
    let codec = BuiltinCodec::from_pair(codec, codec_version)
        .filter(|codec| codecs.contains(codec))
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: "unknown or not-yet-declared Posit codec".to_string(),
        })?;
    let encoded = decode_blob(payload, &mut cursor)?;
    let token = decode_literal(encoded, family, codec)?;
    let value =
        LiteralValue::new(token, family).map_err(|error| DatabaseError::DataCorruption {
            message: format!("invalid persisted literal: {error}"),
        })?;
    let time = decode_time(payload, &mut cursor)?;
    if cursor != payload.len() {
        return corruption("Posit payload has trailing fields");
    }
    Ok(ReplayedPosit {
        identity,
        appearances,
        value,
        time,
    })
}

fn decode_literal(encoded: &[u8], family: LiteralFamily, codec: BuiltinCodec) -> Result<String> {
    match codec {
        BuiltinCodec::RawUtf8 => {
            std::str::from_utf8(encoded)
                .map(str::to_string)
                .map_err(|error| DatabaseError::DataCorruption {
                    message: format!("literal payload is not UTF-8: {error}"),
                })
        }
        BuiltinCodec::CanonicalI64 if family == LiteralFamily::Integer => {
            let mut cursor = 0;
            let zigzag = decode_uleb128(encoded, &mut cursor)?;
            if cursor != encoded.len() {
                return corruption("canonical integer payload has trailing bytes");
            }
            let integer = ((zigzag >> 1) as i64) ^ -((zigzag & 1) as i64);
            Ok(integer.to_string())
        }
        BuiltinCodec::CanonicalCertainty if family == LiteralFamily::Certainty => {
            let [encoded] = encoded else {
                return corruption("canonical certainty payload must contain one byte");
            };
            let percent = *encoded as i8;
            if !(-100..=100).contains(&percent) {
                return corruption("canonical certainty payload is outside -100..=100");
            }
            Ok(format!("{percent}%"))
        }
        _ => corruption("Posit codec is incompatible with its literal family"),
    }
}

fn decode_time(payload: &[u8], cursor: &mut usize) -> Result<Time> {
    let moment = match take_byte(payload, cursor, "time tag")? {
        0 => TimeType::BeginningOfTime,
        1 => TimeType::EndOfTime,
        2 => TimeType::Year(take_i32(payload, cursor, "time year")?),
        3 => {
            let year = take_i32(payload, cursor, "time year")?;
            let month = take_byte(payload, cursor, "time month")?;
            if !(1..=12).contains(&month) {
                return corruption("invalid time month");
            }
            TimeType::YearMonth(year, month)
        }
        4 | 5 => {
            let tag = payload[*cursor - 1];
            let year = take_i32(payload, cursor, "time year")?;
            let month = take_byte(payload, cursor, "time month")?;
            let day = take_byte(payload, cursor, "time day")?;
            let date = NaiveDate::from_ymd_opt(year, u32::from(month), u32::from(day)).ok_or_else(
                || DatabaseError::DataCorruption {
                    message: "invalid persisted calendar date".to_string(),
                },
            )?;
            if tag == 4 {
                TimeType::Date(date)
            } else {
                let hour = take_byte(payload, cursor, "time hour")?;
                let minute = take_byte(payload, cursor, "time minute")?;
                let second = take_byte(payload, cursor, "time second")?;
                let nanosecond = take_u32(payload, cursor, "time nanosecond")?;
                let datetime = date
                    .and_hms_nano_opt(
                        u32::from(hour),
                        u32::from(minute),
                        u32::from(second),
                        nanosecond,
                    )
                    .ok_or_else(|| DatabaseError::DataCorruption {
                        message: "invalid persisted UTC datetime".to_string(),
                    })?;
                TimeType::DateTime(datetime)
            }
        }
        tag => return corruption(format!("unknown time tag {tag}")),
    };
    Ok(Time::from_time_type(moment))
}

fn record_corruption<T>(record: &StoredRecord, message: impl Into<String>) -> Result<T> {
    corruption(format!(
        "record sequence {}: {}",
        record.sequence,
        message.into()
    ))
}

fn record_corruption_error(record: &StoredRecord, error: DatabaseError) -> DatabaseError {
    DatabaseError::DataCorruption {
        message: format!("record sequence {}: {error}", record.sequence),
    }
}

impl StoreBackend for LogStore {
    fn append_batch(&mut self, records: &[PendingRecord]) -> Result<Vec<StoredRecord>> {
        if self.poisoned {
            return Err(DatabaseError::Persistence(
                "store writer is fail-closed after an earlier uncertain write; reopen the store"
                    .to_string(),
            ));
        }
        validate_pending_records(records)?;
        #[cfg(test)]
        let injected_failure = self.injected_failure.take();
        #[cfg(test)]
        if injected_failure == Some(InjectedFailure::LogWrite) {
            self.poisoned = true;
            return Err(DatabaseError::Persistence(
                "injected log write failure (simulated disk full)".to_string(),
            ));
        }
        let old_length = self.manifest.committed_length;
        let mut sequence = self.next_sequence;
        let mut framed = Vec::new();
        let mut stored = Vec::with_capacity(records.len());
        for record in records {
            let stored_record = StoredRecord {
                record_type: record.record_type,
                version: record.version,
                flags: record.flags,
                sequence,
                payload: record.payload.clone(),
            };
            framed.extend_from_slice(&encode_frame(&stored_record)?);
            stored.push(stored_record);
            sequence = sequence
                .checked_add(1)
                .ok_or_else(|| DatabaseError::Invariant("record sequence exhausted".to_string()))?;
        }

        let first = stored.first().map_or(0, |record| record.sequence);
        let last = stored.last().map_or(0, |record| record.sequence);
        let mut commit_payload = Vec::new();
        commit_payload.extend_from_slice(&self.next_batch.to_le_bytes());
        commit_payload.extend_from_slice(&first.to_le_bytes());
        commit_payload.extend_from_slice(&last.to_le_bytes());
        encode_uleb128(records.len() as u64, &mut commit_payload);
        let commit = StoredRecord {
            record_type: RECORD_COMMIT,
            version: 1,
            flags: 0,
            sequence,
            payload: commit_payload,
        };
        framed.extend_from_slice(&encode_frame(&commit)?);

        if let Err(error) = self
            .log
            .seek(SeekFrom::Start(old_length))
            .and_then(|_| self.log.write_all(&framed))
            .and_then(|_| self.log.sync_all())
        {
            self.poisoned = true;
            return Err(DatabaseError::from_io(error));
        }
        let new_length = old_length
            .checked_add(framed.len() as u64)
            .ok_or_else(|| DatabaseError::Invariant("log length exhausted".to_string()))?;
        let new_manifest = Manifest {
            store_uuid: self.manifest.store_uuid,
            committed_length: new_length,
        };
        #[cfg(test)]
        if injected_failure == Some(InjectedFailure::ManifestCommit) {
            self.poisoned = true;
            return Err(DatabaseError::Persistence(
                "injected manifest commit failure".to_string(),
            ));
        }
        if let Err(error) = write_manifest(&self.directory, &new_manifest) {
            self.poisoned = true;
            return Err(error);
        }

        self.manifest = new_manifest;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("record sequence exhausted".to_string()))?;
        self.next_batch = self
            .next_batch
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("batch sequence exhausted".to_string()))?;
        self.batch_count = self
            .batch_count
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("batch count exhausted".to_string()))?;
        Ok(stored)
    }

    fn flush(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(DatabaseError::Persistence(
                "store writer is fail-closed after an earlier uncertain write; reopen the store"
                    .to_string(),
            ));
        }
        self.log.sync_all().map_err(DatabaseError::from_io)
    }
}

fn validate_pending_records(records: &[PendingRecord]) -> Result<()> {
    for record in records {
        if !matches!(
            record.record_type,
            RECORD_ROLE | RECORD_CODEC | RECORD_POSIT
        ) {
            return Err(DatabaseError::Invariant(format!(
                "unsupported pending record type {}",
                record.record_type
            )));
        }
        if record.version != 1 || record.flags != 0 {
            return Err(DatabaseError::Invariant(format!(
                "unsupported record version/flags {}/{}",
                record.version, record.flags
            )));
        }
    }
    Ok(())
}

fn initialize_store(directory: &Path) -> Result<Manifest> {
    let mut store_uuid = [0u8; 16];
    getrandom::fill(&mut store_uuid).map_err(|error| {
        DatabaseError::Persistence(format!("failed to generate store UUID: {error}"))
    })?;
    store_uuid[6] = (store_uuid[6] & 0x0f) | 0x40;
    store_uuid[8] = (store_uuid[8] & 0x3f) | 0x80;

    let log_path = directory.join(LOG_NAME);
    let mut log = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&log_path)
        .map_err(DatabaseError::from_io)?;
    log.write_all(&encode_log_header(store_uuid))
        .and_then(|_| log.sync_all())
        .map_err(DatabaseError::from_io)?;
    let manifest = Manifest {
        store_uuid,
        committed_length: u64::from(LOG_HEADER_LEN),
    };
    write_manifest(directory, &manifest)?;
    sync_parent_directory(directory)?;
    Ok(manifest)
}

fn encode_manifest(manifest: &Manifest) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MANIFEST_MAGIC);
    bytes.extend_from_slice(&FORMAT_MAJOR.to_le_bytes());
    bytes.extend_from_slice(&FORMAT_MINOR.to_le_bytes());
    bytes.extend_from_slice(&MANIFEST_HEADER_LEN.to_le_bytes());
    bytes.extend_from_slice(&manifest.store_uuid);
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    encode_uleb128(1, &mut bytes);
    encode_text(LOG_NAME, &mut bytes);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&manifest.committed_length.to_le_bytes());
    let checksum = crc32c(&bytes);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    bytes
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(DatabaseError::from_io)?;
    if bytes.len() < MANIFEST_HEADER_LEN as usize + 4 {
        return corruption("manifest is truncated");
    }
    let checksum_offset = bytes.len() - 4;
    let stored_checksum = u32::from_le_bytes(bytes[checksum_offset..].try_into().unwrap());
    if crc32c(&bytes[..checksum_offset]) != stored_checksum {
        return corruption("manifest checksum mismatch");
    }
    let mut cursor = 0usize;
    expect_bytes(&bytes, &mut cursor, MANIFEST_MAGIC, "manifest magic")?;
    expect_u16(&bytes, &mut cursor, FORMAT_MAJOR, "manifest major version")?;
    expect_u16(&bytes, &mut cursor, FORMAT_MINOR, "manifest minor version")?;
    expect_u32(
        &bytes,
        &mut cursor,
        MANIFEST_HEADER_LEN,
        "manifest header length",
    )?;
    let store_uuid = take_array::<16>(&bytes, &mut cursor, "store UUID")?;
    expect_u64(&bytes, &mut cursor, 0, "required manifest flags")?;
    expect_u64(&bytes, &mut cursor, 0, "optional manifest flags")?;
    let segment_count = decode_uleb128(&bytes, &mut cursor)?;
    if segment_count != 1 {
        return corruption(format!(
            "v1 implementation requires one log segment, found {segment_count}"
        ));
    }
    let log_name = decode_text(&bytes, &mut cursor)?;
    if log_name != LOG_NAME {
        return corruption(format!("unexpected log segment name '{log_name}'"));
    }
    expect_u64(&bytes, &mut cursor, 1, "segment ordinal")?;
    let sealed = take_byte(&bytes, &mut cursor, "segment sealed flag")?;
    if sealed != 0 {
        return corruption("the only v1 log segment must be active");
    }
    let committed_length = take_u64(&bytes, &mut cursor, "committed length")?;
    if cursor != checksum_offset {
        return corruption("manifest has trailing fields");
    }
    Ok(Manifest {
        store_uuid,
        committed_length,
    })
}

fn write_manifest(directory: &Path, manifest: &Manifest) -> Result<()> {
    let next_path = directory.join(MANIFEST_NEXT_NAME);
    let manifest_path = directory.join(MANIFEST_NAME);
    let bytes = encode_manifest(manifest);
    let mut next = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&next_path)
        .map_err(DatabaseError::from_io)?;
    next.write_all(&bytes)
        .and_then(|_| next.sync_all())
        .map_err(DatabaseError::from_io)?;
    std::fs::rename(&next_path, &manifest_path).map_err(DatabaseError::from_io)?;
    sync_directory(directory)?;
    Ok(())
}

pub(crate) fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(DatabaseError::from_io)?;
    }
    Ok(())
}

pub(crate) fn sync_parent_directory(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        sync_directory(parent)?;
    }
    Ok(())
}

fn encode_log_header(store_uuid: [u8; 16]) -> [u8; LOG_HEADER_LEN as usize] {
    let mut bytes = [0u8; LOG_HEADER_LEN as usize];
    bytes[0..8].copy_from_slice(LOG_MAGIC);
    bytes[8..10].copy_from_slice(&FORMAT_MAJOR.to_le_bytes());
    bytes[10..12].copy_from_slice(&FORMAT_MINOR.to_le_bytes());
    bytes[12..16].copy_from_slice(&LOG_HEADER_LEN.to_le_bytes());
    bytes[16..32].copy_from_slice(&store_uuid);
    bytes[32..40].copy_from_slice(&1u64.to_le_bytes());
    let checksum = crc32c(&bytes[..60]);
    bytes[60..64].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

fn verify_log_header(log: &mut File, store_uuid: [u8; 16]) -> Result<()> {
    let mut bytes = [0u8; LOG_HEADER_LEN as usize];
    log.seek(SeekFrom::Start(0))
        .and_then(|_| log.read_exact(&mut bytes))
        .map_err(DatabaseError::from_io)?;
    if &bytes[0..8] != LOG_MAGIC
        || u16::from_le_bytes(bytes[8..10].try_into().unwrap()) != FORMAT_MAJOR
        || u16::from_le_bytes(bytes[10..12].try_into().unwrap()) != FORMAT_MINOR
        || u32::from_le_bytes(bytes[12..16].try_into().unwrap()) != LOG_HEADER_LEN
        || bytes[16..32] != store_uuid
        || u64::from_le_bytes(bytes[32..40].try_into().unwrap()) != 1
        || bytes[40..60].iter().any(|byte| *byte != 0)
    {
        return corruption("invalid log header metadata");
    }
    let checksum = u32::from_le_bytes(bytes[60..64].try_into().unwrap());
    if crc32c(&bytes[..60]) != checksum {
        return corruption("log header checksum mismatch");
    }
    Ok(())
}

fn encode_frame(record: &StoredRecord) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(FRAME_MAGIC);
    bytes.push(record.version);
    bytes.push(record.record_type);
    bytes.extend_from_slice(&record.flags.to_le_bytes());
    bytes.extend_from_slice(&record.sequence.to_le_bytes());
    encode_uleb128(record.payload.len() as u64, &mut bytes);
    let complete_length = bytes
        .len()
        .checked_add(record.payload.len())
        .and_then(|length| length.checked_add(4))
        .ok_or_else(|| DatabaseError::Invariant("record frame length overflow".to_string()))?;
    if complete_length > MAX_FRAME_SIZE {
        return Err(DatabaseError::Persistence(format!(
            "record frame is {complete_length} bytes; maximum is {MAX_FRAME_SIZE}"
        )));
    }
    bytes.extend_from_slice(&record.payload);
    let checksum = crc32c(&bytes);
    bytes.extend_from_slice(&checksum.to_le_bytes());
    Ok(bytes)
}

fn replay_committed_logical(
    log: &mut File,
    committed_length: u64,
    store_uuid: [u8; 16],
) -> Result<(ReplayMetadata, ReplayedStore)> {
    let mut logical = LogicalReplay::new(store_uuid);
    let metadata = scan_committed(log, committed_length, |batch| logical.apply_batch(batch))?;
    Ok((metadata, logical.finish()?))
}

fn scan_committed(
    log: &mut File,
    committed_length: u64,
    mut on_batch: impl FnMut(&[StoredRecord]) -> Result<()>,
) -> Result<ReplayMetadata> {
    let mut offset = u64::from(LOG_HEADER_LEN);
    let mut expected_sequence = 1u64;
    let mut expected_batch = 1u64;
    let mut batch_count = 0usize;
    let mut staged = Vec::new();
    log.seek(SeekFrom::Start(offset))
        .map_err(DatabaseError::from_io)?;
    let mut reader = BufReader::new(log);
    while offset < committed_length {
        let (record, next_offset) = read_frame(&mut reader, offset, committed_length)?;
        if record.sequence != expected_sequence {
            return corruption_at(
                offset,
                Some(record.sequence),
                format!(
                    "record sequence gap: expected {expected_sequence}, found {}",
                    record.sequence
                ),
            );
        }
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("record sequence exhausted".to_string()))?;
        match record.record_type {
            RECORD_ROLE | RECORD_CODEC | RECORD_POSIT => staged.push(record),
            RECORD_COMMIT => {
                validate_commit(&record, expected_batch, &staged, offset)?;
                on_batch(&staged)?;
                staged.clear();
                batch_count = batch_count
                    .checked_add(1)
                    .ok_or_else(|| DatabaseError::Invariant("batch count exhausted".to_string()))?;
                expected_batch = expected_batch.checked_add(1).ok_or_else(|| {
                    DatabaseError::Invariant("batch sequence exhausted".to_string())
                })?;
            }
            other => {
                return corruption_at(
                    offset,
                    Some(record.sequence),
                    format!("unknown mandatory record type {other}"),
                );
            }
        }
        offset = next_offset;
    }
    if offset != committed_length || !staged.is_empty() {
        return corruption_at(
            offset,
            staged.first().map(|record| record.sequence),
            "manifest committed length ends outside a Commit frame",
        );
    }
    Ok(ReplayMetadata {
        next_sequence: expected_sequence,
        next_batch: expected_batch,
        batch_count,
    })
}

fn read_frame(source: &mut impl Read, offset: u64, limit: u64) -> Result<(StoredRecord, u64)> {
    let mut prefix = [0u8; 16];
    source
        .read_exact(&mut prefix)
        .map_err(|error| frame_io_error(offset, None, error))?;
    if &prefix[0..4] != FRAME_MAGIC {
        return corruption_at(offset, None, "record sync word mismatch");
    }
    let version = prefix[4];
    let record_type = prefix[5];
    let flags = u16::from_le_bytes(prefix[6..8].try_into().unwrap());
    let sequence = u64::from_le_bytes(prefix[8..16].try_into().unwrap());
    if version != 1 || flags != 0 {
        return corruption_at(
            offset,
            Some(sequence),
            format!("unsupported frame version/flags {version}/{flags}"),
        );
    }

    let mut framed = prefix.to_vec();
    let payload_length = read_uleb128(source, &mut framed, offset, sequence)?;
    let payload_length =
        usize::try_from(payload_length).map_err(|_| DatabaseError::DataCorruption {
            message: format!("log offset {offset}, sequence {sequence}: payload too large"),
        })?;
    let complete_length = framed
        .len()
        .checked_add(payload_length)
        .and_then(|length| length.checked_add(4))
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: format!("log offset {offset}, sequence {sequence}: frame length overflow"),
        })?;
    if complete_length > MAX_FRAME_SIZE
        || offset
            .checked_add(complete_length as u64)
            .is_none_or(|end| end > limit)
    {
        return corruption_at(
            offset,
            Some(sequence),
            format!("invalid frame length {complete_length}"),
        );
    }
    let mut payload = vec![0u8; payload_length];
    source
        .read_exact(&mut payload)
        .map_err(|error| frame_io_error(offset, Some(sequence), error))?;
    framed.extend_from_slice(&payload);
    let mut checksum_bytes = [0u8; 4];
    source
        .read_exact(&mut checksum_bytes)
        .map_err(|error| frame_io_error(offset, Some(sequence), error))?;
    let stored_checksum = u32::from_le_bytes(checksum_bytes);
    if crc32c(&framed) != stored_checksum {
        return corruption_at(offset, Some(sequence), "record checksum mismatch");
    }
    Ok((
        StoredRecord {
            record_type,
            version,
            flags,
            sequence,
            payload,
        },
        offset + complete_length as u64,
    ))
}

fn validate_commit(
    commit: &StoredRecord,
    expected_batch: u64,
    staged: &[StoredRecord],
    offset: u64,
) -> Result<()> {
    let mut cursor = 0usize;
    let batch = take_u64(&commit.payload, &mut cursor, "commit batch")?;
    let first = take_u64(&commit.payload, &mut cursor, "commit first sequence")?;
    let last = take_u64(&commit.payload, &mut cursor, "commit last sequence")?;
    let count = decode_uleb128(&commit.payload, &mut cursor)?;
    if cursor != commit.payload.len() {
        return corruption_at(offset, Some(commit.sequence), "commit has trailing fields");
    }
    let expected_first = staged.first().map_or(0, |record| record.sequence);
    let expected_last = staged.last().map_or(0, |record| record.sequence);
    if batch != expected_batch
        || first != expected_first
        || last != expected_last
        || count != staged.len() as u64
    {
        return corruption_at(
            offset,
            Some(commit.sequence),
            format!("invalid Commit: batch/range/count ({batch}, {first}, {last}, {count})"),
        );
    }
    Ok(())
}

fn encode_text(value: &str, target: &mut Vec<u8>) {
    encode_blob(value.as_bytes(), target);
}

fn encode_blob(value: &[u8], target: &mut Vec<u8>) {
    encode_uleb128(value.len() as u64, target);
    target.extend_from_slice(value);
}

fn decode_text(bytes: &[u8], cursor: &mut usize) -> Result<String> {
    let encoded = decode_blob(bytes, cursor)?;
    std::str::from_utf8(encoded)
        .map_err(|error| DatabaseError::DataCorruption {
            message: format!("invalid UTF-8 text: {error}"),
        })
        .map(str::to_string)
}

fn decode_blob<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8]> {
    let length = usize::try_from(decode_uleb128(bytes, cursor)?).map_err(|_| {
        DatabaseError::DataCorruption {
            message: "byte-string length exceeds addressable memory".to_string(),
        }
    })?;
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: "truncated byte-string field".to_string(),
        })?;
    let value = &bytes[*cursor..end];
    *cursor = end;
    Ok(value)
}

fn encode_uleb128(mut value: u64, target: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        target.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn decode_uleb128(bytes: &[u8], cursor: &mut usize) -> Result<u64> {
    let start = *cursor;
    let mut value = 0u64;
    for shift in (0..=63).step_by(7) {
        let byte = take_byte(bytes, cursor, "ULEB128")?;
        let part = u64::from(byte & 0x7f);
        if shift == 63 && part > 1 {
            return corruption("ULEB128 overflow");
        }
        value |= part << shift;
        if byte & 0x80 == 0 {
            if *cursor - start > 1 && byte == 0 {
                return corruption("non-canonical ULEB128");
            }
            return Ok(value);
        }
    }
    corruption("ULEB128 overflow")
}

fn read_uleb128(
    source: &mut impl Read,
    framed: &mut Vec<u8>,
    offset: u64,
    sequence: u64,
) -> Result<u64> {
    let mut value = 0u64;
    for (index, shift) in (0..=63).step_by(7).enumerate() {
        let mut byte = [0u8; 1];
        source
            .read_exact(&mut byte)
            .map_err(|error| frame_io_error(offset, Some(sequence), error))?;
        framed.push(byte[0]);
        let part = u64::from(byte[0] & 0x7f);
        if shift == 63 && part > 1 {
            return corruption_at(offset, Some(sequence), "ULEB128 overflow");
        }
        value |= part << shift;
        if byte[0] & 0x80 == 0 {
            if index > 0 && byte[0] == 0 {
                return corruption_at(offset, Some(sequence), "non-canonical ULEB128");
            }
            return Ok(value);
        }
    }
    corruption_at(offset, Some(sequence), "ULEB128 overflow")
}

fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0x82f6_3b78
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn expect_bytes(bytes: &[u8], cursor: &mut usize, expected: &[u8], field: &str) -> Result<()> {
    let end = cursor
        .checked_add(expected.len())
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: format!("truncated {field}"),
        })?;
    if &bytes[*cursor..end] != expected {
        return corruption(format!("invalid {field}"));
    }
    *cursor = end;
    Ok(())
}

fn take_array<const N: usize>(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<[u8; N]> {
    let end = cursor
        .checked_add(N)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: format!("truncated {field}"),
        })?;
    let value = bytes[*cursor..end].try_into().unwrap();
    *cursor = end;
    Ok(value)
}

fn take_byte(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<u8> {
    Ok(take_array::<1>(bytes, cursor, field)?[0])
}

fn take_u16(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<u16> {
    Ok(u16::from_le_bytes(take_array::<2>(bytes, cursor, field)?))
}

fn take_u32(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<u32> {
    Ok(u32::from_le_bytes(take_array::<4>(bytes, cursor, field)?))
}

fn take_i32(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<i32> {
    Ok(i32::from_le_bytes(take_array::<4>(bytes, cursor, field)?))
}

fn take_u64(bytes: &[u8], cursor: &mut usize, field: &str) -> Result<u64> {
    Ok(u64::from_le_bytes(take_array::<8>(bytes, cursor, field)?))
}

fn expect_u16(bytes: &[u8], cursor: &mut usize, expected: u16, field: &str) -> Result<()> {
    let found = u16::from_le_bytes(take_array::<2>(bytes, cursor, field)?);
    if found != expected {
        return corruption(format!("invalid {field}: {found}"));
    }
    Ok(())
}

fn expect_u32(bytes: &[u8], cursor: &mut usize, expected: u32, field: &str) -> Result<()> {
    let found = u32::from_le_bytes(take_array::<4>(bytes, cursor, field)?);
    if found != expected {
        return corruption(format!("invalid {field}: {found}"));
    }
    Ok(())
}

fn expect_u64(bytes: &[u8], cursor: &mut usize, expected: u64, field: &str) -> Result<()> {
    let found = take_u64(bytes, cursor, field)?;
    if found != expected {
        return corruption(format!("invalid {field}: {found}"));
    }
    Ok(())
}

fn corruption<T>(message: impl Into<String>) -> Result<T> {
    Err(DatabaseError::DataCorruption {
        message: message.into(),
    })
}

fn corruption_at<T>(offset: u64, sequence: Option<u64>, message: impl Into<String>) -> Result<T> {
    let sequence = sequence.map_or_else(|| "unknown".to_string(), |value| value.to_string());
    corruption(format!(
        "segment {LOG_NAME}, byte offset {offset}, record sequence {sequence}: {}",
        message.into()
    ))
}

fn frame_io_error(offset: u64, sequence: Option<u64>, error: std::io::Error) -> DatabaseError {
    DatabaseError::DataCorruption {
        message: format!(
            "segment {LOG_NAME}, byte offset {offset}, record sequence {}: truncated frame: {error}",
            sequence.map_or_else(|| "unknown".to_string(), |value| value.to_string())
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::construct::{Appearance, AppearanceSet};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    struct TemporaryStore(PathBuf);

    impl TemporaryStore {
        fn new(label: &str) -> Self {
            let ordinal = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "positorium-log-{label}-{}-{ordinal}",
                std::process::id()
            ));
            Self(path)
        }
    }

    impl Drop for TemporaryStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn stored(record: PendingRecord, sequence: u64) -> StoredRecord {
        StoredRecord {
            record_type: record.record_type,
            version: record.version,
            flags: record.flags,
            sequence,
            payload: record.payload,
        }
    }

    fn builtin_role_records() -> Vec<StoredRecord> {
        crate::construct::BUILTIN_ROLES
            .into_iter()
            .enumerate()
            .map(|(index, (identity, name))| {
                stored(
                    role_record(&Role::new(identity, name.to_string(), true)),
                    1 + index as u64,
                )
            })
            .collect()
    }

    #[test]
    fn logical_records_round_trip_every_literal_family_and_time_precision() {
        let value_role = Arc::new(Role::new(6, "value".to_string(), false));
        let appearance = Arc::new(Appearance::new(42, Arc::clone(&value_role)));
        let appearance_set = Arc::new(AppearanceSet::new(vec![appearance]).unwrap());
        let values = [
            LiteralValue::new(r#""\u0041""#, LiteralFamily::String).unwrap(),
            LiteralValue::new("+001", LiteralFamily::Integer).unwrap(),
            LiteralValue::new("01.00", LiteralFamily::Decimal).unwrap(),
            LiteralValue::new("075%", LiteralFamily::Certainty).unwrap(),
            LiteralValue::new(r#"{ "a": 1.00 }"#, LiteralFamily::Json).unwrap(),
            LiteralValue::new("@BOT", LiteralFamily::Time).unwrap(),
        ];
        let times = [
            Time::new_beginning_of_time(),
            Time::new_end_of_time(),
            Time::new_year_from("2024").unwrap(),
            Time::new_year_month_from("2024-05").unwrap(),
            Time::new_date_from("2024-05-06").unwrap(),
            Time::new_datetime_from("2024-05-06T07:08:09.123456789").unwrap(),
        ];
        let mut batch = builtin_role_records();
        batch.push(stored(role_record(&value_role), 6));
        batch.extend(
            codec_records()
                .into_iter()
                .enumerate()
                .map(|(index, record)| stored(record, 7 + index as u64)),
        );
        for (index, (value, time)) in values.iter().zip(times.iter()).enumerate() {
            let posit = Posit::new(
                100 + index as u64,
                Arc::clone(&appearance_set),
                value.clone(),
                time.clone(),
            );
            batch.push(stored(posit_record(&posit).unwrap(), 10 + index as u64));
        }

        let replayed = replay_logical([7; 16], &[batch]).unwrap();
        assert_eq!(replayed.store_uuid, [7; 16]);
        assert!(replayed.has_required_codecs);
        assert_eq!(replayed.roles.len(), 6);
        assert_eq!(replayed.posits.len(), values.len());
        for (index, posit) in replayed.posits.iter().enumerate() {
            assert_eq!(posit.identity, 100 + index as u64);
            assert_eq!(posit.appearances, vec![(6, 42)]);
            assert_eq!(posit.value, values[index]);
            assert_eq!(posit.time, times[index]);
        }
    }

    #[test]
    fn compact_and_raw_codecs_reconstruct_one_logical_literal_identity() {
        let value_role = Role::new(6, "value".to_string(), false);
        let value = LiteralValue::new("9223372036854775807", LiteralFamily::Integer).unwrap();
        let padded = LiteralValue::new("+001", LiteralFamily::Integer).unwrap();
        let certainty = LiteralValue::new("75%", LiteralFamily::Certainty).unwrap();
        let padded_certainty = LiteralValue::new("075%", LiteralFamily::Certainty).unwrap();
        assert_eq!(select_codec(&value), BuiltinCodec::CanonicalI64);
        assert_eq!(select_codec(&padded), BuiltinCodec::RawUtf8);
        assert_eq!(select_codec(&certainty), BuiltinCodec::CanonicalCertainty);
        assert_eq!(select_codec(&padded_certainty), BuiltinCodec::RawUtf8);

        let time = Time::new_date_from("2024-05-06").unwrap();
        let appearances = [(6, 42)];
        let compact = posit_record_logical_with_codec(
            100,
            &appearances,
            &value,
            &time,
            BuiltinCodec::CanonicalI64,
        )
        .unwrap();
        let raw = posit_record_logical_with_codec(
            101,
            &appearances,
            &value,
            &time,
            BuiltinCodec::RawUtf8,
        )
        .unwrap();
        assert_ne!(compact.payload, raw.payload);
        assert!(compact.payload.len() < raw.payload.len());

        let metadata = || {
            let mut batch = builtin_role_records();
            batch.push(stored(role_record(&value_role), 6));
            batch.extend(
                codec_records()
                    .into_iter()
                    .enumerate()
                    .map(|(index, record)| stored(record, 7 + index as u64)),
            );
            batch
        };

        for record in [compact.clone(), raw.clone()] {
            let mut batch = metadata();
            batch.push(stored(record, 10));
            let replayed = replay_logical([6; 16], &[batch]).unwrap();
            assert_eq!(replayed.posits[0].value, value);
        }

        let mut duplicate = metadata();
        duplicate.push(stored(compact, 10));
        duplicate.push(stored(raw, 11));
        let error = replay_logical([6; 16], &[duplicate]).unwrap_err();
        assert!(
            error.to_string().contains("duplicate Posit proposition"),
            "{error}"
        );
    }

    #[test]
    fn logical_replay_rejects_dangling_roles_and_duplicate_propositions() {
        let missing_role = Arc::new(Role::new(9, "missing".to_string(), false));
        let appearance = Arc::new(Appearance::new(42, missing_role));
        let appearance_set = Arc::new(AppearanceSet::new(vec![appearance]).unwrap());
        let value = LiteralValue::new("10", LiteralFamily::Integer).unwrap();
        let posit = Posit::new(
            100,
            appearance_set,
            value,
            Time::new_date_from("2024-05-06").unwrap(),
        );
        let mut base = builtin_role_records();
        base.extend(
            codec_records()
                .into_iter()
                .enumerate()
                .map(|(index, record)| stored(record, 6 + index as u64)),
        );
        let mut dangling = base.clone();
        dangling.push(stored(posit_record(&posit).unwrap(), 9));
        let error = replay_logical([8; 16], &[dangling]).unwrap_err();
        assert!(error.to_string().contains("unknown Role"), "{error}");

        let missing_role_record = Role::new(9, "missing".to_string(), false);
        let mut duplicate = base;
        duplicate.push(stored(role_record(&missing_role_record), 9));
        duplicate.push(stored(posit_record(&posit).unwrap(), 10));
        let mut second = posit_record(&posit).unwrap();
        second.payload[0..8].copy_from_slice(&101u64.to_le_bytes());
        duplicate.push(stored(second, 11));
        let error = replay_logical([8; 16], &[duplicate]).unwrap_err();
        assert!(
            error.to_string().contains("duplicate Posit proposition"),
            "{error}"
        );

        let posit_role = Role::new(1, "posit".to_string(), true);
        let ascertains_role = Role::new(2, "ascertains".to_string(), true);
        let mut duplicate_role = vec![
            stored(role_record(&posit_role), 1),
            stored(role_record(&posit_role), 2),
        ];
        duplicate_role.push(stored(role_record(&ascertains_role), 3));
        duplicate_role.extend(
            codec_records()
                .into_iter()
                .enumerate()
                .map(|(index, record)| stored(record, 4 + index as u64)),
        );
        let error = replay_logical([8; 16], &[duplicate_role]).unwrap_err();
        assert!(error.to_string().contains("duplicate or conflicting Role"));
    }

    #[test]
    fn logical_replay_rejects_unknown_required_codecs() {
        let mut codecs = codec_records();
        codecs[0].payload[2..4].copy_from_slice(&2u16.to_le_bytes());
        let mut batch = builtin_role_records();
        batch.extend(
            codecs
                .into_iter()
                .enumerate()
                .map(|(index, record)| stored(record, 6 + index as u64)),
        );
        let error = replay_logical([9; 16], &[batch]).unwrap_err();
        assert!(error.to_string().contains("unknown required codec"));
    }

    #[test]
    fn crc32c_matches_the_standard_check_value() {
        assert_eq!(crc32c(b"123456789"), 0xe306_9283);
    }

    #[test]
    fn uleb128_is_canonical() {
        let mut bytes = Vec::new();
        encode_uleb128(624_485, &mut bytes);
        assert_eq!(bytes, [0xe5, 0x8e, 0x26]);
        let mut cursor = 0;
        assert_eq!(decode_uleb128(&bytes, &mut cursor).unwrap(), 624_485);
        assert_eq!(cursor, bytes.len());

        let mut cursor = 0;
        assert!(decode_uleb128(&[0x80, 0x00], &mut cursor).is_err());
    }

    #[test]
    fn arbitrary_storage_payloads_return_bounded_errors_without_panicking() {
        let codecs = BuiltinCodec::ALL.into_iter().collect::<HashSet<_>>();
        for seed in 0_u16..1024 {
            let length = usize::from(seed % 512);
            let bytes = (0..length)
                .map(|index| {
                    seed.wrapping_mul(109)
                        .wrapping_add((index as u16).wrapping_mul(197)) as u8
                })
                .collect::<Vec<_>>();
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                let _ = decode_role(&bytes);
                let _ = decode_codec(&bytes);
                let _ = decode_posit(&bytes, &codecs);
                let mut cursor = 0;
                let _ = decode_time(&bytes, &mut cursor);
                let mut cursor = 0;
                let _ = decode_uleb128(&bytes, &mut cursor);
            }));
            assert!(outcome.is_ok(), "storage payload panicked for seed {seed}");
        }
    }

    #[test]
    fn memory_batches_receive_monotonic_sequences() {
        let mut store = MemoryStore::new();
        let first = store
            .append_batch(&[PendingRecord::new(RECORD_ROLE, vec![1])])
            .unwrap();
        let second = store
            .append_batch(&[PendingRecord::new(RECORD_POSIT, vec![2])])
            .unwrap();
        assert_eq!(first[0].sequence, 1);
        assert_eq!(second[0].sequence, 2);
        assert_eq!(store.replay().len(), 2);
        store.flush().unwrap();
    }

    #[test]
    fn acknowledged_batches_survive_reopen_without_an_extra_flush() {
        let directory = TemporaryStore::new("reopen");
        let (uuid, committed_length) = {
            let mut store = LogStore::open(&directory.0).unwrap();
            let records = store
                .append_batch(&[
                    PendingRecord::new(RECORD_ROLE, b"role".to_vec()),
                    PendingRecord::new(RECORD_CODEC, b"codec".to_vec()),
                    PendingRecord::new(RECORD_POSIT, b"posit".to_vec()),
                ])
                .unwrap();
            assert_eq!(
                records
                    .iter()
                    .map(|record| record.sequence)
                    .collect::<Vec<_>>(),
                [1, 2, 3]
            );
            (store.store_uuid(), store.committed_length())
        };

        let mut reopened = LogStore::open(&directory.0).unwrap();
        assert_eq!(reopened.store_uuid(), uuid);
        assert_eq!(reopened.committed_length(), committed_length);
        let batches = reopened.committed_batches().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].len(), 3);
        assert_eq!(batches[0][2].payload, b"posit");
    }

    #[test]
    fn a_second_store_owner_is_rejected() {
        let directory = TemporaryStore::new("lock");
        let first = LogStore::open(&directory.0).unwrap();
        let error = match LogStore::open(&directory.0) {
            Ok(_) => panic!("second owner must not open the store"),
            Err(error) => error,
        };
        assert!(format!("{error}").contains("already owned"));
        drop(first);
        LogStore::open(&directory.0).unwrap();
    }

    #[test]
    fn writable_recovery_truncates_only_the_uncommitted_tail() {
        let directory = TemporaryStore::new("tail");
        let committed_length = {
            let mut store = LogStore::open(&directory.0).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"role".to_vec())])
                .unwrap();
            store.committed_length()
        };
        let log_path = directory.0.join(LOG_NAME);
        OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap()
            .write_all(b"torn-tail")
            .unwrap();
        assert!(std::fs::metadata(&log_path).unwrap().len() > committed_length);

        let reopened = LogStore::open(&directory.0).unwrap();
        assert_eq!(reopened.batch_count(), 1);
        assert_eq!(
            std::fs::metadata(&log_path).unwrap().len(),
            committed_length
        );
    }

    #[test]
    fn committed_checksum_corruption_reports_offset_and_sequence() {
        let directory = TemporaryStore::new("corrupt");
        {
            let mut store = LogStore::open(&directory.0).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"role".to_vec())])
                .unwrap();
        }
        let log_path = directory.0.join(LOG_NAME);
        let payload_offset = u64::from(LOG_HEADER_LEN) + 16 + 1;
        let mut log = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&log_path)
            .unwrap();
        log.seek(SeekFrom::Start(payload_offset)).unwrap();
        log.write_all(b"R").unwrap();
        log.sync_all().unwrap();

        let error = match LogStore::open(&directory.0) {
            Ok(_) => panic!("committed corruption must fail startup"),
            Err(error) => error,
        };
        let message = format!("{error}");
        assert!(message.contains("byte offset 64"));
        assert!(message.contains("record sequence 1"));
        assert!(message.contains("checksum mismatch"));
    }

    #[test]
    fn unsupported_frame_versions_fail_without_rewriting_the_log() {
        let directory = TemporaryStore::new("frame-version");
        {
            let mut store = LogStore::open(&directory.0).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"role".to_vec())])
                .unwrap();
        }
        let log_path = directory.0.join(LOG_NAME);
        let mut bytes = std::fs::read(&log_path).unwrap();
        bytes[LOG_HEADER_LEN as usize + 4] = 2;
        std::fs::write(&log_path, &bytes).unwrap();

        let error = match LogStore::open(&directory.0) {
            Ok(_) => panic!("unsupported committed frame version must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("unsupported frame version"));
        assert_eq!(std::fs::read(&log_path).unwrap(), bytes);
    }

    #[test]
    fn malformed_payload_lengths_are_bounded_before_allocation() {
        let directory = TemporaryStore::new("frame-length");
        {
            let mut store = LogStore::open(&directory.0).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, vec![0xff; 32])])
                .unwrap();
        }
        let log_path = directory.0.join(LOG_NAME);
        let mut bytes = std::fs::read(&log_path).unwrap();
        bytes[LOG_HEADER_LEN as usize + 16] = 0xff;
        std::fs::write(&log_path, &bytes).unwrap();
        let error = match LogStore::open(&directory.0) {
            Ok(_) => panic!("malformed committed length must fail"),
            Err(error) => error,
        };
        let message = error.to_string();
        assert!(message.contains("byte offset 64"), "{message}");
        assert!(
            message.contains("ULEB128") || message.contains("frame length"),
            "{message}"
        );
    }

    #[test]
    fn every_uncommitted_tail_prefix_is_discarded_but_the_commit_is_preserved() {
        let source = TemporaryStore::new("tail-source");
        let (base_length, base_manifest) = {
            let mut store = LogStore::open(&source.0).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"first".to_vec())])
                .unwrap();
            let length = store.committed_length();
            let manifest = std::fs::read(source.0.join(MANIFEST_NAME)).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"second".to_vec())])
                .unwrap();
            (length, manifest)
        };
        let complete_log = std::fs::read(source.0.join(LOG_NAME)).unwrap();
        let committed_prefix = &complete_log[..base_length as usize];
        let uncommitted_batch = &complete_log[base_length as usize..];

        for tail_length in 0..=uncommitted_batch.len() {
            let candidate = TemporaryStore::new(&format!("tail-cut-{tail_length}"));
            std::fs::create_dir_all(&candidate.0).unwrap();
            std::fs::write(candidate.0.join(LOCK_NAME), []).unwrap();
            std::fs::write(candidate.0.join(MANIFEST_NAME), &base_manifest).unwrap();
            let mut log = committed_prefix.to_vec();
            log.extend_from_slice(&uncommitted_batch[..tail_length]);
            std::fs::write(candidate.0.join(LOG_NAME), log).unwrap();

            let reopened = LogStore::open(&candidate.0).unwrap();
            assert_eq!(reopened.batch_count(), 1);
            assert_eq!(
                std::fs::metadata(candidate.0.join(LOG_NAME)).unwrap().len(),
                base_length
            );
        }
    }

    #[test]
    fn every_truncation_inside_committed_tail_is_fatal_and_never_rewritten() {
        let source = TemporaryStore::new("committed-tail-source");
        let first_length = {
            let mut store = LogStore::open(&source.0).unwrap();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"first".to_vec())])
                .unwrap();
            let first_length = store.committed_length();
            store
                .append_batch(&[PendingRecord::new(RECORD_ROLE, b"second".to_vec())])
                .unwrap();
            first_length
        };
        let manifest = std::fs::read(source.0.join(MANIFEST_NAME)).unwrap();
        let log = std::fs::read(source.0.join(LOG_NAME)).unwrap();

        for cut in first_length as usize..log.len() {
            let candidate = TemporaryStore::new(&format!("committed-tail-cut-{cut}"));
            std::fs::create_dir_all(&candidate.0).unwrap();
            std::fs::write(candidate.0.join(LOCK_NAME), []).unwrap();
            std::fs::write(candidate.0.join(MANIFEST_NAME), &manifest).unwrap();
            std::fs::write(candidate.0.join(LOG_NAME), &log[..cut]).unwrap();

            let error = match LogStore::open(&candidate.0) {
                Ok(_) => panic!("committed truncation at byte {cut} unexpectedly opened"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains("committed length")
                    || error.to_string().contains("truncated"),
                "cut {cut}: {error}"
            );
            assert_eq!(
                std::fs::metadata(candidate.0.join(LOG_NAME)).unwrap().len(),
                cut as u64,
                "failed open must not rewrite committed corruption"
            );
        }
    }

    #[test]
    fn an_empty_committed_log_file_is_corruption_not_a_new_store() {
        let directory = TemporaryStore::new("empty-log");
        {
            let _store = LogStore::open(&directory.0).unwrap();
        }
        let log_path = directory.0.join(LOG_NAME);
        std::fs::write(&log_path, []).unwrap();
        let error = match LogStore::open(&directory.0) {
            Ok(_) => panic!("empty committed log must fail"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("committed length")
                || error.to_string().contains("failed to fill whole buffer")
        );
        assert_eq!(std::fs::metadata(log_path).unwrap().len(), 0);
    }
}
