use positorium::construct::{Database, PersistenceMode};
use positorium::traqula::Engine;

fn engine() -> Engine<'static> {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(Box::leak(Box::new(database)))
}

#[test]
fn results_preserve_complete_literal_tokens() {
    let engine = engine();
    let result = engine
        .execute_collect(
            r#"
            add role value;
            add posit [{(+decimal, value)}, +001.00, @NOW];
            add posit [{(+string, value)}, "\u0041\u00e9", @NOW];
            add posit [{(+json, value)}, { "b": [true, null], "a": 1.00 }, @NOW];
            add posit [{(+certainty, value)}, 075%, @NOW];
            search [{(*, value)}, +literal, *] return literal;
            "#,
        )
        .expect("lossless literal script");

    let mut tokens: Vec<String> = result.rows.into_iter().map(|row| row[0].clone()).collect();
    tokens.sort();
    assert_eq!(
        tokens,
        vec![
            r#""\u0041\u00e9""#.to_string(),
            "+001.00".to_string(),
            "075%".to_string(),
            r#"{ "b": [true, null], "a": 1.00 }"#.to_string(),
        ]
    );
    assert!(
        result
            .row_types
            .iter()
            .all(|types| types == &["Literal".to_string()])
    );
}

#[test]
fn predicates_use_nominal_semantics_without_changing_results() {
    let engine = engine();
    engine
        .execute(
            r#"
        add role number;
        add role label;
        add role document;
        add posit [{(+number, number)}, +0010.00, @NOW];
        add posit [{(+label, label)}, "\u0041", @NOW];
        add posit [{(+document, document)}, {"a": 1.00, "b": [true, null]}, @NOW];
        "#,
        )
        .unwrap();

    let number = engine
        .execute_collect("search [{(*, number)}, +n, *] where n = 10 return n;")
        .expect("cross-numeric-family equality");
    assert_eq!(number.rows, vec![vec!["+0010.00".to_string()]]);

    let string = engine
        .execute_collect(r#"search [{(*, label)}, +s, *] where s = "A" return s;"#)
        .expect("decoded string equality");
    assert_eq!(string.rows, vec![vec![r#""\u0041""#.to_string()]]);

    let json = engine
        .execute_collect(
            r#"search [{(+item, document)}, { "b": [true,null], "a": 1 }, *] return item;"#,
        )
        .expect("structural JSON equality");
    assert_eq!(json.rows.len(), 1);
}

#[test]
fn arbitrary_precision_integer_predicates_are_exact() {
    let engine = engine();
    engine.execute(
        "add role number; add posit [{(+lower, number)}, 340282366920938463463374607431768211454, @NOW]; add posit [{(+upper, number)}, 340282366920938463463374607431768211455, @NOW];",
    ).unwrap();

    let result = engine
        .execute_collect(
            "search [{(*, number)}, +n, *] where n > 340282366920938463463374607431768211454 return n;",
        )
        .expect("arbitrary precision comparison");
    assert_eq!(
        result.rows,
        vec![vec!["340282366920938463463374607431768211455".to_string()]]
    );
}

#[test]
fn literal_operators_keep_identity_nominality_and_compatibility_distinct() {
    let engine = engine();
    engine
        .execute(
            r#"
        add role number;
        add role document;
        add posit [{(+number, number)}, +0010.00, @NOW];
        add posit [{(+document, document)}, {"a": 1.00, "b": [true, null]}, @NOW];
        "#,
        )
        .unwrap();

    let exact = engine
        .execute_collect("search [{(*, number)}, +n, *] where n === +0010.00 return n;")
        .expect("exact identity predicate");
    assert_eq!(exact.rows, vec![vec!["+0010.00".to_string()]]);

    let different_spelling = engine
        .execute_collect("search [{(*, number)}, +n, *] where n === 10.00 return n;")
        .expect("exact non-match");
    assert!(different_spelling.rows.is_empty());

    for operator in ["=", "==", "?="] {
        let result = engine
            .execute_collect(&format!(
                "search [{{(*, number)}}, +n, *] where n {operator} 10 return n;"
            ))
            .unwrap_or_else(|error| panic!("{operator} comparison failed: {error}"));
        assert_eq!(result.rows, vec![vec!["+0010.00".to_string()]]);
        if operator == "==" {
            assert_eq!(result.metadata.warnings.len(), 1);
            assert_eq!(result.metadata.warnings[0].code, "legacy-double-equals");
        } else {
            assert!(result.metadata.warnings.is_empty());
        }
    }

    let exact_json = engine
        .execute_collect(
            r#"search [{(*, document)}, +d, *] where d === {"a": 1.00, "b": [true, null]} return d;"#,
        )
        .expect("exact JSON identity");
    assert_eq!(exact_json.rows.len(), 1);

    let reordered_json = engine
        .execute_collect(
            r#"search [{(*, document)}, +d, *] where d === { "b": [true,null], "a": 1 } return d;"#,
        )
        .expect("presentation-sensitive JSON identity");
    assert!(reordered_json.rows.is_empty());
}

#[test]
fn malformed_structured_literals_fail_without_creating_a_posit() {
    let engine = engine();
    engine.execute("add role value;").unwrap();
    let error = engine
        .execute_collect(r#"add posit [{(+item, value)}, {"name": 1, "\u006eame": 2}, @NOW];"#)
        .unwrap_err();
    assert!(
        error.to_string().contains("duplicate JSON object key"),
        "{error}"
    );

    let result = engine
        .execute_collect("search [{(*, value)}, +literal, *] return literal;")
        .expect("database remains queryable");
    assert!(result.rows.is_empty());
}
