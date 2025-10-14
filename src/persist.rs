//! Persistence layer: SQLite schema management and (re)construction logic.
//!
//! The `Persistor` encapsulates prepared statements for both writing and
//! restoring constructs. It owns no *logical* state besides a small cache of
//! seen data type identifiers (`seen_data_types`) to avoid redundant catalog
//! inserts.
//!
//! # Schema Overview
//! * `Thing(Thing_Identity)` – canonical identity table.
//! * `Role(Role_Identity, Role, Reserved)` – role metadata (identity FK to Thing).
//! * `DataType(DataType_Identity, DataType)` – catalog of logical value/time types.
//! * `Posit(Posit_Identity, AppearanceSet, AppearingValue, ValueType_Identity, AppearanceTime)` – stored propositions.
//!
//! Appearance sets are serialized as a pipe separated list of `thing,role` pairs
//! in natural order: `thing_id,role_id|thing_id,role_id|...`.
//!
//! # Lifecyle
//! * During startup `Database::new` calls restoration helpers which replay
//!   persisted rows into in-memory keepers.
//! * New constructs invoke `persist_*` methods which perform idempotent writes
//!   (checking for existing rows first) and return whether the row already existed.
//!
//! # Adding New Data Types
//! After implementing [`crate::datatype::DataType`] for a type, extend the
//! match section in `restore_posits` so the value can be reconstructed.
//!
//! # Error Handling
//! Current implementation panics on unexpected SQLite errors. A future revision
//! could propagate a domain error type instead.
// used for persistence
use blake3;
use rusqlite::{Connection, Error, params};
use crate::error::{DatabaseError, Result};

/// 64 zero hex string representing the genesis (no previous) hash in the integrity chain.
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

// our own stuff
use crate::construct::{Appearance, AppearanceSet, Database, Posit, Role, Thing};
use crate::datatype::{Certainty, DataType, Decimal, JSON, Time};

// ------------- Persistence -------------
pub struct Persistor {
    /// File path of the SQLite database, if file-backed. If None, using in-memory (runtime writes/restores are no-ops).
    db_path: Option<String>,
    /// Cache of data type identifiers already inserted into `DataType`.
    seen_data_types: Vec<u8>,
}
impl Persistor {
    /// Create a file-backed persistor given a filesystem path; opens a connection to initialize schema and records the path for later calls.
    pub fn new_from_file(path: &str) -> Result<Persistor> {
        let conn = Connection::open(path).map_err(DatabaseError::from)?;
        let _ = conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;");
        conn.execute_batch(
            "
            create table if not exists Thing (
                Thing_Identity integer not null, 
                constraint unique_and_referenceable_Thing_Identity primary key (
                    Thing_Identity
                )
            ) STRICT;
            create table if not exists Role (
                Role_Identity integer not null,
                Role text not null,
                Reserved integer not null,
                constraint Role_is_Thing foreign key (
                    Role_Identity
                ) references Thing(Thing_Identity),
                constraint referenceable_Role_Identity primary key (
                    Role_Identity
                ),
                constraint unique_Role unique (
                    Role
                )
            ) STRICT;
            create table if not exists DataType (
                DataType_Identity integer not null,
                DataType text not null,
                constraint referenceable_DataType_Identity primary key (
                    DataType_Identity
                ),
                constraint unique_DataType unique (
                    DataType
                )
            ) STRICT;
            create table if not exists Posit (
                Posit_Identity integer not null,
                AppearanceSet text not null,
                AppearingValue any null, 
                ValueType_Identity integer not null, 
                AppearanceTime any null,
                constraint Posit_is_Thing foreign key (
                    Posit_Identity
                ) references Thing(Thing_Identity),
                constraint ValueType_is_DataType foreign key (
                    ValueType_Identity
                ) references DataType(DataType_Identity),
                constraint referenceable_Posit_Identity primary key (
                    Posit_Identity
                ),
                constraint unique_Posit unique (
                    AppearanceSet,
                    AppearingValue,
                    AppearanceTime
                )
            ) STRICT;
            create table if not exists PositHash (
                Posit_Identity integer not null,
                PrevHash text not null,
                Hash text not null,
                constraint PositHash_is_Posit foreign key (
                    Posit_Identity
                ) references Posit(Posit_Identity),
                constraint referenceable_PositHash_Identity primary key (
                    Posit_Identity
                )
            ) STRICT;
            create table if not exists LedgerHead (
                Name text not null,
                HeadHash text not null,
                Count integer not null,
                constraint referenceable_LedgerHead_Name primary key (
                    Name
                )
            ) STRICT;
            ",
    )
    .map_err(DatabaseError::from)?;
        Ok(Persistor {
            db_path: Some(path.to_string()),
            seen_data_types: Vec::new(),
        })
    }

    /// Create a persistor that performs no persistence at runtime (no file I/O).
    pub fn new_no_persistence() -> Persistor {
        Persistor {
            db_path: None,
            seen_data_types: Vec::new(),
        }
    }

    /// Helper: run an operation with a Connection. For file-backed databases, opens a fresh
    /// connection per call to avoid sharing Connection across threads. For in-memory, falls back
    /// to the primary connection created by the caller.
    fn with_conn<T>(&self, mut op: impl FnMut(&Connection) -> Result<T>) -> Option<Result<T>> {
        if let Some(ref path) = self.db_path {
            let conn = match Connection::open(path) { Ok(c) => c, Err(e) => return Some(Err(DatabaseError::from(e))) };
            // Busy timeout helps under concurrent writes
            let _ = conn.busy_timeout(std::time::Duration::from_millis(5000));
            Some(op(&conn))
        } else {
            // In-memory mode: no shared path to reopen; treat persistence as a no-op at runtime
            None
        }
    }
    /// Persist a thing identity if not already present.
    /// Returns true if the record already existed.
    pub fn persist_thing(&mut self, thing: &Thing) -> Result<bool> {
        let mut existing = false;
        if let Some(r) = self.with_conn(|conn| {
            let mut stmt = conn.prepare("select Thing_Identity from Thing where Thing_Identity = ?")?;
            match stmt.query_row::<usize, _, _>(params![&thing], |r| r.get(0)) {
                Ok(_) => { existing = true; }
                Err(Error::QueryReturnedNoRows) => {
                    conn.prepare("insert into Thing (Thing_Identity) values (?)")?
                        .execute(params![&thing])?;
                }
                Err(e) => { return Err(DatabaseError::Persistence(format!("Thing check failed: {e}"))); }
            }
            Ok(())
        }) { r?; }
        Ok(existing)
    }
    /// Persist a role row by unique role name. Returns true if already present.
    pub fn persist_role(&mut self, role: &Role) -> Result<bool> {
        let mut existing = false;
        if let Some(r) = self.with_conn(|conn| {
            let mut stmt = conn.prepare("select Role_Identity from Role where Role = ?")?;
            match stmt.query_row::<usize, _, _>(params![&role.name()], |r| r.get(0)) {
                Ok(_) => { existing = true; }
                Err(Error::QueryReturnedNoRows) => {
                    conn.prepare("insert into Role (Role_Identity, Role, Reserved) values (?, ?, ?)")?
                        .execute(params![&role.role(), &role.name(), &role.reserved()])?;
                }
                Err(e) => { return Err(DatabaseError::Persistence(format!("Role check failed: {e}"))); }
            }
            Ok(())
        }) { r?; }
        Ok(existing)
    }
    /// Persist a posit (idempotent). If unseen, ensures associated value & time
    /// data types are catalogued. Returns true if the posit already existed.
    pub fn persist_posit<V: 'static + DataType>(&mut self, posit: &Posit<V>) -> Result<bool> {
        let mut appearances = Vec::new();
        let appearance_set = posit.appearance_set();
        for appearance in appearance_set.appearances().iter() {
            appearances
                .push(appearance.thing().to_string() + "," + &appearance.role().role().to_string());
        }
        let apperance_set_as_text = appearances.join("|");
        let mut existing = false;
        // Existence check
        if let Some(r) = self.with_conn(|conn| {
            let mut stmt = conn.prepare("select Posit_Identity from Posit where AppearanceSet = ? and AppearingValue = ? and AppearanceTime = ?")?;
            match stmt.query_row::<usize, _, _>(params![&apperance_set_as_text, &posit.value(), &posit.time()], |r| r.get(0)) {
                Ok(_) => { existing = true; }
                Err(Error::QueryReturnedNoRows) => { /* insert below */ }
                Err(e) => { return Err(DatabaseError::Persistence(format!("Posit check failed: {e}"))); }
            }
            Ok(())
        }) { r?; }
        if !existing {
            // Update cache outside of connection borrow
            let need_value_dt = !self.seen_data_types.contains(&posit.value().identifier());
            let need_time_dt = !self.seen_data_types.contains(&posit.time().identifier());
            if need_value_dt {
                self.seen_data_types.push(posit.value().identifier());
            }
            if need_time_dt {
                self.seen_data_types.push(posit.time().identifier());
            }
            // Perform inserts
            if let Some(r) = self.with_conn(|conn| {
                if need_value_dt {
                    conn.prepare("insert or ignore into DataType (DataType_Identity, DataType) values (?, ?)")?
                        .execute(params![&posit.value().identifier(), &posit.value().data_type()])?;
                }
                if need_time_dt {
                    conn.prepare("insert or ignore into DataType (DataType_Identity, DataType) values (?, ?)")?
                        .execute(params![&posit.time().identifier(), &posit.time().data_type()])?;
                }
                conn.prepare("insert into Posit (Posit_Identity, AppearanceSet, AppearingValue, ValueType_Identity, AppearanceTime) values (?, ?, ?, ?, ?)")?
                    .execute(params![&posit.posit(), &apperance_set_as_text, &posit.value(), &posit.value().identifier(), &posit.time()])?;
                let prev_hash: String = {
                    let mut stmt = conn.prepare("select Hash from PositHash order by Posit_Identity desc limit 1")?;
                    let mut rows = stmt.query([])?;
                    if let Some(row) = rows.next()? { row.get::<_, String>(0)? } else { GENESIS_HASH.to_string() }
                };
                let mut stmt = conn.prepare("select AppearanceSet, cast(AppearingValue as text), ValueType_Identity, AppearanceTime from Posit where Posit_Identity = ?")?;
                let mut rows = stmt.query(params![&posit.posit()])?;
                if let Some(r) = rows.next()? {
                    let aset_text: String = r.get(0)?;
                    let value_text: String = r.get(1)?;
                    let vtid: i64 = r.get(2)?;
                    let atime_text: String = r.get(3)?;
                    let input = format!("{}|{}|{}|{}|{}|prev={}", &posit.posit(), aset_text, vtid, value_text, atime_text, prev_hash);
                    let hash_hex = blake3::hash(input.as_bytes()).to_hex().to_string();
                    conn.prepare("insert into PositHash (Posit_Identity, PrevHash, Hash) values (?, ?, ?)")?
                        .execute(params![&posit.posit(), &prev_hash, &hash_hex])?;
                    let count: i64 = conn.prepare("select count(1) from PositHash")?
                        .query_row([], |r| r.get(0))?;
                    conn.prepare("insert into LedgerHead (Name, HeadHash, Count) values ('PositLedger', ?, ?) on conflict(Name) do update set HeadHash=excluded.HeadHash, Count=excluded.Count")?
                        .execute(params![&hash_hex, &count])?;
                }
                Ok(())
            }) { r?; }
        }
        Ok(existing)
    }
    /// Rehydrate all thing identities into the in-memory generator.
    pub fn restore_things(&mut self, db: &Database) -> Result<()> {
        if let Some(ref path) = self.db_path {
            let conn = Connection::open(path).map_err(DatabaseError::from)?;
            let mut stmt = conn.prepare("select Thing_Identity from Thing").map_err(DatabaseError::from)?;
            let rows = stmt.query_map([], |row| row.get::<_, Thing>(0)).map_err(DatabaseError::from)?;
            for thing in rows {
                match thing {
                    Ok(t) => { db.thing_generator().lock().unwrap().retain(t); }
                    Err(e) => return Err(DatabaseError::DataCorruption { message: format!("Bad Thing row: {e}") })
                }
            }
        }
        Ok(())
    }
    /// Rehydrate all roles into the in-memory keeper.
    pub fn restore_roles(&mut self, db: &Database) -> Result<()> {
        if let Some(ref path) = self.db_path {
            let conn = Connection::open(path).map_err(DatabaseError::from)?;
            let mut stmt = conn.prepare("select Role_Identity, Role, Reserved from Role").map_err(DatabaseError::from)?;
            let rows = stmt.query_map([], |row| {
                let role_id: Thing = row.get(0)?;
                let name: String = row.get(1)?;
                let reserved: i64 = row.get(2)?;
                Ok(Role::new(role_id, name, reserved != 0))
            }).map_err(DatabaseError::from)?;
            for r in rows {
                match r {
                    Ok(role) => { db.keep_role(role); }
                    Err(e) => return Err(DatabaseError::DataCorruption { message: format!("Bad Role row: {e}") })
                }
            }
        }
        Ok(())
    }
    /// Rehydrate all posits (including nested appearance sets) into memory.
    ///
    /// Appearance sets are parsed from their serialized pipe-separated form.
    pub fn restore_posits(&mut self, db: &Database) -> Result<()> {
        if self.db_path.is_none() { return Ok(()); }
        let conn = Connection::open(self.db_path.as_ref().unwrap()).map_err(DatabaseError::from)?;
        let mut stmt = conn.prepare("select p.Posit_Identity, p.AppearanceSet, p.AppearingValue, v.DataType as ValueType, p.AppearanceTime from Posit p join DataType v on v.DataType_Identity = p.ValueType_Identity").map_err(DatabaseError::from)?;
        let mut rows = stmt.query([]).map_err(DatabaseError::from)?;
        while let Some(row) = rows.next().map_err(DatabaseError::from)? {
            let value_type: String = row.get(3).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad value type: {e}") })?;
            let thing: Thing = row.get(0).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad posit id: {e}") })?;
            let appearances: String = row.get(1).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad appearance set: {e}") })?;
            let mut appearance_vec = Vec::new();
            for appearance_text in appearances.split('|') {
                let Some((thing_txt, role_txt)) = appearance_text.split_once(',') else { return Err(DatabaseError::DataCorruption { message: format!("Malformed appearance fragment: '{appearance_text}'") }); };
                let thing_id: Thing = thing_txt.parse().map_err(|e| DatabaseError::DataCorruption { message: format!("Bad appearance thing id '{thing_txt}': {e}") })?;
                let role_id: Thing = role_txt.parse().map_err(|e| DatabaseError::DataCorruption { message: format!("Bad role id '{role_txt}': {e}") })?;
                let role_arc = db.role_keeper().lock().unwrap().lookup(&role_id);
                let appearance = Appearance::new(thing_id, role_arc);
                let (kept_appearance, _) = db.keep_appearance(appearance);
                appearance_vec.push(kept_appearance);
            }
            let aset_res = AppearanceSet::new(appearance_vec).ok_or_else(|| DatabaseError::DataCorruption { message: "Duplicate role in appearance set during restore".into() })?;
            let (kept_appearance_set, _) = db.keep_appearance_set(aset_res);
            let time = Time::convert(&row.get_ref(4).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad time ref: {e}") })?);
            match value_type.as_str() {
                String::DATA_TYPE => {
                    let v = <String as DataType>::convert(&row.get_ref(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad string value: {e}") })?);
                    db.keep_posit(Posit::new(thing, kept_appearance_set, v, time.clone()));
                }
                i64::DATA_TYPE => {
                    let v = <i64 as DataType>::convert(&row.get_ref(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad i64 value: {e}") })?);
                    db.keep_posit(Posit::new(thing, kept_appearance_set, v, time.clone()));
                }
                Decimal::DATA_TYPE => {
                    let v = <Decimal as DataType>::convert(&row.get_ref(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad decimal value: {e}") })?);
                    db.keep_posit(Posit::new(thing, kept_appearance_set, v, time.clone()));
                }
                Time::DATA_TYPE => {
                    let v = <Time as DataType>::convert(&row.get_ref(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad time value: {e}") })?);
                    db.keep_posit(Posit::new(thing, kept_appearance_set, v, time.clone()));
                }
                JSON::DATA_TYPE => {
                    let v = <JSON as DataType>::convert(&row.get_ref(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad json value: {e}") })?);
                    db.keep_posit(Posit::new(thing, kept_appearance_set, v, time.clone()));
                }
                Certainty::DATA_TYPE => {
                    let v = <Certainty as DataType>::convert(&row.get_ref(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad certainty value: {e}") })?);
                    db.keep_posit(Posit::new(thing, kept_appearance_set, v, time.clone()));
                }
                _ => { /* unknown type silently skipped */ }
            }
        }
        Ok(())
    }

    /// Verify the integrity chain of posits (no auto backfill / rebuild).
    /// Emits warnings if ledger missing or hashes mismatch.
    pub fn verify_integrity(&mut self) -> Result<()> {
    if self.db_path.is_none() { return Ok(()); }
    let conn = Connection::open(self.db_path.as_ref().unwrap()).map_err(DatabaseError::from)?;
    let posit_count: i64 = conn.prepare("select count(1) from Posit").map_err(DatabaseError::from)?.query_row([], |r| r.get(0)).map_err(DatabaseError::from)?;
        if posit_count == 0 { return Ok(()); }
    let hash_count: i64 = conn.prepare("select count(1) from PositHash").map_err(DatabaseError::from)?.query_row([], |r| r.get(0)).map_err(DatabaseError::from)?;
        if hash_count == 0 {
            return Err(DatabaseError::Invariant(format!("Integrity ledger missing ({} posits present)", posit_count)));
        }
    let mut stmt = conn.prepare("select p.Posit_Identity, p.AppearanceSet, cast(p.AppearingValue as text), p.ValueType_Identity, p.AppearanceTime, h.Hash from Posit p join PositHash h on h.Posit_Identity = p.Posit_Identity order by p.Posit_Identity asc").map_err(DatabaseError::from)?;
    let mut rows = stmt.query([]).map_err(DatabaseError::from)?;
        let mut prev = GENESIS_HASH.to_string();
        let mut mismatches = 0usize;
        let mut first_bad: Option<i64> = None;
        let mut last_hash = prev.clone();
        while let Some(row) = rows.next().map_err(DatabaseError::from)? {
            let thing: i64 = row.get(0).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad posit id: {e}") })?;
            let aset: String = row.get(1).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad AppearanceSet: {e}") })?;
            let aval: String = row.get(2).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad AppearingValue: {e}") })?;
            let vtid: i64 = row.get(3).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad ValueType id: {e}") })?;
            let atime: String = row.get(4).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad AppearanceTime: {e}") })?;
            let stored_hash: String = row.get(5).map_err(|e| DatabaseError::DataCorruption { message: format!("Bad stored hash: {e}") })?;
            let input = format!("{}|{}|{}|{}|{}|prev={}", thing, aset, vtid, aval, atime, prev);
            let calc = blake3::hash(input.as_bytes()).to_hex().to_string();
            if calc != stored_hash {
                mismatches += 1;
                if first_bad.is_none() { first_bad = Some(thing); }
            }
            prev = stored_hash.clone();
            last_hash = stored_hash;
        }
        conn.prepare("insert into LedgerHead (Name, HeadHash, Count) values ('PositLedger', ?, ?) on conflict(Name) do update set HeadHash=excluded.HeadHash, Count=excluded.Count")
            .map_err(DatabaseError::from)?
            .execute(params![&last_hash, &posit_count])
            .map_err(DatabaseError::from)?;
        if mismatches > 0 {
            return Err(DatabaseError::Invariant(format!("Integrity violation: {mismatches} mismatched hashes (first at {:?})", first_bad)));
        }
        Ok(())
    }

    /// Returns the current integrity ledger head hash and count, when persistence is enabled and the ledger exists.
    pub fn current_superhash(&self) -> Option<(String, i64)> {
        if self.db_path.is_none() {
            return None;
        }
        let conn = Connection::open(self.db_path.as_ref().unwrap()).unwrap();
        let mut stmt = conn
            .prepare("select HeadHash, Count from LedgerHead where Name = 'PositLedger'")
            .unwrap();
        let mut rows = stmt.query([]).unwrap();
        if let Some(row) = rows.next().unwrap() {
            let head: String = row.get_unwrap(0);
            let count: i64 = row.get_unwrap(1);
            Some((head, count))
        } else {
            None
        }
    }
}
