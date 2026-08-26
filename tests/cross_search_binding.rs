use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

#[test]
fn query_bindings_are_lexical_to_one_search() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let script = r#"
add role color;
add posit [{(+red_item, color)}, "red", '2024-01-01'],
          [{(+blue_item, color)}, "blue", '2024-01-01'];

search [{(+item, color)}, "red", *]
return item;

search [{(+item, color)}, +value, *]
return item, value;
"#;

    let results = engine.execute_collect_multi(script).expect("multi search");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].row_count, 1);
    assert_eq!(results[1].row_count, 2);
    let mut values: Vec<&str> = results[1].rows.iter().map(|row| row[1].as_str()).collect();
    values.sort_unstable();
    assert_eq!(values, vec!["\"blue\"", "\"red\""]);
}
