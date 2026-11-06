use positorium::{
    CancellationToken, Database, DatabaseError, Engine, ExecutionOptions, MultiStreamCallbacks,
    PersistenceMode, PlanOperator, ResultCell, RowSink, SinkFlow,
};

fn fixture() -> Database {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            "add role left, right, empty, copy; add posit \
         [{(+a, left)}, 2, '2024'], [{(+b, left)}, 1, '2024'], \
         [{(+c, left)}, 1.0, '2024'], [{(a, right)}, 3, '2024'], \
         [{(b, right)}, 4, '2024'];",
        )
        .unwrap();
    database
}

#[derive(Default)]
struct Events {
    metadata: usize,
    rows: Vec<Vec<ResultCell>>,
    ends: Vec<(usize, bool)>,
    stop: bool,
}

impl RowSink for Events {
    fn on_meta(&mut self, _: &[String]) -> SinkFlow {
        self.metadata += 1;
        if self.stop {
            SinkFlow::Stop
        } else {
            SinkFlow::Continue
        }
    }
    fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
        self.rows.push(row);
        SinkFlow::Continue
    }
}

impl MultiStreamCallbacks for Events {
    fn on_result_set_start(&mut self, _: usize, _: &[String], _: &str) {
        self.metadata += 1;
    }
    fn on_row(&mut self, _: usize, row: Vec<ResultCell>) -> bool {
        self.rows.push(row);
        true
    }
    fn on_result_set_end(&mut self, _: usize, count: usize, limited: bool) {
        self.ends.push((count, limited));
    }
}

#[test]
fn limits_and_option_caps_use_one_distinct_lookahead_on_every_surface() {
    let database = fixture();
    let engine = Engine::new(&database);
    for query in [
        "search [{(?x, left)}, ?v, *] return ?v",
        "search [{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] return ?v, ?w",
        "search [{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] return distinct ?v",
        "search [{(?x, left)}, ?v, *] union [{(?x, left)}, ?v, *] return distinct ?v",
        "search [{(?x, left)}, ?v, *] not exists { [{(?x, right)}, *, *] } return ?v",
        "search [{(?x, empty)}, ?v, *] return ?v",
    ] {
        for order in ["", " order by ?v", " order by ?v desc"] {
            let full = engine.execute_collect(&format!("{query}{order};")).unwrap();
            for limit in [0, 1, 2, 3, 6, usize::MAX] {
                let script = format!("{query}{order} limit {limit};");
                let expected = &full.rows[..full.row_count.min(limit)];
                let limited = full.row_count > limit;
                let result = engine.execute_collect(&script).unwrap();
                assert_eq!(result.rows, expected, "{script}");
                assert_eq!(result.limited, limited, "{script}");
                let mut single = Events::default();
                let (_, actual_limited, count) =
                    engine.execute_stream_single(&script, &mut single).unwrap();
                assert_eq!(
                    (actual_limited, count),
                    (limited, expected.len()),
                    "{script}"
                );
                assert_eq!(single.rows, expected);
                assert_eq!(single.metadata, 1);
                let mut multi = Events::default();
                engine.execute_stream_multi(&script, &mut multi).unwrap();
                assert_eq!(multi.rows, expected);
                assert_eq!(multi.ends, [(expected.len(), limited)]);
                let capped = engine
                    .execute_collect_with_options(
                        &format!("{query}{order};"),
                        ExecutionOptions {
                            max_rows_per_search: Some(limit),
                            ..ExecutionOptions::default()
                        },
                    )
                    .unwrap();
                assert_eq!(capped.rows, expected);
                assert_eq!(capped.limited, limited);
            }
        }
    }
}

#[test]
fn top_k_is_stable_across_numeric_ties_and_multiple_keys() {
    let database = fixture();
    let engine = Engine::new(&database);
    for keys in ["?v", "?v desc", "?v, ?w desc", "?w desc, ?v desc"] {
        let query = format!(
            "search [{{(?x, left)}}, ?v, *], [{{(?y, right)}}, ?w, *] return ?v, ?w order by {keys}"
        );
        let all = engine.execute_collect(&format!("{query};")).unwrap();
        for limit in 0..=7 {
            let limited = engine
                .execute_collect(&format!("{query} limit {limit};"))
                .unwrap();
            assert_eq!(
                limited.rows,
                all.rows[..all.row_count.min(limit)],
                "{query} limit {limit}"
            );
        }
    }
}

#[test]
fn cached_sort_keys_preserve_exact_numbers_certainties_and_tokens() {
    for (values, expected) in [
        (
            [
                "9007199254740993",
                "-1.500",
                "+000",
                "9007199254740992.0",
                "0.00",
            ],
            [
                "-1.500",
                "+000",
                "0.00",
                "9007199254740992.0",
                "9007199254740993",
            ],
        ),
        (
            ["100%", "0%", "-100%", "50%", "-50%"],
            ["-100%", "-50%", "0%", "50%", "100%"],
        ),
    ] {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        let engine = Engine::new(&database);
        let mut script = String::from("add role value;");
        for (index, value) in values.iter().enumerate() {
            script.push_str(&format!(
                "add posit [{{(+x_{index}, value)}}, {value}, '2024'];"
            ));
        }
        engine.execute(&script).unwrap();
        for limit in 0..=5 {
            let result = engine
                .execute_collect(&format!(
                    "search [{{(*, value)}}, ?v, *] return ?v order by ?v limit {limit};"
                ))
                .unwrap();
            assert_eq!(
                result
                    .rows
                    .iter()
                    .map(|row| row[0].as_str())
                    .collect::<Vec<_>>(),
                expected[..limit]
            );
            assert_eq!(result.limited, limit < 5);
        }
    }
}

#[test]
fn late_comparison_and_sort_errors_are_never_hidden_by_limit_or_stopped_metadata() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            "add role value; add posit [{(+a, value)}, 1, '2024'], [{(+b, value)}, 100%, '2024'];",
        )
        .unwrap();
    for body in [
        "[{(?x, value)}, ?v, *] where ?v >= 0 return ?x",
        "[{(?x, value)}, ?v, *] return ?v order by ?v",
        "[{(?x, value)}, 1, *] return ?x",
        "[{(?x, value)}, ?v, *] not exists { [{(*, value)}, 1, *] } return ?x",
    ] {
        for limit in [0, 1] {
            let script = format!("search {body} limit {limit};");
            for stop in [false, true] {
                let mut events = Events {
                    stop,
                    ..Events::default()
                };
                assert!(
                    matches!(
                        engine.execute_stream_single(&script, &mut events),
                        Err(DatabaseError::Comparison(_))
                    ),
                    "{script}"
                );
                assert_eq!(events.metadata, 0, "{script}");
                assert!(events.rows.is_empty());
            }
        }
    }
}

#[test]
fn fallback_promotes_all_matches_despite_limits_and_sink_stop() {
    for limit in [0, 1, 3] {
        for stop in [false, true] {
            let database = fixture();
            let engine = Engine::new(&database);
            let script = format!(
                "search [{{(?x, left)}}, *, *] return ?x limit {limit} \
                 or add posit [{{(+x, left)}}, 0, '2024']; \
                 add posit [{{(x, copy)}}, 1, '2024'];"
            );
            let mut sink = Events {
                stop,
                ..Events::default()
            };
            engine.execute_stream_single(&script, &mut sink).unwrap();
            let copies = engine
                .execute_collect("search [{(?x, copy)}, *, *] return ?x;")
                .unwrap();
            assert_eq!(copies.row_count, 3, "limit {limit}, stop {stop}");
        }
    }
}

#[test]
fn explain_reports_early_limits_top_k_filters_and_validation_barriers() {
    let database = fixture();
    let engine = Engine::new(&database);
    let query = "search [{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] where ?v === 1 return ?v, ?w limit 1;";
    let plan = engine.explain(query).unwrap();
    assert!(plan.incremental);
    assert_eq!(plan.materialization_reason, None);
    assert!(matches!(
        plan.root.operation,
        PlanOperator::Limit {
            rows: 1,
            presentation_only: false
        }
    ));
    let serialized = serde_json::to_value(plan).unwrap();
    // The filter is attached to the left pattern, below final projection.
    assert_eq!(serialized["root"]["inputs"][0]["operator"], "project");
    assert!(serialized.to_string().contains("filter"));
    let sorted = engine
        .explain("search [{(?x, left)}, ?v, *] return ?v order by ?v limit 1;")
        .unwrap();
    assert!(matches!(
        sorted.root.inputs[0].operation,
        PlanOperator::TopK { rows: 2, .. }
    ));
    assert!(matches!(
        sorted.root.operation,
        PlanOperator::Limit {
            presentation_only: true,
            ..
        }
    ));
    for query in [
        "search [{(?x, left)}, ?v, *] where ?v = 1 return ?x limit 1;",
        "search [{(?x, left)}, 1, *] return ?x limit 1;",
        "search [{(?x, left)}, *, *] in effect @EOT, @EOT return ?x limit 1;",
        "search [{(?x, left)}, *, *] return ?x limit 1 or add posit [{(+x, left)}, 0, '2024'];",
    ] {
        let plan = engine.explain(query).unwrap();
        assert!(!plan.incremental, "{query}");
        assert!(plan.materialization_reason.is_some(), "{query}");
    }
}

#[test]
fn cancellation_after_a_streamed_row_stops_downstream_work() {
    struct CancelOnRow(CancellationToken, usize);
    impl RowSink for CancelOnRow {
        fn push(&mut self, _: Vec<ResultCell>) -> SinkFlow {
            self.1 += 1;
            self.0.cancel();
            SinkFlow::Continue
        }
    }
    let database = fixture();
    let token = CancellationToken::new();
    let mut sink = CancelOnRow(token.clone(), 0);
    let result = Engine::new(&database).execute_stream_single_with_options(
        "search [{(?x, left)}, ?v, *], [{(?y, right)}, ?w, *] return ?v, ?w;",
        &mut sink,
        ExecutionOptions {
            cancellation: Some(token),
            ..ExecutionOptions::default()
        },
    );
    assert!(matches!(result, Err(DatabaseError::Cancelled)));
    assert_eq!(sink.1, 1);
}
