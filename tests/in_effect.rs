use positorium::{
    CancellationToken, Database, DatabaseError, EffectCut, EffectLimits, Engine, ExecutionOptions,
    PersistenceMode, ResultCellKind,
};

fn date(value: &str) -> positorium::datatype::Time {
    positorium::datatype::Time::new_date_from(value).unwrap()
}

fn cut(assertion: &str, appearance: &str) -> EffectCut {
    EffectCut::new(date(assertion), date(appearance))
}

#[test]
fn resolver_applies_retractions_and_both_grouped_maxima() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            r#"
            add role status;
            add posit +old [{(+case, status)}, "open", '2020-01-01'];
            add posit [{(old, posit), (+source, ascertains)}, 80%, '2020-02-01'];
            add posit +new [{(case, status)}, "closed", '2021-01-01'];
            add posit [{(new, posit), (source, ascertains)}, 70%, '2021-02-01'];
            add posit [{(new, posit), (source, ascertains)}, 90%, '2021-03-01'];
            add posit [{(new, posit), (source, ascertains)}, 0%, '2021-04-01'];
            add posit [{(old, posit), (source, ascertains)}, 0%, '2022-01-01'];
            "#,
        )
        .unwrap();

    let before_retraction = database
        .information_in_effect(cut("2021-03-15", "2021-12-31"))
        .unwrap();
    assert_eq!(before_retraction.assertions().len(), 1);
    assert_eq!(
        before_retraction.assertions()[0].target().value().token(),
        "\"closed\""
    );
    assert_eq!(before_retraction.assertions()[0].certainty_percent(), 90);

    let after_new_retraction = database
        .information_in_effect(cut("2021-12-31", "2021-12-31"))
        .unwrap();
    assert_eq!(after_new_retraction.assertions().len(), 1);
    assert_eq!(
        after_new_retraction.assertions()[0]
            .target()
            .value()
            .token(),
        "\"open\""
    );

    let after_both_retractions = database
        .information_in_effect(cut("2022-12-31", "2022-12-31"))
        .unwrap();
    assert!(after_both_retractions.assertions().is_empty());
    assert_eq!(after_both_retractions.counters().retractions, 2);
}

#[test]
fn resolver_retains_incomparable_target_times_and_separate_sources() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            r#"
            add role status;
            add posit +broad [{(+case, status)}, "broad", '2024'];
            add posit +precise [{(case, status)}, "precise", '2024-05-06'];
            add posit [{(broad, posit), (+source_a, ascertains)}, 60%, '2024-06-01'];
            add posit [{(precise, posit), (source_a, ascertains)}, 70%, '2024-06-01'];
            add posit [{(precise, posit), (+source_b, ascertains)}, 80%, '2024-06-01'];
            "#,
        )
        .unwrap();

    let slice = database
        .information_in_effect(cut("2025-01-01", "2025-01-01"))
        .unwrap();
    let mut values = slice
        .assertions()
        .iter()
        .map(|assertion| assertion.target().value().token().to_string())
        .collect::<Vec<_>>();
    values.sort();
    assert_eq!(values, ["\"broad\"", "\"precise\"", "\"precise\""]);
}

#[test]
fn equal_time_alternatives_survive_regardless_of_certainty() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&database)
        .execute(
            r#"
            add role status;
            add posit +left [{(+case, status)}, "left", '2024-01-01'];
            add posit +right [{(case, status)}, "right", '2024-01-01'];
            add posit [{(left, posit), (+source, ascertains)}, 10%, '2024-02-01'];
            add posit [{(right, posit), (source, ascertains)}, 90%, '2024-02-01'];
            "#,
        )
        .unwrap();

    let slice = database
        .information_in_effect(cut("2025-01-01", "2025-01-01"))
        .unwrap();
    let mut values = slice
        .assertions()
        .iter()
        .map(|assertion| assertion.target().value().token().to_string())
        .collect::<Vec<_>>();
    values.sort();
    assert_eq!(values, ["\"left\"", "\"right\""]);
}

#[test]
fn exact_assertion_shape_is_validated_but_larger_shapes_remain_ordinary_data() {
    let malformed = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&malformed)
        .execute("add posit [{(+missing, posit), (+source, ascertains)}, 1, '2024-01-01'];")
        .unwrap();
    let error = malformed
        .information_in_effect(cut("2025-01-01", "2025-01-01"))
        .unwrap_err();
    assert!(error.to_string().contains("require a certainty literal"));

    let ordinary = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&ordinary)
        .execute(
            "add role note; add posit [{(+missing, posit), (+source, ascertains), (+extra, note)}, 1, '2024-01-01'];",
        )
        .unwrap();
    assert!(
        ordinary
            .information_in_effect(cut("2025-01-01", "2025-01-01"))
            .unwrap()
            .assertions()
            .is_empty()
    );

    let dangling = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&dangling)
        .execute("add posit [{(+missing, posit), (+source, ascertains)}, 100%, '2024-01-01'];")
        .unwrap();
    let error = dangling
        .information_in_effect(cut("2025-01-01", "2025-01-01"))
        .unwrap_err();
    assert!(error.to_string().contains("target posit"));
}

#[test]
fn target_matching_happens_after_resolution_and_via_exposes_provenance() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add role status;
            add posit +old [{(+case, status)}, "wanted", '2020-01-01'];
            add posit [{(old, posit), (+source, ascertains)}, 80%, '2020-02-01'];
            add posit +new [{(case, status)}, "closed", '2021-01-01'];
            add posit [{(new, posit), (source, ascertains)}, 90%, '2021-02-01'];
            "#,
        )
        .unwrap();

    let filtered = engine
        .execute_collect(
            "search [{(?case, status)}, \"wanted\", ?appeared] in effect '2022-01-01', '2022-01-01' return ?case, ?appeared;",
        )
        .unwrap();
    assert!(filtered.rows.is_empty());

    let result = engine
        .execute_collect(
            r#"
            search ?claim = [{(?case, status)}, ?state, ?appeared]
                in effect '2022-01-01', '2022-01-01'
                via ?assertion = [
                    {(?claim, posit), (?source, ascertains)},
                    ?certainty,
                    ?asserted
                ]
            return ?claim, ?case, ?state, ?appeared,
                   ?assertion, ?source, ?certainty, ?asserted;
            "#,
        )
        .unwrap();
    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0].kind, ResultCellKind::Posit);
    assert_eq!(result.rows[0][2].text, "\"closed\"");
    assert_eq!(result.rows[0][3].text, "2021-01-01");
    assert_eq!(result.rows[0][4].kind, ResultCellKind::Posit);
    assert_eq!(result.rows[0][6].text, "90%");
    assert_eq!(result.rows[0][7].text, "2021-02-01");

    let member_roles = engine
        .execute_collect(
            r#"
            search ?classification = [
              {(?member, status)}, ?state, ?appeared
            ] in effect '2022-01-01', '2022-01-01'
              via [
                {(?classification, posit), (?source, ascertains)}, ?certainty, *
              ],
              [{(?member, ?member_role)}, *, *] as of '2022-01-01'
            return ?member, ?state, ?source, ?certainty, ?member_role;
            "#,
        )
        .unwrap();
    assert_eq!(member_roles.rows.len(), 1);
    assert_eq!(member_roles.rows[0][1].text, "\"closed\"");
    assert_eq!(member_roles.rows[0][4].text, "status");
}

#[test]
fn in_effect_preserves_source_local_bag_rows_and_accepts_variable_cuts() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add role status, marker;
            add posit +target [{(+case, status)}, "open", '2020-01-01'];
            add posit [{(target, posit), (+source_a, ascertains)}, 80%, '2020-02-01'];
            add posit [{(target, posit), (+source_b, ascertains)}, 90%, '2020-03-01'];
            add posit [{(+clock, marker)}, "cut", '2021-01-01'];
            "#,
        )
        .unwrap();

    let bag = engine
        .execute_collect(
            "search [{(*, marker)}, *, ?cut], [{(?case, status)}, ?state, *] in effect ?cut, ?cut return ?state;",
        )
        .unwrap();
    assert_eq!(bag.rows.len(), 2);
    assert_eq!(bag.rows[0][0].text, "\"open\"");
    assert_eq!(bag.rows[1][0].text, "\"open\"");

    let distinct = engine
        .execute_collect(
            "search [{(*, marker)}, *, ?cut], [{(?case, status)}, ?state, *] in effect ?cut, ?cut return distinct ?state;",
        )
        .unwrap();
    assert_eq!(distinct.rows.len(), 1);
}

#[test]
fn in_effect_limits_and_cancellation_fail_closed() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            "add role status; add posit +target [{(+case, status)}, \"open\", '2020-01-01']; add posit [{(target, posit), (+source, ascertains)}, 80%, '2020-02-01'];",
        )
        .unwrap();

    let error = database
        .information_in_effect_with_limits(
            cut("2021-01-01", "2021-01-01"),
            EffectLimits {
                assertion_candidates: 0,
                temporal_comparisons: 0,
            },
        )
        .unwrap_err();
    assert!(matches!(
        error,
        DatabaseError::ResourceLimit {
            resource: "effective assertion candidates",
            limit: 0
        }
    ));

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = engine
        .execute_collect_with_options(
            "search [{(?case, status)}, ?state, *] in effect '2021-01-01', '2021-01-01' return ?state;",
            ExecutionOptions {
                cancellation: Some(cancellation),
                ..ExecutionOptions::default()
            },
        )
        .unwrap_err();
    assert!(matches!(error, DatabaseError::Cancelled));
}

#[test]
fn shared_via_source_correlates_effective_patterns_explicitly() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add role status, note;
            add posit +status_fact [{(+case, status)}, "open", '2020-01-01'];
            add posit +note_fact [{(case, note)}, "reviewed", '2020-01-01'];
            add posit [{(status_fact, posit), (+source_a, ascertains)}, 80%, '2020-02-01'];
            add posit [{(status_fact, posit), (+source_b, ascertains)}, 70%, '2020-02-01'];
            add posit [{(note_fact, posit), (source_a, ascertains)}, 90%, '2020-02-01'];
            "#,
        )
        .unwrap();

    let independent = engine
        .execute_collect(
            r#"
            search [{(?case, status)}, ?status, *] in effect '2021-01-01', '2021-01-01',
                   [{(?case, note)}, ?note, *] in effect '2021-01-01', '2021-01-01'
            return ?case, ?status, ?note;
            "#,
        )
        .unwrap();
    assert_eq!(independent.rows.len(), 2);

    let correlated = engine
        .execute_collect(
            r#"
            search ?status_fact = [{(?case, status)}, ?status, *]
                     in effect '2021-01-01', '2021-01-01'
                     via [{(?status_fact, posit), (?source, ascertains)}, *, *],
                   ?note_fact = [{(?case, note)}, ?note, *]
                     in effect '2021-01-01', '2021-01-01'
                     via [{(?note_fact, posit), (?source, ascertains)}, *, *]
            return ?case, ?status, ?note, ?source;
            "#,
        )
        .unwrap();
    assert_eq!(correlated.rows.len(), 1);
}

#[test]
fn in_effect_patterns_work_inside_correlated_not_exists() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add role status, note;
            add posit +status_fact [{(+case, status)}, "open", '2020-01-01'];
            add posit [{(status_fact, posit), (+source, ascertains)}, 80%, '2020-02-01'];
            "#,
        )
        .unwrap();

    let absent_note = engine
        .execute_collect(
            r#"
            search [{(?case, status)}, ?status, *]
                     in effect '2021-01-01', '2021-01-01'
            not exists {
              [{(?case, note)}, *, *] in effect '2021-01-01', '2021-01-01'
            }
            return ?case, ?status;
            "#,
        )
        .unwrap();
    assert_eq!(absent_note.rows.len(), 1);

    let existing_status = engine
        .execute_collect(
            r#"
            search [{(?case, status)}, ?status, *]
                     in effect '2021-01-01', '2021-01-01'
            not exists {
              [{(?case, status)}, *, *] in effect '2021-01-01', '2021-01-01'
            }
            return ?case, ?status;
            "#,
        )
        .unwrap();
    assert!(existing_status.rows.is_empty());
}

#[test]
fn terrain_fractional_now_cutoffs_are_valid_in_effect_operands() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let result = Engine::new(&database)
        .execute_collect(
            "search [{(?class, class)}, *, *] in effect '2026-08-28 12:18:20.001135', '2026-08-28 12:18:20.001135' return ?class;",
        )
        .unwrap();
    assert!(result.rows.is_empty());
}

#[test]
fn independent_assertions_do_not_consume_cross_group_comparisons() {
    use std::fmt::Write;

    const ASSERTIONS: usize = 500;
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let mut script = String::from("add role status;");
    for index in 0..ASSERTIONS {
        write!(
            script,
            "add posit +target_{index} [{{(+case_{index}, status)}}, {index}, '2020-01-01'];\
             add posit [{{(target_{index}, posit), (+source_{index}, ascertains)}}, 100%, '2020-02-01'];"
        )
        .unwrap();
    }
    Engine::new(&database).execute(&script).unwrap();

    let slice = database
        .information_in_effect_with_limits(
            cut("2021-01-01", "2021-01-01"),
            EffectLimits {
                assertion_candidates: ASSERTIONS as u64,
                temporal_comparisons: 0,
            },
        )
        .unwrap();
    assert_eq!(slice.assertions().len(), ASSERTIONS);
    assert_eq!(slice.counters().temporal_comparisons, 0);
}
