use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

fn setup() -> Engine<'static> {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(Box::leak(Box::new(db)));
    // Base roles
    engine.execute("add role event; add role label;");
    // Seed two events with different times
    engine.execute("add posit [{(+e1, event)}, \"alpha\", '2010-01-01']; add posit [{(+e2, event)}, \"beta\", '2020-01-01'];");
    engine
}

#[test]
fn time_literal_ordering() {
    let engine = setup();
    // t < 2015 should only return the 2010 posit
    let script = "search [{(*, event)}, +lbl, +t] where t < '2015-01-01' return t;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], "2010-01-01");

    // t >= 2015 should only return the 2020 posit
    let script = "search [{(*, event)}, +lbl, +t] where t >= '2015-01-01' return t;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], "2020-01-01");
}

#[test]
fn time_range_and() {
    let engine = setup();
    // Inclusive range capturing only 2010 row
    let script = "search [{(*, event)}, +lbl, +t] where t >= '2010-01-01' and t <= '2010-01-01' return t;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], "2010-01-01");

    // Range excluding all (before earliest)
    let script = "search [{(*, event)}, +lbl, +t] where t < '2000-01-01' and t > '1999-12-31' return t;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 0);
}

#[test]
fn time_variable_variable_ordering() {
    let engine = setup();
    // Expect exactly one ordered pair (2010 < 2020) times two label combinations? Actually bindings carry values not labels; return times only here.
    let script = "search [{(*, event)}, +v1, +t1], [{(*, event)}, +v2, +t2] where t1 < t2 return t1, t2;";
    let res = engine.execute_collect(script).expect("query ok");
    // There should be exactly one distinct (t1,t2) pair
    assert_eq!(res.rows.len(), 1, "expected single ordered time pair");
    assert_eq!(res.rows[0][0], "2010-01-01");
    assert_eq!(res.rows[0][1], "2020-01-01");
}

#[test]
fn time_variable_variable_no_match() {
    // Only one event so no pair with t1 < t2
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(Box::leak(Box::new(db)));
    engine.execute("add role event; add posit [{(+e1, event)}, \"solo\", '2015-05-05'];");
    let script = "search [{(*, event)}, +v1, +t1], [{(*, event)}, +v2, +t2] where t1 < t2 return t1, t2;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 0, "no strictly ordered pair with a single time");
}

#[test]
fn time_unknown_variable() {
    let engine = setup();
    // Predicate references unknown time variable x. Expect zero rows due to filter elimination.
    let script = "search [{(*, event)}, +lbl, +t] where x > '2015-01-01' return t;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 0, "unknown variable should yield no matches");
}
