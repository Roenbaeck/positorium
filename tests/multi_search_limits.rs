use bareclad::construct::{Database, PersistenceMode};
use bareclad::traqula::Engine;

// Verifies that each search command applies its own LIMIT independently.
// Script under test:
// search +p [{(h, name)}, +n, +t] return p, h, n, t limit 2;
// search [{(*, name)}, +n2, +t2] return n2, t2 limit 1;
#[test]
fn per_search_limits() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    engine.execute("add role name; add role h; add posit [{(+h1, h)}, 1, @NOW]; add posit [{(+h2, h)}, 2, @NOW]; add posit [{(+h3, h)}, 3, @NOW]; add posit [{(+a, name)}, \"Alice\", @NOW]; add posit [{(+b, name)}, \"Bob\", @NOW]; add posit [{(+c, name)}, \"Carol\", @NOW];");
    let script = r#"
search +p [{(*, name)}, +n, +t]
return
    p, n, t
limit 2;

search [{(*, name)}, +n2, +t2]
return
    n2, t2
limit 1;
"#;
    let results = engine.execute_collect_multi(script).expect("multi ok");
    assert_eq!(results.len(), 2, "two searches");
    assert_eq!(results[0].row_count, 2, "first search limited to 2 rows");
    assert!(results[0].limited, "first search limited flag");
    assert_eq!(results[1].row_count, 1, "second search limited to 1 row");
    assert!(results[1].limited, "second search limited flag");
}
