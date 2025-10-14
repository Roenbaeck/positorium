use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

// Regression: reusing +w,+h across searches should preserve bindings (proper intersection),
// not toggle them away. Second search must still yield rows.
#[test]
fn cross_search_binding_persists() {
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

/* divorced historical */
search [{(+w, wife), (+h, husband)}, "divorced", *], [{(w|h, name)}, +n, +t]
return n, t;

/* current married */
search [{(+w, wife), (+h, husband)}, "married", +mt] as of @NOW, [{(w|h, name)}, +n2, +t2]
return n2, t2, mt;
"#;
    let results = engine.execute_collect_multi(script).expect("multi ok");
    assert_eq!(results.len(), 2, "two searches");
    assert_eq!(results[0].row_count, 5, "historical divorced names");
    assert_eq!(results[1].row_count, 5, "current married snapshot names");
    assert!(results[1].rows.iter().all(|r| r.last().unwrap().contains("2024") || r.last().unwrap().contains("2025") || r.last().unwrap().contains("202")), "snapshot time present");
}
