use positorium::construct::{Database, PersistenceMode};

#[cfg(feature = "persistence")]
fn temporary_store_path(label: &str) -> String {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "positorium-{label}-{}-{nonce}.store",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

#[cfg(feature = "persistence")]
fn remove_store(path: &str) {
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn in_memory_mode_allows_basic_operations() {
    let db = Database::new(PersistenceMode::InMemory).expect("db");
    positorium::Engine::new(&db)
        .execute("add role person; add posit [{(+person, person)}, \"Alice\", @NOW];")
        .expect("execute in memory");
    assert!(db.contains_role("person").unwrap());
}

#[test]
#[cfg(feature = "persistence")]
fn append_only_store_replays_roles_posits_and_indexes() {
    use positorium::traqula::Engine;

    let path = temporary_store_path("replay");
    {
        let db = Database::new(PersistenceMode::File(path.clone())).expect("create store");
        let engine = Engine::new(&db);
        engine
            .execute_collect(
                "add role amount; add posit [{(+item, amount)}, +001.00, '2026-08-26'];",
            )
            .expect("durable commands");
        assert_eq!(db.role_count().unwrap(), 6);
    }

    let restored = Database::new(PersistenceMode::File(path.clone())).expect("replay store");
    let result = Engine::new(&restored)
        .execute_collect("search [{(*, amount), ...}, ?value, ?time] return ?value, ?time;")
        .expect("query restored indexes");
    assert_eq!(
        result.rows,
        vec![vec!["+001.00".to_string(), "2026-08-26".to_string()]]
    );
    drop(restored);
    remove_store(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn every_literal_family_survives_append_only_restart_losslessly() {
    use positorium::traqula::Engine;

    let path = temporary_store_path("literal-families");
    {
        let db = Database::new(PersistenceMode::File(path.clone())).expect("create store");
        Engine::new(&db)
            .execute_collect(
                r#"
                add role value;
                add posit [{(+s, value)}, "\u0041", @NOW];
                add posit [{(+i, value)}, +001, @NOW];
                add posit [{(+canonical_i, value)}, 10, @NOW];
                add posit [{(+minimum_i, value)}, -9223372036854775808, @NOW];
                add posit [{(+maximum_i, value)}, 9223372036854775807, @NOW];
                add posit [{(+d, value)}, 01.00, @NOW];
                add posit [{(+c, value)}, 075%, @NOW];
                add posit [{(+canonical_c, value)}, 75%, @NOW];
                add posit [{(+j, value)}, { "a": 1.00 }, @NOW];
                add posit [{(+t, value)}, '2024-05', @NOW];
                "#,
            )
            .expect("persist literal families");
    }

    let restored = Database::new(PersistenceMode::File(path.clone())).expect("restore store");
    let result = Engine::new(&restored)
        .execute_collect("search [{(*, value), ...}, ?literal, *] return ?literal;")
        .expect("query restored literals");
    let mut tokens: Vec<String> = result.rows.iter().map(|row| row[0].text.clone()).collect();
    tokens.sort();
    let mut expected = vec![
        r#""\u0041""#.to_string(),
        "'2024-05'".to_string(),
        "+001".to_string(),
        "-9223372036854775808".to_string(),
        "01.00".to_string(),
        "075%".to_string(),
        "10".to_string(),
        "75%".to_string(),
        "9223372036854775807".to_string(),
        r#"{ "a": 1.00 }"#.to_string(),
    ];
    expected.sort();
    assert_eq!(tokens, expected);
    drop(restored);
    remove_store(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn information_in_effect_survives_append_only_restart() {
    use positorium::traqula::Engine;

    let path = temporary_store_path("information-in-effect");
    {
        let database = Database::new(PersistenceMode::File(path.clone())).unwrap();
        Engine::new(&database)
            .execute(
                "add role status; \
                 add posit +target [{(+case, status)}, \"open\", '2024-01-01']; \
                 add posit [{(target, posit), (+source, ascertains)}, 80%, '2024-02-01'];",
            )
            .unwrap();
    }

    let restored = Database::new(PersistenceMode::File(path.clone())).unwrap();
    let result = Engine::new(&restored)
        .execute_collect(
            "search [{(?case, status)}, ?state, *] \
             in effect '2025-01-01', '2025-01-01' \
             return ?case, ?state;",
        )
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][1].text, "\"open\"");
    drop(restored);
    remove_store(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn explicit_shutdown_flush_leaves_a_reopenable_store() {
    let path = temporary_store_path("shutdown-flush");
    {
        let database = Database::new(PersistenceMode::File(path.clone())).unwrap();
        positorium::traqula::Engine::new(&database)
            .execute("add role shutdown; add posit [{(+item, shutdown)}, \"clean\", @NOW];")
            .unwrap();
        database.flush().unwrap();
    }

    let reopened = Database::new(PersistenceMode::File(path.clone())).unwrap();
    assert!(reopened.contains_role("shutdown").unwrap());
    drop(reopened);
    remove_store(&path);
}

#[test]
#[cfg(feature = "persistence")]
fn committed_checksum_corruption_fails_open_without_rewriting_source() {
    use std::io::{Seek, SeekFrom, Write};

    let path = temporary_store_path("corruption");
    {
        let db = Database::new(PersistenceMode::File(path.clone())).expect("create store");
        positorium::traqula::Engine::new(&db)
            .execute_collect("add role value;")
            .expect("append role");
    }
    let log_path = std::path::Path::new(&path).join("log-0000000000000001.ptl");
    let mut bytes = std::fs::read(&log_path).expect("read log");
    bytes[90] ^= 0x40;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&log_path)
        .expect("open log");
    file.seek(SeekFrom::Start(90)).unwrap();
    file.write_all(&bytes[90..91]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let corrupted = std::fs::read(&log_path).unwrap();

    let error = match Database::new(PersistenceMode::File(path.clone())) {
        Ok(_) => panic!("committed corruption must fail startup"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("checksum"), "{error}");
    assert_eq!(std::fs::read(&log_path).unwrap(), corrupted);
    remove_store(&path);
}
