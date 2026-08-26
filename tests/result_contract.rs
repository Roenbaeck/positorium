use positorium::{Database, Engine, PersistenceMode, ResultCellKind};

#[test]
fn rust_results_use_lossless_structured_cells() {
    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let result = Engine::new(&database)
        .execute_collect(
            "add role value; add posit [{(+item, value)}, +0010.00, '2024-05']; search [{(?found, value), ...}, ?literal, ?time] return ?found, ?literal, ?time;",
        )
        .unwrap();

    assert_eq!(result.rows.len(), 1);
    assert_eq!(result.rows[0][0].kind, ResultCellKind::Thing);
    assert_eq!(result.rows[0][1].kind, ResultCellKind::Literal);
    assert_eq!(result.rows[0][1].text, "+0010.00");
    assert_eq!(result.rows[0][2].kind, ResultCellKind::Time);
    assert_eq!(result.rows[0][2].text, "2024-05");

    let json = serde_json::to_value(&result).unwrap();
    assert_eq!(json["rows"][0][1]["kind"], "literal");
    assert_eq!(json["rows"][0][1]["text"], "+0010.00");
    assert!(json.get("row_types").is_none());
}
