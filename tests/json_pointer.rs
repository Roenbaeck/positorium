use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, ExecutionParameter, PersistenceMode,
    QueryLimits, ResultCellKind,
};
use serde_json::Value;

const DOCUMENT: &str = r#"{ "items": [{ "sku": "\u0041", "qty": 1.00e+2 }], "nil": null, "": [], "a/b": {"~": true}, "~1": false, "01": 3, "a\\b": 4, "k\u00e9": 5, "huge": 123456789012345678901234567890123456789.01 }"#;
const SEARCH: &str = "search [{(?order, document)}, ?doc, *]";

fn database() -> Database {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    Engine::new(&db)
        .execute(&format!(
            "add role document; add posit [{{(+o, document)}}, {DOCUMENT}, '2026'];"
        ))
        .unwrap();
    db
}

#[test]
fn pointers_select_original_tokens_in_predicates_and_results() {
    let db = database();
    let engine = Engine::new(&db);
    for (pointer, expected) in [
        ("", DOCUMENT),
        ("/", "[]"),
        ("/a~1b/~0", "true"),
        ("/~01", "false"),
        ("/01", "3"),
        ("/a\\b", "4"),
        ("/ké", "5"),
        ("/nil", "null"),
        ("/items/0/qty", "1.00e+2"),
        ("/items/0/sku", r#""\u0041""#),
        ("/items", r#"[{ "sku": "\u0041", "qty": 1.00e+2 }]"#),
    ] {
        let path = serde_json::to_string(pointer).unwrap();
        let result = engine
            .execute_collect(&format!(
                "{SEARCH} where json_exists(?doc, {path}) and json_at(?doc, {path}) === {expected}
             return json_at(?doc, {path}) as ?value, json_exists(?doc, {path});"
            ))
            .unwrap();
        assert_eq!(result.columns[0], "value");
        assert_eq!(result.rows.len(), 1, "{pointer}");
        assert_eq!(result.rows[0][0].text, expected);
        assert_eq!(result.rows[0][0].kind, ResultCellKind::Literal);
        assert_eq!(result.rows[0][1], "true");
    }
    for comparison in [
        r#"json_at(?doc, "/items/0/sku") = "A""#,
        r#"json_at(?doc, "/items/0/qty") = 100"#,
        r#"100.0 ?= json_at(?doc, "/items/0/qty")"#,
        r#"json_at(?doc, "/items") = [{"qty":100,"sku":"A"}]"#,
        r#"json_at(?doc, "/items/0/qty") < json_at(?doc, "/huge")"#,
        r#"json_at(?doc, "/huge") > 123456789012345678901234567890123456789.00"#,
        r#"?doc = json_at(?doc, "")"#,
        r#"json_at([1e400, null, true], "/1") = null"#,
        r#"json_exists(null, "")"#,
        r#"json_at({"nothing": [1e400], "key": 9}, "/key") = 9"#,
    ] {
        let query = format!("{SEARCH} where {comparison} return ?order;");
        assert_eq!(
            engine.execute_collect(&query).unwrap().row_count,
            1,
            "{comparison}"
        );
    }
    for comparison in [
        r#"json_at(?doc, "/items/0/qty") = "100""#,
        r#"json_at(?doc, "/items/0/sku") === "A""#,
        r#"json_at(?doc, "/items/0/qty") === 100"#,
        r#"json_at(?doc, "/nil") = false"#,
    ] {
        assert_eq!(
            engine
                .execute_collect(&format!("{SEARCH} where {comparison} return ?order;"))
                .unwrap()
                .row_count,
            0,
            "{comparison}"
        );
    }
}

#[test]
fn absent_paths_are_distinct_from_null_and_bad_pointer_syntax() {
    let db = database();
    let engine = Engine::new(&db);
    for pointer in [
        "/absent",
        "/items/1",
        "/items/00",
        "/items/+0",
        "/items/-",
        "/items/999999999999999999999999999",
        "/nil/child",
    ] {
        let condition = format!(
            r#"not json_exists(?doc, "{pointer}") and json_exists(?doc, "{pointer}") = false"#
        );
        let query = format!("{SEARCH} where {condition} return ?order;");
        assert_eq!(
            engine.execute_collect(&query).unwrap().row_count,
            1,
            "{pointer}"
        );
        let query = format!(r#"{SEARCH} where json_at(?doc, "{pointer}") === null return ?order;"#);
        assert_eq!(engine.execute_collect(&query).unwrap().row_count, 0);
    }
    for pointer in ["a", "#/a", "/~", "/bad~2key"] {
        let query = format!(
            r#"search [{{(*, unknown_role)}}, ?d, *] return json_exists(?d, "{pointer}");"#
        );
        let error = engine.explain(&query).unwrap_err();
        assert!(error.to_string().contains("JSON Pointer"), "{error}");
    }
    for query in [
        format!(r#"{SEARCH} return json_at(?doc, "/missing");"#),
        format!(r#"{SEARCH} return json_exists(?unknown, "");"#),
        format!(r#"{SEARCH} where json_exists(?order, "") return ?order;"#),
        format!(r#"{SEARCH} return json_at(?doc, "") as ?order;"#),
    ] {
        assert!(engine.execute_collect(&query).is_err(), "{query}");
    }
}

#[test]
fn filters_use_the_selected_revision_and_aliases_sort_by_numeric_value() {
    let db = database();
    let engine = Engine::new(&db);
    engine
        .execute(
            r#"add role import;
        add posit [{(+a, import)}, {"n": 2, "was": true}, '2024'],
                  [{(a, import)}, {"n": 10}, '2025'],
                  [{(+b, import)}, {"n": 3}, '2025'],
                  [{(+c, import)}, "plain text", '2025'];"#,
        )
        .unwrap();
    let query =
        r#"search [{(?o, import)}, ?d, *] as of '2026' where json_exists(?d, "/was") return ?o;"#;
    assert_eq!(engine.execute_collect(query).unwrap().row_count, 0);
    let query = r#"search [{(?o, import)}, ?d, *] as of '2026' where json_exists(?d, "/n")
                   return json_at(?d, "/n") as ?n order by ?n;"#;
    assert_eq!(
        engine.execute_collect(query).unwrap().rows,
        vec![vec!["3"], vec!["10"]]
    );
    engine.explain(query).unwrap();
    let query = r#"search [{(?o, import)}, ?d, *] not exists { [{(?o, import)}, ?other, *]
                   where json_at(?other, "/n") > json_at(?d, "/n") }
                   where json_exists(?d, "/n") return json_at(?d, "/n") as ?n order by ?n;"#;
    assert_eq!(
        engine.execute_collect(query).unwrap().rows,
        vec![vec!["3"], vec!["10"]]
    );
}

#[test]
fn saved_constraints_check_parameterized_pointers_and_report_the_witness() {
    let db = database();
    let engine = Engine::new(&db);
    engine
        .execute(
            r#"add constraint "Quantity exceeds policy" {
        search ?record = [{(?order, document)}, ?doc, *]
        where json_at(?doc, $path) > json_at($policy, "/max") and json_at(?doc, $path) = $expected
        return ?order, ?record, json_at(?doc, $path) as ?quantity;
    };"#,
        )
        .unwrap();
    let options = ExecutionOptions {
        parameters: [
            ("path", r#""/items/0/qty""#),
            ("policy", r#"{"max":99}"#),
            ("expected", "1e2"),
        ]
        .into_iter()
        .map(|(key, text)| {
            (
                key.into(),
                ExecutionParameter::Literal { text: text.into() },
            )
        })
        .collect(),
        ..Default::default()
    };
    let result = engine
        .execute_collect_with_options(
            r#"check constraint "Quantity exceeds policy";"#,
            options.clone(),
        )
        .unwrap();
    let report: Value = serde_json::from_str(&result.rows[0][3].text).unwrap();
    assert_eq!(report["verdict"], "violated");
    assert_eq!(report["findings"].as_array().unwrap().len(), 1);
    let finding = &report["findings"][0];
    assert_eq!(finding["observed"][2]["text"], "1.00e+2");
    assert_eq!(finding["witnesses"].as_array().unwrap().len(), 1);
    assert_eq!(finding["witnesses"][0], finding["observed"][1]);
    let mut wrong = options;
    wrong.parameters.insert(
        "path".into(),
        ExecutionParameter::Literal { text: "10".into() },
    );
    assert!(
        engine
            .execute_collect_with_options(r#"check constraint "Quantity exceeds policy";"#, wrong)
            .is_err()
    );
    let bad = r#"add constraint "bad" { search [{(?o, document)}, ?d, *]
                 where json_exists($both, $both) return ?o; };"#;
    assert!(
        engine
            .execute(bad)
            .unwrap_err()
            .to_string()
            .contains("incompatible uses")
    );
}

#[test]
fn copied_fragments_count_toward_the_query_memory_limit() {
    let db = database();
    let engine = Engine::new(&db);
    let document = serde_json::json!({"large": "x".repeat(32768)}).to_string();
    let options = ExecutionOptions {
        parameters: [("doc".into(), ExecutionParameter::Literal { text: document })].into(),
        query_limits: QueryLimits {
            max_memory_bytes: Some(8192),
            ..Default::default()
        },
        ..Default::default()
    };
    let search = "search [{(?o, document)}, *, *]";
    let exists = format!(r#"{search} where json_exists($doc, "/large") return ?o;"#);
    assert_eq!(
        engine
            .execute_collect_with_options(&exists, options.clone())
            .unwrap()
            .row_count,
        1
    );
    for clause in [
        r#"return json_at($doc, "/large")"#,
        r#"where json_at($doc, "/large") === "x" return ?o"#,
    ] {
        let error = engine
            .execute_collect_with_options(&format!("{search} {clause};"), options.clone())
            .unwrap_err();
        assert!(matches!(
            error,
            DatabaseError::ResourceLimit {
                resource: "query memory bytes",
                ..
            }
        ));
    }
}
