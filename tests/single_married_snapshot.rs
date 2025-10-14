use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

// Diagnostic: ensure the married snapshot search by itself yields 5 rows (names) when not preceded by divorced search.
#[test]
fn single_married_snapshot_yields_rows() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let script = r#"
add role wife; add role husband; add role name;
add posit [{(+idw, wife), (+idh, husband)}, "married", '2004-06-19'],
          [{(idw, wife), (idh, husband)}, "divorced", '2020-12-04'],
          [{(idw, wife), (idh, husband)}, "married", '2024-03-17'],
          [{(idh, name)}, "Archie Bald", '1972-08-20'],
          [{(idh, name)}, "Archie Trix", '2004-09-21'],
          [{(idh, name)}, "Archie Bald", '2021-01-19'],
          [{(idw, name)}, "Bella Trix", '1972-12-13'],
          [{(idw, name)}, "Bella Bald", '2024-05-29'];

search [{(+w, wife), (+h, husband)}, "married", +mt] as of @NOW, [{(w|h, name)}, +n2, +t2]
return n2, t2, mt;"#;
    let results = engine.execute_collect_multi(script).expect("multi ok");
    assert_eq!(results.len(), 1, "one search");
    assert_eq!(results[0].row_count, 5, "married snapshot names by itself");
}
