#![cfg(feature = "persistence")]

use positorium::construct::{Database, PersistenceMode};
use positorium::maintenance::{backup_store, export_store, import_store, inspect_store};
use positorium::traqula::Engine;
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_CASE: AtomicU64 = AtomicU64::new(1);

struct TemporaryCase(PathBuf);

impl TemporaryCase {
    fn new(label: &str) -> Self {
        let ordinal = NEXT_CASE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "positorium-maintenance-{label}-{}-{ordinal}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TemporaryCase {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn populate(path: &Path) -> Vec<String> {
    let database =
        Database::new(PersistenceMode::File(path.to_string_lossy().into_owned())).unwrap();
    Engine::new(&database)
        .execute_collect(
            r#"
            add role value;
            add posit [{(+s, value)}, "\u0041", @NOW];
            add posit [{(+i, value)}, +001, @NOW];
            add posit [{(+d, value)}, 01.00, @NOW];
            add posit [{(+c, value)}, 075%, @NOW];
            add posit [{(+j, value)}, { "a": 1.00 }, @NOW];
            add posit [{(+t, value)}, '2024-05', @NOW];
            "#,
        )
        .unwrap();
    drop(database);
    vec![
        r#""\u0041""#.to_string(),
        "'2024-05'".to_string(),
        "+001".to_string(),
        "01.00".to_string(),
        "075%".to_string(),
        r#"{ "a": 1.00 }"#.to_string(),
    ]
}

fn query_tokens(path: &Path) -> Vec<String> {
    let database =
        Database::new(PersistenceMode::File(path.to_string_lossy().into_owned())).unwrap();
    let result = Engine::new(&database)
        .execute_collect("search [{(*, value), ...}, ?literal, *] return ?literal;")
        .unwrap();
    let mut tokens = result
        .rows
        .into_iter()
        .map(|row| row[0].text.clone())
        .collect::<Vec<_>>();
    tokens.sort();
    tokens
}

#[test]
fn logical_export_import_is_lossless_and_remaps_every_non_builtin_identity() {
    let case = TemporaryCase::new("logical-transfer");
    let source = case.path("source.store");
    let export = case.path("source.jsonl");
    let destination = case.path("destination.store");
    let remap = case.path("identity-remap.json");
    let mut expected = populate(&source);
    expected.sort();

    let source_inspection = inspect_store(&source).unwrap();
    let export_report = export_store(&source, &export).unwrap();
    assert_eq!(export_report.roles, 3);
    assert_eq!(export_report.posits, 6);

    let import_report = import_store(&export, &destination, &remap).unwrap();
    assert_ne!(
        import_report.source_store_uuid,
        import_report.destination_store_uuid
    );
    assert_eq!(import_report.roles, source_inspection.roles);
    assert_eq!(import_report.posits, source_inspection.posits);
    assert_eq!(query_tokens(&destination), expected);

    let remap_document: Value = serde_json::from_slice(&std::fs::read(remap).unwrap()).unwrap();
    let mappings = remap_document["mappings"].as_array().unwrap();
    assert_eq!(mappings.len(), import_report.remapped_identities);
    for mapping in mappings {
        let source_local = mapping["source"]["local"].as_u64().unwrap();
        let destination_local = mapping["destination"]["local"].as_u64().unwrap();
        if !matches!(source_local, 1 | 2) {
            assert_ne!(source_local, destination_local);
        }
        assert_eq!(
            mapping["source"]["store_uuid"],
            import_report.source_store_uuid
        );
        assert_eq!(
            mapping["destination"]["store_uuid"],
            import_report.destination_store_uuid
        );
    }
}

#[test]
fn physical_backup_copies_only_the_committed_prefix_and_preserves_source_tail() {
    let case = TemporaryCase::new("backup");
    let source = case.path("source.store");
    let destination = case.path("backup.store");
    let expected = populate(&source);
    let log = source.join("log-0000000000000001.ptl");
    OpenOptions::new()
        .append(true)
        .open(&log)
        .unwrap()
        .write_all(b"uncommitted-tail")
        .unwrap();
    let source_length = std::fs::metadata(&log).unwrap().len();

    let before = inspect_store(&source).unwrap();
    assert_eq!(before.ignored_tail_bytes, 16);
    let report = backup_store(&source, &destination).unwrap();
    assert_eq!(report.store_uuid, before.store_uuid);
    assert_eq!(std::fs::metadata(&log).unwrap().len(), source_length);

    let backup = inspect_store(&destination).unwrap();
    assert_eq!(backup.ignored_tail_bytes, 0);
    assert_eq!(backup.physical_log_length, before.committed_length);
    let mut actual = query_tokens(&destination);
    actual.sort();
    let mut expected = expected;
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn maintenance_outputs_cannot_be_written_inside_the_store() {
    let case = TemporaryCase::new("nested-target");
    let source = case.path("source.store");
    populate(&source);
    assert!(export_store(&source, source.join("dump.jsonl")).is_err());
    assert!(backup_store(&source, source.join("backup.store")).is_err());
}

#[test]
fn read_only_tools_refuse_a_store_owned_by_a_writer() {
    let case = TemporaryCase::new("writer-lock");
    let source = case.path("source.store");
    let database =
        Database::new(PersistenceMode::File(source.to_string_lossy().into_owned())).unwrap();
    let error = inspect_store(&source).unwrap_err();
    assert!(error.to_string().contains("owned by a writer"), "{error}");
    drop(database);
    inspect_store(&source).unwrap();
}

#[test]
fn unsupported_export_version_fails_before_creating_a_destination() {
    let case = TemporaryCase::new("export-version");
    let source = case.path("source.store");
    let export = case.path("source.jsonl");
    let invalid = case.path("invalid.jsonl");
    let destination = case.path("destination.store");
    let remap = case.path("remap.json");
    populate(&source);
    export_store(&source, &export).unwrap();
    let contents = std::fs::read_to_string(export).unwrap();
    std::fs::write(
        &invalid,
        contents.replacen("\"version\":1", "\"version\":2", 1),
    )
    .unwrap();

    let error = import_store(&invalid, &destination, &remap).unwrap_err();
    assert!(error.to_string().contains("unsupported logical export"));
    assert!(!destination.exists());
    assert!(!remap.exists());
}
