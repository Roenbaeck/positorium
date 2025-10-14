use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

fn setup() -> Engine<'static> {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(Box::leak(Box::new(db)));
    engine.execute("add role number; add role confidence; add posit [{(+n1, number)}, 5, @NOW]; add posit [{(+n2, number)}, 10, @NOW]; add posit [{(+c1, confidence)}, 60%, @NOW]; add posit [{(+c2, confidence)}, 75%, @NOW];");
    engine
}

#[test]
fn numeric_var_ordering() {
    let engine = setup();
    // Expect only (5,10)
    let script = "search [{(*, number)}, +a, +ta], [{(*, number)}, +b, +tb] where a < b return a, b;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0], vec!["5".to_string(), "10".to_string()]);
}

#[test]
fn numeric_var_equality() {
    let engine = setup();
    // a = b should yield (5,5) and (10,10)
    let script = "search [{(*, number)}, +a, *], [{(*, number)}, +b, *] where a = b return a, b;";
    let res = engine.execute_collect(script).expect("query ok");
    let mut pairs: Vec<(String,String)> = res.rows.into_iter().map(|r| (r[0].clone(), r[1].clone())).collect();
    pairs.sort();
    assert_eq!(pairs, vec![("10".into(),"10".into()), ("5".into(),"5".into())]);
}

#[test]
fn certainty_var_ordering() {
    let engine = setup();
    // 60% < 75%
    let script = "search [{(*, confidence)}, +c1, *], [{(*, confidence)}, +c2, *] where c1 < c2 return c1, c2;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], "0.60");
    assert_eq!(res.rows[0][1], "0.75");
}

#[test]
fn certainty_mixed_ordering_error() {
    let engine = setup();
    // Mix number and certainty in ordering -> should error
    let script = "search [{(*, number)}, +n, *], [{(*, confidence)}, +c, *] where n < c return n, c;";
    let err = engine.execute_collect(script).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("Ordering comparison") || msg.contains("certainty"), "unexpected msg: {msg}");
}

#[test]
fn string_ordering_error() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(Box::leak(Box::new(db)));
    engine.execute("add role label; add posit [{(+l1, label)}, \"alpha\", @NOW]; add posit [{(+l2, label)}, \"beta\", @NOW];");
    let script = "search [{(*, label)}, +l1, *], [{(*, label)}, +l2, *] where l1 < l2 return l1, l2;";
    let err = engine.execute_collect(script).unwrap_err();
    assert!(format!("{}", err).contains("Ordering comparison not allowed"));
}

#[test]
fn numeric_mixed_decimal_int() {
    let engine = setup();
    engine.execute("add posit [{(+n3, number)}, 10.00, @NOW];");
    let script = "search [{(*, number)}, +a, *], [{(*, number)}, +b, *] where a = b return a, b;";
    let res = engine.execute_collect(script).expect("query ok");
    // Should include 10 vs 10.00 equality pairs (treat numerically equal)
    let any_mixed = res.rows.iter().any(|r| r[0] == "10" && r[1] == "10.00" || r[0] == "10.00" && r[1] == "10");
    assert!(any_mixed, "expected mixed decimal/int equality to be recognized");
}
