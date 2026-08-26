//! Versioned append-only storage primitives for the beta store format.
//!
//! This module is intentionally private until the high-level database API has
//! fully replaced the SQLite prototype. Its byte contract is specified in
//! `STORAGE.md`.

#![allow(dead_code)] // Removed when Database switches from the SQLite importer to this backend.

use crate::error::{DatabaseError, Result};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const MANIFEST_NAME: &str = "manifest.pmf";
const MANIFEST_NEXT_NAME: &str = "manifest.next";
const LOCK_NAME: &str = "store.lock";
const LOG_NAME: &str = "log-0000000000000001.ptl";
const MANIFEST_MAGIC: &[u8; 8] = b"POSITPMF";
const LOG_MAGIC: &[u8; 8] = b"POSITLOG";
const FRAME_MAGIC: &[u8; 4] = b"PTR1";
const FORMAT_MAJOR: u16 = 1;
const FORMAT_MINOR: u16 = 0;
const MANIFEST_HEADER_LEN: u32 = 48;
const LOG_HEADER_LEN: u32 = 64;
const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

pub(crate) const RECORD_ROLE: u8 = 1;
pub(crate) const RECORD_CODEC: u8 = 2;
pub(crate) const RECORD_POSIT: u8 = 3;
const RECORD_COMMIT: u8 = 255;

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
    fn replay(&self) -> &[Vec<StoredRecord>];
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

    fn replay(&self) -> &[Vec<StoredRecord>] {
        &self.batches
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
    batches: Vec<Vec<StoredRecord>>,
    next_sequence: u64,
    next_batch: u64,
}

impl LogStore {
    pub(crate) fn open(path: impl AsRef<Path>) -> Result<Self> {
        let directory = path.as_ref().to_path_buf();
        std::fs::create_dir_all(&directory).map_err(DatabaseError::from_io)?;
        let lock_path = directory.join(LOCK_NAME);
        let lock = OpenOptions::new()
            .create(true)
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
        let (batches, next_sequence, next_batch) =
            replay_committed(&mut log, manifest.committed_length)?;

        Ok(Self {
            directory,
            _lock: lock,
            log,
            manifest,
            batches,
            next_sequence,
            next_batch,
        })
    }

    pub(crate) fn store_uuid(&self) -> [u8; 16] {
        self.manifest.store_uuid
    }

    pub(crate) fn committed_length(&self) -> u64 {
        self.manifest.committed_length
    }
}

impl StoreBackend for LogStore {
    fn append_batch(&mut self, records: &[PendingRecord]) -> Result<Vec<StoredRecord>> {
        validate_pending_records(records)?;
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

        self.log
            .seek(SeekFrom::Start(old_length))
            .and_then(|_| self.log.write_all(&framed))
            .and_then(|_| self.log.sync_all())
            .map_err(DatabaseError::from_io)?;
        let new_length = old_length
            .checked_add(framed.len() as u64)
            .ok_or_else(|| DatabaseError::Invariant("log length exhausted".to_string()))?;
        let new_manifest = Manifest {
            store_uuid: self.manifest.store_uuid,
            committed_length: new_length,
        };
        write_manifest(&self.directory, &new_manifest)?;

        self.manifest = new_manifest;
        self.next_sequence = sequence
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("record sequence exhausted".to_string()))?;
        self.next_batch = self
            .next_batch
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("batch sequence exhausted".to_string()))?;
        self.batches.push(stored.clone());
        Ok(stored)
    }

    fn replay(&self) -> &[Vec<StoredRecord>] {
        &self.batches
    }

    fn flush(&mut self) -> Result<()> {
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
    getrandom::getrandom(&mut store_uuid).map_err(|error| {
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

fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(DatabaseError::from_io)?;
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

fn replay_committed(
    log: &mut File,
    committed_length: u64,
) -> Result<(Vec<Vec<StoredRecord>>, u64, u64)> {
    let mut offset = u64::from(LOG_HEADER_LEN);
    let mut expected_sequence = 1u64;
    let mut expected_batch = 1u64;
    let mut staged = Vec::new();
    let mut batches = Vec::new();
    while offset < committed_length {
        let (record, next_offset) = read_frame(log, offset, committed_length)?;
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
                batches.push(std::mem::take(&mut staged));
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
    Ok((batches, expected_sequence, expected_batch))
}

fn read_frame(log: &mut File, offset: u64, limit: u64) -> Result<(StoredRecord, u64)> {
    log.seek(SeekFrom::Start(offset))
        .map_err(DatabaseError::from_io)?;
    let mut prefix = [0u8; 16];
    log.read_exact(&mut prefix)
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
    let payload_length = read_uleb128(log, &mut framed, offset, sequence)?;
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
    log.read_exact(&mut payload)
        .map_err(|error| frame_io_error(offset, Some(sequence), error))?;
    framed.extend_from_slice(&payload);
    let mut checksum_bytes = [0u8; 4];
    log.read_exact(&mut checksum_bytes)
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
    encode_uleb128(value.len() as u64, target);
    target.extend_from_slice(value.as_bytes());
}

fn decode_text(bytes: &[u8], cursor: &mut usize) -> Result<String> {
    let length = usize::try_from(decode_uleb128(bytes, cursor)?).map_err(|_| {
        DatabaseError::DataCorruption {
            message: "text length exceeds addressable memory".to_string(),
        }
    })?;
    let end = cursor
        .checked_add(length)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| DatabaseError::DataCorruption {
            message: "truncated text field".to_string(),
        })?;
    let value = std::str::from_utf8(&bytes[*cursor..end])
        .map_err(|error| DatabaseError::DataCorruption {
            message: format!("invalid UTF-8 text: {error}"),
        })?
        .to_string();
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
    source: &mut File,
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

impl DatabaseError {
    fn from_io(error: std::io::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn log_batches_survive_reopen_with_the_same_uuid() {
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
            store.flush().unwrap();
            (store.store_uuid(), store.committed_length())
        };

        let reopened = LogStore::open(&directory.0).unwrap();
        assert_eq!(reopened.store_uuid(), uuid);
        assert_eq!(reopened.committed_length(), committed_length);
        assert_eq!(reopened.replay().len(), 1);
        assert_eq!(reopened.replay()[0].len(), 3);
        assert_eq!(reopened.replay()[0][2].payload, b"posit");
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
        assert_eq!(reopened.replay().len(), 1);
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
}
