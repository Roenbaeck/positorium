use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, PersistenceMode, QueryLimits, ResultCell,
    RowSink, SinkFlow,
};

fn fixture(rows: usize) -> Database {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from("add role value, other, absent; add posit ");
    for i in 0..rows {
        if i != 0 {
            script.push(',');
        }
        script.push_str(&format!(
            "[{{(+x_{i}, value)}}, {i}, '2024'],[{{(x_{i}, other)}}, {i}, '2024']"
        ));
    }
    script.push(';');
    Engine::new(&db).execute(&script).unwrap();
    db
}

fn options(work: Option<u64>, memory: Option<u64>) -> ExecutionOptions {
    ExecutionOptions {
        query_limits: QueryLimits {
            max_work_units: work,
            max_memory_bytes: memory,
        },
        ..ExecutionOptions::default()
    }
}

fn resource(error: DatabaseError, expected: &str, cap: u64) {
    match error {
        DatabaseError::ResourceLimit { resource, limit } => {
            assert_eq!(resource, expected);
            assert_eq!(limit, cap);
        }
        other => panic!("expected {expected}, got {other}"),
    }
}

fn minimum_memory(engine: &Engine<'_>, script: &str) -> u64 {
    let mut low = 0;
    let mut high = 1_048_576;
    engine
        .execute_collect_multi_with_options(script, options(None, Some(high)))
        .unwrap();
    while low < high {
        let middle = low + (high - low) / 2;
        match engine.execute_collect_multi_with_options(script, options(None, Some(middle))) {
            Ok(_) => high = middle,
            Err(DatabaseError::ResourceLimit {
                resource: "query memory bytes",
                ..
            }) => low = middle + 1,
            Err(error) => panic!("{script}: {error}"),
        }
    }
    low
}

#[test]
fn work_caps_bound_streaming_joins_snapshots_sorts_and_fallible_queries() {
    let db = fixture(64);
    let engine = Engine::new(&db);
    let limited = "search [{(*, value)}, ?v, *] return ?v limit 1;";
    let result = engine
        .execute_collect_with_options(limited, options(Some(150), None))
        .unwrap();
    assert_eq!(result.row_count, 1);
    assert!(result.limited);
    for script in [
        "search [{(*, value)}, ?v, *] return ?v;",
        "search [{(*, value)}, ?v, *], [{(*, other)}, ?w, *] return ?v, ?w;",
        "search [{(*, value)}, ?v, *] return ?v order by ?v limit 1;",
        "search [{(*, value)}, ?v, *] as of @EOT return ?v limit 1;",
        "search [{(*, value)}, ?v, *] where ?v >= 0 return ?v limit 1;",
        "search [{(*, value)}, ?v, *] not exists { [{(*, other)}, *, *] } return ?v;",
    ] {
        resource(
            engine
                .execute_collect_with_options(script, options(Some(150), None))
                .unwrap_err(),
            "query work units",
            150,
        );
    }
    for cap in [0, 1, u64::MAX] {
        let result = engine.execute_collect_with_options(limited, options(Some(cap), None));
        if cap < 2 {
            resource(result.unwrap_err(), "query work units", cap);
        } else {
            assert!(result.is_ok());
        }
    }
}

#[test]
fn live_workspace_is_released_between_rows_and_searches() {
    let db = fixture(48);
    let engine = Engine::new(&db);
    let scan = "search [{(*, value)}, ?v, *] return ?v;";
    let memory = minimum_memory(&engine, scan);
    assert!(memory > 0);
    engine
        .execute_collect_with_options(scan, options(None, Some(memory)))
        .unwrap();
    resource(
        engine
            .execute_collect_with_options(scan, options(None, Some(memory - 1)))
            .unwrap_err(),
        "query memory bytes",
        memory - 1,
    );
    let twice = engine
        .execute_collect_multi_with_options(&format!("{scan} {scan}"), options(None, Some(memory)))
        .unwrap();
    assert_eq!(twice.len(), 2);
    assert_eq!(twice[0].row_count, 48);
    assert_eq!(twice[1].row_count, 48);
    // Caller-owned collected results are deliberately outside workspace limits.
    assert_eq!(minimum_memory(&engine, &format!("{scan} {scan}")), memory);
}

#[test]
fn top_k_fits_below_full_sort_and_distinct_retains_accounted_state() {
    let db = fixture(48);
    let engine = Engine::new(&db);
    let top_k = "search [{(*, value)}, ?v, *] return ?v order by ?v desc limit 2;";
    let full = "search [{(*, value)}, ?v, *] return ?v order by ?v desc;";
    let distinct = "search [{(*, value)}, ?v, *] return distinct ?v;";
    let scan = "search [{(*, value)}, ?v, *] return ?v;";
    let cap = minimum_memory(&engine, top_k);
    assert!(minimum_memory(&engine, full) > cap);
    assert!(minimum_memory(&engine, distinct) > minimum_memory(&engine, scan));
    let top = engine
        .execute_collect_with_options(top_k, options(None, Some(cap)))
        .unwrap();
    assert_eq!(top.rows[0][0].as_str(), "47");
    assert!(top.limited);
    resource(
        engine
            .execute_collect_with_options(full, options(None, Some(cap)))
            .unwrap_err(),
        "query memory bytes",
        cap,
    );
}

#[test]
fn full_scans_temporal_scratch_and_materialized_relations_are_accounted() {
    let db = fixture(24);
    let engine = Engine::new(&db);
    for script in [
        "search ?p = [*, *, *] return ?p;",
        "search [{(?x, value)}, ?v, *] as of @EOT return ?v;",
        "search [{(*, value)}, ?v, *], [{(*, other)}, ?w, *] where ?v >= 0 return ?v, ?w limit 1;",
    ] {
        let memory = minimum_memory(&engine, script);
        let capped = engine
            .execute_collect_with_options(script, options(None, Some(memory)))
            .unwrap();
        assert_eq!(capped.rows, engine.execute_collect(script).unwrap().rows);
        resource(
            engine
                .execute_collect_with_options(script, options(None, Some(memory - 1)))
                .unwrap_err(),
            "query memory bytes",
            memory - 1,
        );
    }
}

#[test]
fn cumulative_work_failure_can_end_a_stream_without_a_successful_tail() {
    #[derive(Default)]
    struct Rows(usize);
    impl RowSink for Rows {
        fn push(&mut self, _: Vec<ResultCell>) -> SinkFlow {
            self.0 += 1;
            SinkFlow::Continue
        }
    }
    let db = fixture(64);
    let engine = Engine::new(&db);
    let mut sink = Rows::default();
    let result = engine.execute_stream_single_with_options(
        "search [{(*, value)}, ?v, *] return ?v;",
        &mut sink,
        options(Some(150), None),
    );
    resource(result.unwrap_err(), "query work units", 150);
    assert!(sink.0 > 0 && sink.0 < 64);
    let one = "search [{(*, value)}, ?v, *] return ?v limit 1;";
    engine
        .execute_collect_with_options(one, options(Some(150), None))
        .unwrap();
    resource(
        engine
            .execute_collect_multi_with_options(&format!("{one} {one}"), options(Some(150), None))
            .unwrap_err(),
        "query work units",
        150,
    );
}

#[test]
fn failed_searches_never_trigger_fallback_or_following_mutations() {
    for limits in [options(Some(0), None), options(None, Some(1))] {
        let db = fixture(4);
        let engine = Engine::new(&db);
        let script = "search [{(?x, absent)}, *, *] return ?x limit 0 \
            or add posit [{(+x, absent)}, 1, '2024']; \
            add posit [{(+later, absent)}, 2, '2024'];";
        assert!(matches!(
            engine.execute_collect_with_options(script, limits),
            Err(DatabaseError::ResourceLimit { .. })
        ));
        assert_eq!(
            engine
                .execute_collect("search [{(*, absent)}, ?v, *] return ?v;")
                .unwrap()
                .row_count,
            0
        );
    }
}

#[test]
fn budgets_exclude_explain_and_mutations_without_rolling_back_prior_commands() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let zero = options(Some(0), Some(0));
    let result = engine.execute_collect_with_options(
        "add role value; add posit [{(+x, value)}, 1, '2024']; \
        search [{(*, value)}, ?v, *] return ?v; \
        add posit [{(+y, value)}, 2, '2024'];",
        zero.clone(),
    );
    resource(result.unwrap_err(), "query work units", 0);
    assert_eq!(
        engine
            .execute_collect("search [{(*, value)}, ?v, *] return ?v;")
            .unwrap()
            .row_count,
        1,
    );
    assert_eq!(
        engine
            .execute_collect_with_options("explain search [{(*, value)}, ?v, *] return ?v;", zero,)
            .unwrap()
            .row_count,
        1,
    );
}

#[test]
fn effective_resolution_and_cached_distinct_cuts_share_the_budget() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    engine
        .execute(
            "add role value, cut; add posit +target [{(+x, value)}, 1, '2020']; \
        add posit [{(target, posit), (+source, ascertains)}, 100%, '2021']; \
        add posit [{(+a, cut)}, 1, '2022'], [{(+b, cut)}, 1, '2023'], [{(+c, cut)}, 1, '2024'];",
        )
        .unwrap();
    for script in [
        "search [{(*, value)}, ?v, *] in effect @EOT, @EOT return ?v;",
        "search [{(*, cut)}, *, ?cut], [{(*, value)}, ?v, *] in effect ?cut, ?cut return ?v;",
    ] {
        let cap = minimum_memory(&engine, script);
        engine
            .execute_collect_with_options(script, options(None, Some(cap)))
            .unwrap();
        resource(
            engine
                .execute_collect_with_options(script, options(None, Some(cap - 1)))
                .unwrap_err(),
            "query memory bytes",
            cap - 1,
        );
        resource(
            engine
                .execute_collect_with_options(script, options(Some(1), None))
                .unwrap_err(),
            "query work units",
            1,
        );
    }
}
