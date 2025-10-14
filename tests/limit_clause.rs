use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

#[test]
fn limit_after_return_parses() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    engine.execute("add role name; add posit [{(+x, name)}, \"Alice\", @NOW]; add posit [{(+y, name)}, \"Bob\", @NOW];");
    let res = engine.execute_collect("search [{(*, name)}, +n, *] return n limit 1;").expect("parse ok");
    assert_eq!(res.columns, vec!["n"], "column name aligned");
    assert_eq!(res.row_count, 1, "limit should restrict to exactly one row");
    assert!(res.limited, "limited flag should be true when limit reached");
}

#[test]
fn reserved_keyword_not_variable() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    engine.execute("add role name; add posit [{(+x, name)}, \"Alice\", @NOW]; add posit [{(+y, name)}, \"Bob\", @NOW];");
    // Previously this would fail because 'limit' was consumed as variable; now it must parse as clause.
    let res = engine.execute_collect("search [{(*, name)}, +n, *] return n limit 2;").expect("parse ok");
    assert!(res.row_count <= 2);
    assert!(res.limited, "two rows present so limit reached");
}
