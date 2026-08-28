use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::{Engine, ResultCell, ResultCellKind, RowSink, SinkFlow};

const INVALID_OR_ADD_RETURN: &str = r#"
    search [{(?registry, registry_code), ...}, ?code, *]
    return ?registry, ?code
    or add posit [{(+registry, registry_code)}, "CODE", @NOW];
"#;

fn registry_code_count(engine: &Engine<'_>) -> usize {
    engine
        .execute_collect("search [{(?registry, registry_code), ...}, *, *] return ?registry;")
        .unwrap()
        .row_count
}

fn prepare_registry_state(engine: &Engine<'_>, matched: bool) {
    engine.execute("add role registry_code;").unwrap();
    if matched {
        engine
            .execute("add posit [{(+existing, registry_code)}, \"CODE\", '2024-01-01'];")
            .unwrap();
    }
}

#[test]
fn and_assert_creates_and_binds_an_assertion_envelope() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let result = engine
        .execute_collect(
            r#"
            add role status, target_ref, evidence_ref, source_ref;
            add posit +target [
                {(+case, status)}, "open", '2024-01-01'
            ] and assert +evidence by +registry with 80% at '2024-02-01';

            add posit [{(target, target_ref)}, "target", @NOW],
                      [{(evidence, evidence_ref)}, "evidence", @NOW],
                      [{(registry, source_ref)}, "source", @NOW];

            search ?found = [{(?case, status)}, "open", '2024-01-01'],
                   ?assertion = [
                       {(?found, posit), (?source, ascertains)},
                       80%,
                       '2024-02-01'
                   ],
                   [{(?found, target_ref)}, "target", *],
                   [{(?assertion, evidence_ref)}, "evidence", *],
                   [{(?source, source_ref)}, "source", *]
            return ?found, ?assertion, ?source;
            "#,
        )
        .unwrap();

    assert_eq!(result.row_count, 1);
    assert_eq!(result.rows[0][0].kind, ResultCellKind::Posit);
    assert_eq!(result.rows[0][1].kind, ResultCellKind::Posit);
    assert_eq!(result.rows[0][2].kind, ResultCellKind::Thing);
}

#[test]
fn and_assert_reuses_the_canonical_target_posit() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let result = engine
        .execute_collect(
            r#"
            add role status, original_ref, same_ref;
            add posit +original [{(+case, status)}, "open", '2024-01-01'];
            add posit +same [{(case, status)}, "open", '2024-01-01']
                and assert by +source with 90% at '2024-02-01';

            add posit [{(original, original_ref)}, "original", @NOW],
                      [{(same, same_ref)}, "same", @NOW];

            search ?target = [{(?case, status)}, "open", '2024-01-01'],
                   [{(?target, posit), (?source, ascertains)}, 90%, '2024-02-01'],
                   [{(?target, original_ref)}, "original", *],
                   [{(?target, same_ref)}, "same", *]
            return ?target, ?source;
            "#,
        )
        .unwrap();

    assert_eq!(result.row_count, 1);
}

#[test]
fn search_or_add_resolves_the_same_identity_in_later_scripts() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add role registry_code, status;
            search [{(?registry, registry_code), ...}, "SE-CIVIL-REGISTRY", *]
            or add posit [{(+registry, registry_code)}, "SE-CIVIL-REGISTRY", @NOW];

            add posit [{(+first_case, status)}, "open", '2024-01-01']
                and assert by registry with 100% at '2024-02-01';
            "#,
        )
        .unwrap();

    // Mutation bindings are new for each execution. The second script must
    // recover the durable registry identity through its search branch.
    engine
        .execute(
            r#"
            search [{(?registry, registry_code), ...}, "SE-CIVIL-REGISTRY", *]
            or add posit [{(+registry, registry_code)}, "SE-CIVIL-REGISTRY", @NOW];

            add posit [{(+second_case, status)}, "closed", '2025-01-01']
                and assert by registry with 100% at '2025-02-01';
            "#,
        )
        .unwrap();

    let registries = engine
        .execute_collect(
            r#"
            search [{(?registry, registry_code), ...}, "SE-CIVIL-REGISTRY", *]
            return distinct ?registry;
            "#,
        )
        .unwrap();
    assert_eq!(registries.row_count, 1);

    let assertions = engine
        .execute_collect(
            r#"
            search ?target = [{(?case, status)}, ?status, *],
                   [{(?target, posit), (?source, ascertains)}, 100%, *]
            return ?case, ?status, ?source;
            "#,
        )
        .unwrap();
    assert_eq!(assertions.row_count, 2);
    assert_eq!(assertions.rows[0][2], assertions.rows[1][2]);
}

#[test]
fn search_or_add_promotes_every_matching_identity() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let result = engine
        .execute_collect(
            r#"
            add role registry_code, status;
            add posit [{(+first_registry, registry_code)}, "SHARED", '2020-01-01'],
                      [{(+second_registry, registry_code)}, "SHARED", '2021-01-01'];

            search [{(?registry, registry_code), ...}, "SHARED", *]
            or add posit [{(+registry, registry_code)}, "SHARED", @NOW];

            add posit [{(+case, status)}, "open", '2024-01-01']
                and assert by registry with 75% at '2024-02-01';

            search ?target = [{(?case, status)}, "open", '2024-01-01'],
                   [{(?target, posit), (?source, ascertains)}, 75%, '2024-02-01']
            return distinct ?source
            order by ?source;
            "#,
        )
        .unwrap();

    assert_eq!(result.row_count, 2);
}

#[test]
fn search_or_add_can_promote_a_posit_identity() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add role registry_code, reference;
            search ?code_posit = [
                {(?registry, registry_code)}, "CODE", '2020-01-01'
            ]
            or add posit +code_posit [
                {(+registry, registry_code)}, "CODE", '2020-01-01'
            ];
            add posit [{(code_posit, reference)}, "first", @NOW];
            "#,
        )
        .unwrap();
    engine
        .execute(
            r#"
            search ?code_posit = [
                {(?registry, registry_code)}, "CODE", '2020-01-01'
            ]
            or add posit +code_posit [
                {(+registry, registry_code)}, "CODE", '2020-01-01'
            ];
            add posit [{(code_posit, reference)}, "second", @NOW];
            "#,
        )
        .unwrap();

    let result = engine
        .execute_collect(
            r#"
            search [{(?code_posit, reference)}, "first", *],
                   [{(?code_posit, reference)}, "second", *]
            return distinct ?code_posit;
            "#,
        )
        .unwrap();
    assert_eq!(result.row_count, 1);
    assert_eq!(result.rows[0][0].kind, ResultCellKind::Thing);
}

#[test]
fn a_search_without_return_produces_no_result_set() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let results = engine
        .execute_collect_multi(
            r#"
            add role registry_code;
            search [{(?registry, registry_code), ...}, "CODE", *]
            or add posit [{(+registry, registry_code)}, "CODE", @NOW];
            "#,
        )
        .unwrap();

    assert!(results.is_empty());
}

#[test]
fn search_or_add_can_return_the_fallback_binding() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let result = engine
        .execute_collect(
            r#"
            add role registry_code;
            search [{(?registry, registry_code), ...}, "CODE", *]
            return ?registry
            or add posit [{(+registry, registry_code)}, "CODE", @NOW];
            "#,
        )
        .unwrap();

    assert_eq!(result.columns, vec!["registry"]);
    assert_eq!(result.row_count, 1);
    assert_eq!(result.rows[0][0].kind, ResultCellKind::Thing);
}

#[test]
fn or_add_fallback_can_add_and_assert_atomically() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let result = engine
        .execute_collect(
            r#"
            add role status;
            search ?target = [{(?case, status)}, "open", '2024-01-01']
            or add posit +target [{(+case, status)}, "open", '2024-01-01']
                and assert +evidence by +source with 85% at '2024-02-01';

            search ?target = [{(?case, status)}, "open", '2024-01-01'],
                   ?evidence = [
                       {(?target, posit), (?source, ascertains)},
                       85%,
                       '2024-02-01'
                   ]
            return ?target, ?evidence, ?source;
            "#,
        )
        .unwrap();
    assert_eq!(result.row_count, 1);
}

#[test]
fn concurrent_search_or_add_scripts_create_one_identity() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute("add role registry_code;")
        .unwrap();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..8 {
            handles.push(scope.spawn(|| {
                Engine::new(&database)
                    .execute(
                        r#"
                        search [{(?registry, registry_code), ...}, "CONCURRENT", *]
                        or add posit [{(+registry, registry_code)}, "CONCURRENT", @NOW];
                        "#,
                    )
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
    });

    let result = Engine::new(&database)
        .execute_collect(
            r#"
            search [{(?registry, registry_code), ...}, "CONCURRENT", *]
            return distinct ?registry;
            "#,
        )
        .unwrap();
    assert_eq!(result.row_count, 1);
}

#[test]
fn search_or_add_rejects_branch_domain_mismatches_before_adding() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine.execute("add role registry_code;").unwrap();
    let error = engine
        .execute(
            r#"
            search ?registry = [
                {(?thing, registry_code)}, "CODE", *
            ]
            or add posit [{(+registry, registry_code)}, "CODE", @NOW];
            "#,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("incompatible domains"),
        "{error}"
    );

    let result = engine
        .execute_collect("search [{(?thing, registry_code), ...}, \"CODE\", *] return ?thing;")
        .unwrap();
    assert_eq!(result.row_count, 0);
}

#[test]
fn invalid_or_add_return_shape_is_data_independent_and_never_mutates() {
    for matched in [false, true] {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        let engine = Engine::new(&database);
        prepare_registry_state(&engine, matched);
        let before = registry_code_count(&engine);

        let error = engine.execute_collect(INVALID_OR_ADD_RETURN).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("fallback cannot supply returned variable '?code'"),
            "{error}"
        );
        assert_eq!(registry_code_count(&engine), before);
    }
}

#[test]
fn invalid_or_add_stream_emits_no_metadata_or_rows_in_either_data_state() {
    #[derive(Default)]
    struct RecordingSink {
        metadata_calls: usize,
        rows: usize,
    }

    impl RowSink for RecordingSink {
        fn on_meta(&mut self, _columns: &[String]) -> SinkFlow {
            self.metadata_calls += 1;
            SinkFlow::Continue
        }

        fn push(&mut self, _row: Vec<ResultCell>) -> SinkFlow {
            self.rows += 1;
            SinkFlow::Continue
        }
    }

    for matched in [false, true] {
        let database = Database::new(PersistenceMode::InMemory).unwrap();
        let engine = Engine::new(&database);
        prepare_registry_state(&engine, matched);
        let before = registry_code_count(&engine);
        let mut sink = RecordingSink::default();

        let error = engine
            .execute_stream_single(INVALID_OR_ADD_RETURN, &mut sink)
            .unwrap_err();
        assert!(error.to_string().contains("fallback cannot supply"));
        assert_eq!(sink.metadata_calls, 0);
        assert_eq!(sink.rows, 0);
        assert_eq!(registry_code_count(&engine), before);
    }
}

#[test]
fn valid_or_add_fallback_honors_modifiers_and_metadata_stop() {
    struct StopAtMetadata {
        metadata_calls: usize,
        rows: usize,
    }

    impl RowSink for StopAtMetadata {
        fn on_meta(&mut self, columns: &[String]) -> SinkFlow {
            self.metadata_calls += 1;
            assert_eq!(columns, ["registry"]);
            SinkFlow::Stop
        }

        fn push(&mut self, _row: Vec<ResultCell>) -> SinkFlow {
            self.rows += 1;
            SinkFlow::Continue
        }
    }

    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    prepare_registry_state(&engine, false);
    let mut sink = StopAtMetadata {
        metadata_calls: 0,
        rows: 0,
    };
    let (columns, limited, row_count) = engine
        .execute_stream_single(
            r#"
            search [{(?registry, registry_code), ...}, "CODE", *]
            return distinct ?registry
            order by ?registry
            limit 1
            or add posit [{(+registry, registry_code)}, "CODE", @NOW];
            "#,
            &mut sink,
        )
        .unwrap();

    assert_eq!(columns, ["registry"]);
    assert!(!limited);
    assert_eq!(row_count, 0);
    assert_eq!(sink.metadata_calls, 1);
    assert_eq!(sink.rows, 0);
    assert_eq!(registry_code_count(&engine), 1);
}

#[test]
fn a_failed_assertion_source_leaves_the_target_unpublished() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine.execute("add role status;").unwrap();

    let error = engine
        .execute(
            r#"
            add posit [{(+case, status)}, "open", @NOW]
                and assert by missing_source with 100% at @NOW;
            "#,
        )
        .unwrap_err();
    assert!(error.to_string().contains("missing_source"), "{error}");

    let result = engine
        .execute_collect("search [{(?case, status), ...}, \"open\", *] return ?case;")
        .unwrap();
    assert_eq!(result.row_count, 0);
}
