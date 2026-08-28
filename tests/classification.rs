use positorium::{Database, Engine, PersistenceMode};

#[test]
fn classification_shapes_round_trip_opaque_values() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let result = Engine::new(&database)
        .execute_collect(
            r#"
            add posit +person_decl [{(+person, class)}, {"declared":true}, '2024-01-01'];
            add posit +membership [{(+ada, thing), (person, class)}, "active", '2024-02-01'];
            add posit +subclass_fact [{(+engineer, subclass), (person, class)}, 17, '2024-03-01'];

            search ?record = [{(?class, class)}, ?value, ?time]
            union ?record = [{(?member, thing), (?class, class)}, ?value, ?time]
            union ?record = [{(?child, subclass), (?class, class)}, ?value, ?time]
            return ?record, ?class, ?value, ?time order by ?record;
            "#,
        )
        .unwrap();

    assert_eq!(result.rows.len(), 3);
    let values = result
        .rows
        .iter()
        .map(|row| row[2].text.as_str())
        .collect::<Vec<_>>();
    assert_eq!(values, [r#"{"declared":true}"#, "\"active\"", "17"]);
}

#[test]
fn active_and_inactive_are_literal_data_not_membership_verdicts() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add posit [{(+member, thing), (+class, class)}, "active", '2024-01-01'];
            add posit [{(member, thing), (class, class)}, "inactive", '2025-01-01'];
            "#,
        )
        .unwrap();

    let history = engine
        .execute_collect(
            "search [{(?member, thing), (?class, class)}, ?state, ?time] return ?state, ?time order by ?time;",
        )
        .unwrap();
    assert_eq!(
        history.rows,
        [
            ["\"active\"".to_string(), "2024-01-01".to_string()],
            ["\"inactive\"".to_string(), "2025-01-01".to_string()]
        ]
    );
    let literal_filter = engine
        .execute_collect(
            "search [{(?member, thing), (?class, class)}, \"active\", ?time] return ?member, ?class;",
        )
        .unwrap();
    assert_eq!(literal_filter.rows.len(), 1);
}

#[test]
fn ordinary_search_does_not_traverse_subclass_statements() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            r#"
            add posit [{(+member, thing), (+child, class)}, "included", '2024-01-01'];
            add posit [{(child, subclass), (+parent, class)}, "included", '2024-01-01'];
            "#,
        )
        .unwrap();

    let direct = engine
        .execute_collect(
            "search [{(?member, thing), (?class, class)}, ?state, *] return ?member, ?class, ?state;",
        )
        .unwrap();
    assert_eq!(direct.rows.len(), 1);
    let child = direct.rows[0][1].text.clone();

    let hierarchy = engine
        .execute_collect(
            "search [{(?child, subclass), (?parent, class)}, ?state, *] return ?child, ?parent, ?state;",
        )
        .unwrap();
    assert_eq!(hierarchy.rows.len(), 1);
    assert_eq!(hierarchy.rows[0][0].text, child);

    let all_direct_classes = engine
        .execute_collect("search [{(?member, thing), (?class, class)}, *, *] return ?class;")
        .unwrap();
    assert_eq!(all_direct_classes.rows.len(), 1);
}
