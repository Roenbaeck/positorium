use positorium::construct::{Database, PersistenceMode};

#[cfg(feature = "persistence")]
fn temporary_database_path(label: &str) -> String {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "positorium-{label}-{}-{nonce}.db",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

#[cfg(feature = "persistence")]
fn remove_database_files(path: &str) {
    for suffix in ["", "-shm", "-wal"] {
        let _ = std::fs::remove_file(format!("{path}{suffix}"));
    }
}

#[test]
fn in_memory_mode_allows_basic_operations() {
    let db = Database::new(PersistenceMode::InMemory).expect("db");
    let (role, existed) = db.create_role("person".to_string(), false);
    assert!(!existed);
    let thing = db.create_thing();
    let (appearance, _) = db.create_appearance(*thing, role);
    let (_aset, _) = db.create_appearance_set(vec![appearance]).unwrap();
    // No ledger head should exist (no persistence)
    #[cfg(feature = "persistence")]
    assert!(
        db.persistor
            .lock()
            .unwrap()
            .current_superhash()
            .unwrap()
            .is_none()
    );
}

#[test]
#[cfg(feature = "persistence")]
fn file_mode_persists_and_has_ledger() {
    let path = temporary_database_path("ledger");
    let db = Database::new(PersistenceMode::File(path.clone())).expect("db");
    let (role, _) = db.create_role("audit".to_string(), false);
    let thing = db.create_thing();
    let (appearance, _) = db.create_appearance(*thing, role);
    let (aset, _) = db.create_appearance_set(vec![appearance]).unwrap();
    // Insert a posit to trigger ledger append
    let time = positorium::datatype::Time::new();
    let _posit = db.create_posit(aset, "ok".to_string(), time);
    let head = db.persistor.lock().unwrap().current_superhash().unwrap();
    assert!(
        head.is_some(),
        "expected ledger head after posit insertion in file-backed mode"
    );
    // Clean up
    drop(db);
    remove_database_files(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn restores_all_builtin_date_value_types() {
    use chrono::{NaiveDate, NaiveDateTime};

    let path = temporary_database_path("chrono-values");
    let date = NaiveDate::from_ymd_opt(2026, 8, 26).unwrap();
    let datetime = date.and_hms_opt(12, 34, 56).unwrap();
    let (date_id, datetime_id) = {
        let db = Database::new(PersistenceMode::File(path.clone())).expect("create database");
        let (role, _) = db.create_role("observed".to_string(), false);
        let thing = db.create_thing();
        let (appearance, _) = db.create_appearance(*thing, role);
        let (appearance_set, _) = db.create_appearance_set(vec![appearance]).unwrap();
        let time = positorium::datatype::Time::new();
        let date_posit = db.create_posit(appearance_set.clone(), date, time.clone());
        let datetime_posit = db.create_posit(appearance_set, datetime, time);
        (date_posit.posit(), datetime_posit.posit())
    };

    let restored = Database::new(PersistenceMode::File(path.clone())).expect("restore database");
    let keeper = restored.posit_keeper();
    let mut keeper = keeper.lock().unwrap();
    assert_eq!(*keeper.posit::<NaiveDate>(date_id).unwrap().value(), date);
    assert_eq!(
        *keeper.posit::<NaiveDateTime>(datetime_id).unwrap().value(),
        datetime
    );
    drop(keeper);
    drop(restored);
    remove_database_files(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn unknown_persisted_value_type_fails_restore() {
    let path = temporary_database_path("unknown-type");
    {
        let db = Database::new(PersistenceMode::File(path.clone())).expect("create database");
        let (role, _) = db.create_role("label".to_string(), false);
        let thing = db.create_thing();
        let (appearance, _) = db.create_appearance(*thing, role);
        let (appearance_set, _) = db.create_appearance_set(vec![appearance]).unwrap();
        db.create_posit(
            appearance_set,
            "value".to_string(),
            positorium::datatype::Time::new(),
        );
    }
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "update DataType set DataType = 'Mystery' where DataType_Identity = 2",
            [],
        )
        .unwrap();

    let error = match Database::new(PersistenceMode::File(path.clone())) {
        Ok(_) => panic!("unknown persisted types must not be skipped"),
        Err(error) => error,
    };
    assert!(format!("{error}").contains("Unknown persisted value type 'Mystery'"));
    remove_database_files(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn integrity_verification_does_not_rewrite_a_bad_head() {
    let path = temporary_database_path("bad-head");
    let db = Database::new(PersistenceMode::File(path.clone())).expect("create database");
    let (role, _) = db.create_role("label".to_string(), false);
    let thing = db.create_thing();
    let (appearance, _) = db.create_appearance(*thing, role);
    let (appearance_set, _) = db.create_appearance_set(vec![appearance]).unwrap();
    db.create_posit(
        appearance_set,
        "value".to_string(),
        positorium::datatype::Time::new(),
    );
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute(
            "update LedgerHead set HeadHash = 'bad' where Name = 'PositLedger'",
            [],
        )
        .unwrap();

    let error = db.persistor.lock().unwrap().verify_integrity().unwrap_err();
    assert!(format!("{error}").contains("ledger head mismatch"));
    let stored: String = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row(
            "select HeadHash from LedgerHead where Name = 'PositLedger'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(stored, "bad");
    drop(db);
    remove_database_files(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn incompatible_builtin_role_metadata_fails_open() {
    let path = temporary_database_path("builtin-role");
    {
        Database::new(PersistenceMode::File(path.clone())).expect("create database");
    }
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("update Role set Reserved = 0 where Role = 'posit'", [])
        .unwrap();

    let error = match Database::new(PersistenceMode::File(path.clone())) {
        Ok(_) => panic!("incompatible built-in metadata must fail startup"),
        Err(error) => error,
    };
    assert!(format!("{error}").contains("built-in role 'posit'"));
    remove_database_files(&path);
}
