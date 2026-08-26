use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, ExecutionParameter, PersistenceMode,
    ResultCellKind,
};
use std::collections::HashMap;

fn engine() -> Engine<'static> {
    Engine::new(Box::leak(Box::new(
        Database::new(PersistenceMode::InMemory).unwrap(),
    )))
}

#[test]
fn exact_open_role_and_complete_set_bindings_are_distinct() {
    let engine = engine();
    engine
        .execute(
            r#"
            add role name, tag;
            add posit [{(+plain, name)}, "plain", '2024-01-01'];
            add posit [{(+tagged, name), (+tag, tag)}, "tagged", '2024-01-01'];
            "#,
        )
        .unwrap();

    let exact = engine
        .execute_collect("search [{(?thing, name)}, ?value, *] return ?value;")
        .unwrap();
    assert_eq!(exact.rows, vec![vec!["\"plain\"".to_string()]]);

    let open = engine
        .execute_collect(
            "search ?p = [?aset = {(?thing, ?r), ...}, ?value, ?time] return ?p, ?aset, ?r, ?value order by ?p, ?r;",
        )
        .unwrap();
    assert_eq!(open.rows.len(), 3);
    assert_eq!(open.rows[0][0].kind, ResultCellKind::Posit);
    assert_eq!(open.rows[0][1].kind, ResultCellKind::AppearanceSet);
    assert_eq!(open.rows[0][2].kind, ResultCellKind::Role);
    assert_eq!(open.rows[0][3].kind, ResultCellKind::Literal);
}

#[test]
fn incompatible_variable_domains_are_rejected_before_evaluation() {
    let engine = engine();
    engine
        .execute("add role name; add posit [{(+person, name)}, \"Alice\", @NOW];")
        .unwrap();
    let error = engine
        .execute_collect("search [{(?same, name)}, ?same, *] return ?same;")
        .unwrap_err();
    assert!(matches!(error, DatabaseError::VariableDomain { name, .. } if name == "same"));
}

#[test]
fn quoted_roles_are_literal_and_unquoted_namespace_syntax_is_reserved() {
    let engine = engine();
    engine
        .execute(
            "add role `postal code`, `role::literal`; add posit [{(+place, `postal code`)}, \"11335\", @NOW];",
        )
        .unwrap();
    let result = engine
        .execute_collect("search [{(?place, `postal code`)}, ?code, *] return ?code;")
        .unwrap();
    assert_eq!(result.rows, vec![vec!["\"11335\"".to_string()]]);

    let error = engine.execute("add role future::namespace;").unwrap_err();
    assert!(matches!(error, DatabaseError::Parse { .. }));
}

#[test]
fn parameters_are_typed_values_and_never_source_text() {
    let engine = engine();
    engine
        .execute("add role amount; add posit [{(+item, amount)}, +0010.00, '2024-01-01'];")
        .unwrap();
    let parameters = HashMap::from([
        (
            "target".to_string(),
            ExecutionParameter::Literal {
                text: "10".to_string(),
            },
        ),
        (
            "cutoff".to_string(),
            ExecutionParameter::Time {
                text: "'2024-12-31'".to_string(),
            },
        ),
    ]);
    let result = engine
        .execute_collect_with_options(
            "search [{(?item, amount)}, ?amount, *] as of $cutoff where ?amount = $target return ?amount;",
            ExecutionOptions {
                parameters,
                ..ExecutionOptions::default()
            },
        )
        .unwrap();
    assert_eq!(result.rows, vec![vec!["+0010.00".to_string()]]);

    let missing = engine
        .execute_collect(
            "search [{(?item, amount)}, ?amount, *] where ?amount = $missing return ?amount;",
        )
        .unwrap_err();
    assert!(matches!(missing, DatabaseError::Parameter(_)));

    let source_substitution = engine.execute("add role $role;").unwrap_err();
    assert!(matches!(source_substitution, DatabaseError::Parse { .. }));
}

#[test]
fn natural_join_meaning_is_independent_of_pattern_order() {
    let engine = engine();
    engine
        .execute(
            r#"
            add role name, age;
            add posit [{(+alice, name)}, "Alice", @NOW],
                      [{(alice, age)}, 30, @NOW],
                      [{(+bob, name)}, "Bob", @NOW],
                      [{(bob, age)}, 40, @NOW];
            "#,
        )
        .unwrap();
    let forward = engine
        .execute_collect(
            "search [{(?person, name)}, ?name, *], [{(?person, age)}, ?age, *] return ?person, ?name, ?age order by ?person;",
        )
        .unwrap();
    let reversed = engine
        .execute_collect(
            "search [{(?person, age)}, ?age, *], [{(?person, name)}, ?name, *] return ?person, ?name, ?age order by ?person;",
        )
        .unwrap();
    assert_eq!(forward.rows, reversed.rows);
    assert_eq!(forward.rows.len(), 2);
}

#[test]
fn bag_distinct_union_order_and_limit_follow_the_pipeline() {
    let engine = engine();
    engine
        .execute(
            "add role number; add posit [{(+two, number)}, 2, @NOW], [{(+ten, number)}, 10, @NOW];",
        )
        .unwrap();

    let bag = engine
        .execute_collect(
            "search [{(*, number)}, ?left, *], [{(*, number)}, ?right, *] return ?left order by ?left;",
        )
        .unwrap();
    assert_eq!(bag.rows.len(), 4);
    assert_eq!(bag.rows[0][0], "2");
    assert_eq!(bag.rows[1][0], "2");

    let distinct = engine
        .execute_collect(
            "search [{(*, number)}, ?left, *], [{(*, number)}, ?right, *] return distinct ?left order by ?left desc limit 1;",
        )
        .unwrap();
    assert_eq!(distinct.rows, vec![vec!["10".to_string()]]);
    assert!(distinct.limited);

    let union = engine
        .execute_collect(
            "search [{(?number, number)}, 2, *] union [{(?number, number)}, 10, *] return ?number order by ?number;",
        )
        .unwrap();
    assert_eq!(union.rows.len(), 2);
}

#[test]
fn not_exists_is_a_correlated_absence_query() {
    let engine = engine();
    engine
        .execute(
            r#"
            add role name, age;
            add posit [{(+alice, name)}, "Alice", @NOW],
                      [{(alice, age)}, 30, @NOW],
                      [{(+bob, name)}, "Bob", @NOW];
            "#,
        )
        .unwrap();
    let result = engine
        .execute_collect(
            "search [{(?person, name)}, ?name, *] not exists { [{(?person, age)}, *, *] } return ?name;",
        )
        .unwrap();
    assert_eq!(result.rows, vec![vec!["\"Bob\"".to_string()]]);
}

#[test]
fn ordinary_and_latest_matching_as_of_have_different_filter_order() {
    let engine = engine();
    engine
        .execute(
            r#"
            add role status, marker;
            add posit [{(+case, status)}, "wanted", '2020-01-01'],
                      [{(case, status)}, "closed", '2021-01-01'],
                      [{(+clock, marker)}, "cutoff", '2020-12-31'];
            "#,
        )
        .unwrap();

    let ordinary = engine
        .execute_collect(
            "search [{(?case, status)}, \"wanted\", ?time] as of '2022-01-01' return ?time;",
        )
        .unwrap();
    assert!(ordinary.rows.is_empty());

    let latest_matching = engine
        .execute_collect(
            "search latest [{(?case, status)}, \"wanted\", ?time] as of '2022-01-01' return ?time;",
        )
        .unwrap();
    assert_eq!(latest_matching.rows, vec![vec!["2020-01-01".to_string()]]);

    let variable_cutoff = engine
        .execute_collect(
            "search [{(?case, status)}, ?value, ?time] as of ?cutoff, [{(*, marker)}, *, ?cutoff] return ?value, ?time;",
        )
        .unwrap();
    assert_eq!(
        variable_cutoff.rows,
        vec![vec!["\"wanted\"".to_string(), "2020-01-01".to_string()]]
    );

    let shorthand = engine
        .execute_collect(
            "search [{(?case, status)}, ?value, ?time] as of '2022-01-01' return ?value, ?time order by ?time;",
        )
        .unwrap();
    let filter_first_expansion = engine
        .execute_collect(
            "search latest [{(?case, status)}, ?value, ?time] as of '2022-01-01' return ?value, ?time order by ?time;",
        )
        .unwrap();
    assert_eq!(shorthand.rows, filter_first_expansion.rows);
}
