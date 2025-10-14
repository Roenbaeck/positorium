use positorium::construct::{Database, PersistenceMode};

#[test]
fn in_memory_mode_allows_basic_operations() {
    let db = Database::new(PersistenceMode::InMemory).expect("db");
    let (role, existed) = db.create_role("person".to_string(), false);
    assert!(!existed);
    let thing = db.create_thing();
    let (appearance, _) = db.create_apperance(*thing, role);
    let (_aset, _) = db.create_appearance_set(vec![appearance]);
    // No ledger head should exist (no persistence)
    assert!(db.persistor.lock().unwrap().current_superhash().is_none());
}

#[test]
fn file_mode_persists_and_has_ledger() {
    // Use a temp path; reuse the same file to ensure ledger creation
    let path = "test_positorium_temp.db".to_string();
    // Ensure clean start
    let _ = std::fs::remove_file(&path);
    let db = Database::new(PersistenceMode::File(path.clone())).expect("db");
    let (role, _) = db.create_role("audit".to_string(), false);
    let thing = db.create_thing();
    let (appearance, _) = db.create_apperance(*thing, role);
    let (aset, _) = db.create_appearance_set(vec![appearance]);
    // Insert a posit to trigger ledger append
    let time = positorium::datatype::Time::new();
    let _posit = db.create_posit(aset, "ok".to_string(), time);
    let head = db.persistor.lock().unwrap().current_superhash();
    assert!(
        head.is_some(),
        "expected ledger head after posit insertion in file-backed mode"
    );
    // Clean up
    let _ = std::fs::remove_file(&path);
}
