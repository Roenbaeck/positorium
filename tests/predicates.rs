use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

fn setup() -> Engine<'static> {
    // Create a static DB for test scope (lifetime workaround: leak Box)
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    // Seed roles and posits
    let engine = Engine::new(Box::leak(Box::new(db)));
    // Insert roles / posits covering numeric, string, certainty
    engine.execute("add role number; add role label; add role confidence; add posit [{(+n1, number)}, 5, @NOW]; add posit [{(+n2, number)}, 10, @NOW]; add posit [{(+l1, label)}, \"alpha\", @NOW]; add posit [{(+c1, confidence)}, 75%, @NOW];");
    engine
}

#[test]
fn certainty_literal_equivalents() {
    let engine = setup();
    // Only percent-suffixed literals are valid certainty values now.
    let positive_match = ["75%", "075%" /* leading zero still percent? -> treated as 75% */, "75%" ];
    for f in &positive_match {
        let script = format!("search [{{(*, confidence)}}, +c, *] where c = {f} return c;");
        let res = engine.execute_collect(&script).expect("query ok");
        assert_eq!(res.rows.len(), 1, "percent form {f} should match");
    }
    let negative_match = ["0.75", "75", "74%", "0.749"]; // invalid certainty forms or different value
    for f in &negative_match {
        let script = format!("search [{{(*, confidence)}}, +c, *] where c = {f} return c;");
        // Forms without % are treated as decimal/int -> should not match certainty posit
        let res = engine.execute_collect(&script).unwrap_or_else(|e| panic!("unexpected error for form {f}: {e}"));
        assert_eq!(res.rows.len(), 0, "form {f} should NOT match certainty 75%");
    }
}

#[test]
fn certainty_ordering() {
    let engine = setup();
    // Using percent: 75% >= 70% succeeds
    let script = "search [{(*, confidence)}, +c, *] where c >= 70% return c;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    // Greater-than a higher percent returns 0
    let script = "search [{(*, confidence)}, +c, *] where c > 80% return c;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 0);
    // Missing percent should trigger an error
    let err = engine.execute_collect("search [{(*, confidence)}, +c, *] where c > 80 return c;").unwrap_err();
    assert!(format!("{}", err).contains("percent sign"));
}

#[test]
fn numeric_pruning() {
    let engine = setup();
    // Should prune to the value 10 only
    let script = "search [{(*, number)}, +n, *] where n > 5 return n;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 1);
    assert_eq!(res.rows[0][0], "10");
}

#[test]
fn error_unknown_variable() {
    let engine = setup();
    let script = "search [{(*, number)}, +n, *] where x = 5 return n;"; // x never bound
    let err = engine.execute_collect(script).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("Unknown variable"));
}

#[test]
fn error_type_mismatch_ordering() {
    let engine = setup();
    let script = "search [{(*, label)}, +l, *] where l < 5 return l;"; // string ordering invalid
    let err = engine.execute_collect(script).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("Ordering comparison not allowed") || msg.contains("Type mismatch"));
}

#[test]
fn time_where_regression() {
    // Build a small dataset with a time variable bound earlier than the where predicate filter.
    let engine = setup();
    // Add minimal roles / posits to simulate marriage-like pattern with times.
    engine.execute("add role wife; add role husband; add role name; add role posit; add role ascertains; add posit [{(+w1, wife), (+h1, husband)}, \"married\", '2012-12-12']; add posit [{(+p1, posit), (+a1, ascertains)}, 0%, @NOW]; add posit [{(+h1, name)}, \"Bob\", '2012-12-12'];");
    // Query where bound time t1 is compared to a future date -> expect zero rows
    let script = "search +p [{(+w, wife), (+h, husband)}, \"married\", +t1] as of '2012-12-12', [{(p, posit), (*, ascertains)}, +c, +at] as of @NOW, [{(h, name)}, +n, +t] as of t1 where t1 > '2022-01-01' return n, t, c, at;";
    let res = engine.execute_collect(script).expect("query ok");
    assert_eq!(res.rows.len(), 0, "time predicate should filter all rows");
}
