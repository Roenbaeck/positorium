use positorium::{
    Database, DatabaseError, Engine, MultiStreamCallbacks, PersistenceMode, ResultCell, RowSink,
    SinkFlow,
};

fn database(script: &str) -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database).execute(script).unwrap();
    database
}

#[derive(Default)]
struct Events(Vec<&'static str>);

impl RowSink for Events {
    fn on_meta(&mut self, _: &[String]) -> SinkFlow {
        self.0.push("meta");
        SinkFlow::Continue
    }

    fn push(&mut self, _: Vec<ResultCell>) -> SinkFlow {
        self.0.push("row");
        SinkFlow::Continue
    }
}

impl MultiStreamCallbacks for Events {
    fn on_result_set_start(&mut self, _: usize, _: &[String], _: &str) {
        self.0.push("meta");
    }

    fn on_row(&mut self, _: usize, _: Vec<ResultCell>) -> bool {
        self.0.push("row");
        true
    }

    fn on_result_set_end(&mut self, _: usize, _: usize, _: bool) {
        self.0.push("end");
    }
}

#[test]
fn temporal_patterns_are_invariant_under_external_field_binding_order() {
    let database = database(
        r#"add role status, marker;
        add posit [{(+a, status)}, "old", '2020'],
                  [{(a, status)}, "new", '2021'],
                  [{(+m, marker)}, "old", '2020'];"#,
    );
    let engine = Engine::new(&database);
    for temporal in ["", "latest "] {
        for (history, snapshot) in [
            (
                "?p = [{(?x, status)}, \"old\", *]".to_string(),
                format!("{temporal}?p = [{{(?x, status)}}, ?v, *] as of '2022'"),
            ),
            (
                "[{(*, marker)}, ?v, *]".to_string(),
                format!("{temporal}[{{(?x, status)}}, ?v, *] as of '2022'"),
            ),
            (
                "[{(*, marker)}, *, ?t]".to_string(),
                format!("{temporal}[{{(?x, status)}}, ?v, ?t] as of '2022'"),
            ),
        ] {
            for (first, second) in [(&history, &snapshot), (&snapshot, &history)] {
                let query = format!("search {first}, {second} return ?v;");
                let result = engine.execute_collect(&query).unwrap();
                assert!(result.rows.is_empty(), "{query}: {:?}", result.rows);
            }
        }
    }

    // A literal restriction belongs to the latest-matching operation itself.
    let latest = engine
        .execute_collect("search latest [{(?x, status)}, \"old\", ?t] as of '2022' return ?t;")
        .unwrap();
    assert_eq!(latest.rows, vec![vec!["2020".to_string()]]);

    // Correlation must not resurrect an old target inside an ordinary snapshot.
    let absent = engine
        .execute_collect(
            "search ?p = [{(?x, status)}, \"old\", *] \
             not exists { ?p = [{(?x, status)}, *, *] as of '2022' } return ?p;",
        )
        .unwrap();
    assert_eq!(absent.row_count, 1);
}

#[test]
fn snapshot_identity_correlation_retains_equal_and_incomparable_maxima() {
    let database = database(
        r#"add role status;
        add posit [{(+a, status)}, "year", '2024'],
                  [{(a, status)}, "day", '2024-06-01'],
                  [{(a, status)}, "tie", '2024-06-01'];"#,
    );
    let engine = Engine::new(&database);
    for (first, second) in [
        (
            "?p = [{(?x, status)}, ?v, *]",
            "?p = [{(?x, status)}, *, *] as of '2025'",
        ),
        (
            "?p = [{(?x, status)}, *, *] as of '2025'",
            "?p = [{(?x, status)}, ?v, *]",
        ),
    ] {
        let query = format!("search {first}, {second} return ?p, ?v order by ?p;");
        assert_eq!(engine.execute_collect(&query).unwrap().row_count, 3);
    }
}

#[test]
fn thing_unions_preserve_exact_and_open_set_matching() {
    let database = database(
        r#"add role left, right, name, tag;
        add posit [{(+a, left), (+b, right)}, 1, '2024'],
                  [{(a, name), (+t, tag)}, "tagged", '2024'],
                  [{(b, name)}, "plain", '2024'];"#,
    );
    let engine = Engine::new(&database);
    for (suffix, expected) in [("", 1), (", ...", 2)] {
        let query = format!(
            "search [{{(?a, left), (?b, right)}}, *, *], \
             [{{(?a|?b, name){suffix}}}, ?v, *] return ?v;"
        );
        assert_eq!(engine.execute_collect(&query).unwrap().row_count, expected);
    }
}

#[test]
fn invalid_queries_fail_before_output_or_fallback_in_every_data_state() {
    let queries = [
        "search [{(?x, name)}, ?v, *] not exists { [{(?local, other)}, *, *] } \
         return ?local or add posit [{(+local, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] union [{(?y, other)}, *, *] \
         return ?x or add posit [{(+x, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] union ?x = [{(*, other)}, *, *] \
         return ?x or add posit [{(+x, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] as of ?cut, [{(*, other)}, ?cut, *] \
         return ?x or add posit [{(+x, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] where ?x = 1 \
         return ?x or add posit [{(+x, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] union [{(?y, other)}, *, *] \
         not exists { [{(?x, other)}, *, *] };",
        "search [{(?x, name)}, *, *] not exists { \
         [{(*, other)}, *, ?a] as of ?b, [{(*, name)}, *, ?b] as of ?a } \
         return ?x or add posit [{(+x, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] return ?x limit -1 \
         or add posit [{(+x, name)}, 2, '2024'];",
        "search [{(?x, name)}, *, *] return ?x limit 999999999999999999999999999999999 \
         or add posit [{(+x, name)}, 2, '2024'];",
    ];
    for populated in [false, true] {
        for query in queries {
            let database = database("add role name, other;");
            let engine = Engine::new(&database);
            if populated {
                engine
                    .execute("add posit [{(+x, name)}, 1, '2024'];")
                    .unwrap();
            }
            let before = engine
                .execute_collect("search ?p = [*, *, *] return ?p;")
                .unwrap()
                .row_count;
            assert!(engine.execute_collect(query).is_err(), "{query}");
            assert!(engine.execute_collect_multi(query).is_err(), "{query}");
            let mut events = Events::default();
            assert!(
                engine.execute_stream_multi(query, &mut events).is_err(),
                "{query}"
            );
            assert!(events.0.is_empty(), "{query}: {:?}", events.0);
            if query.contains("return") {
                assert!(
                    engine.execute_stream_single(query, &mut events).is_err(),
                    "{query}"
                );
                assert!(events.0.is_empty(), "{query}: {:?}", events.0);
            }
            assert_eq!(
                engine
                    .execute_collect("search ?p = [*, *, *] return ?p;")
                    .unwrap()
                    .row_count,
                before,
                "invalid query mutated: {query}"
            );
        }
    }
}

#[test]
fn negation_locals_are_independent_and_union_branches_may_have_private_bindings() {
    let database = database(
        r#"add role name, other;
        add posit [{(+x, name)}, "Alice", '2024'];"#,
    );
    let result = Engine::new(&database)
        .execute_collect(
            "search [{(?x, name)}, ?branch_value, *] union [{(?x, other)}, *, ?branch_time] \
             not exists { [{(?local, other)}, *, *] } \
             not exists { [{(*, other)}, ?local, *] } return ?x;",
        )
        .unwrap();
    assert_eq!(result.row_count, 1);

    // Private names may have different domains across branches and are not
    // promoted into the script's mutation bindings after the union.
    let engine = Engine::new(&database);
    let branches = "search [{(?x, name)}, ?private, *] \
        union [{(?x, name)}, *, ?private] union [{(?x, name)}, *, *] return ?x;";
    assert_eq!(engine.execute_collect(branches).unwrap().row_count, 3);
    assert!(
        engine
            .execute(
                "search [{(?x, name)}, *, *] union [{(?y, name)}, *, *]; \
        add posit [{(x, other)}, 1, '2024'];"
            )
            .is_err()
    );
}

#[test]
fn thing_posit_identity_mismatches_are_nonmatching_rows_in_either_order() {
    let database = database(
        "add role status, reference; \
         add posit +old [{(+entity, status)}, 1, '2020']; \
         add posit +new [{(entity, status)}, 2, '2021'], \
                   [{(old, reference)}, 1, '2020']; \
         add posit [{(new, posit), (+source, ascertains)}, 100%, '2021'];",
    );
    let engine = Engine::new(&database);
    for temporal in ["as of '2022'", "in effect '2022', '2022'"] {
        let target = format!("?p = [{{(*, status)}}, *, *] {temporal}");
        let reference = "[{(?p, reference)}, *, *]";
        for (first, second) in [(reference, target.as_str()), (target.as_str(), reference)] {
            let query = format!("search {first}, {second} return ?p;");
            assert!(engine.execute_collect(&query).unwrap().rows.is_empty());
        }
    }
    assert!(
        engine
            .execute_collect("search ?p = [{(?p, status)}, *, *] return ?p;")
            .unwrap()
            .rows
            .is_empty()
    );
}

#[test]
fn ordering_rejects_mixed_certainty_and_numeric_domains_before_streaming() {
    let mixed = database(
        "add role value; add posit [{(+a, value)}, 10, '2024'], \
         [{(+b, value)}, 50%, '2024'], [{(+c, value)}, 2, '2024'];",
    );
    let engine = Engine::new(&mixed);
    let query = "search [{(*, value)}, ?v, *] return ?v order by ?v;";
    assert!(matches!(
        engine.execute_collect(query),
        Err(DatabaseError::Comparison(_))
    ));
    let mut events = Events::default();
    assert!(matches!(
        engine.execute_stream_single(query, &mut events),
        Err(DatabaseError::Comparison(_))
    ));
    assert!(events.0.is_empty());

    let database = database(
        "add role value; add posit [{(+a, value)}, 10, '2024'], [{(+b, value)}, 2.00, '2024'];",
    );
    let ordered = Engine::new(&database).execute_collect(query).unwrap();
    assert_eq!(
        ordered.rows,
        vec![vec!["2.00".to_string()], vec!["10".to_string()]]
    );
}
