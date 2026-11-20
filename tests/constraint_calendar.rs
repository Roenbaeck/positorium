use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, ExecutionParameter, PersistenceMode,
};

#[test]
fn calendar_intervals_preserve_precision_and_literal_spelling() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    e.execute("add role day; add posit [{(+o,day)},'2024-02-28','2024-03'];")
        .unwrap();
    for condition in [
        "overlaps(?month, add_days(?leap, 2))",
        "disjoint(?month, '2024-04')",
        "contains(?month, '2024-03-31')",
        "within('2024-03-01', ?month)",
        "definitely_before(?leap, '2024-02-29')",
        "definitely_after(?month, ?leap)",
        "possibly_before(?month, '2024-03-10')",
        "possibly_after(?month, '2024-03-10')",
        "within(add_days(?leap, 1), '2024-02-29')",
        "contains(subtract_days(?month,1), '2024-02-29')",
        "min_days_between(?month, '2024-04-01') = 1",
        "max_days_between(?month, '2024-04-01') = 31",
        "min_days_between('2024-04-01', ?month) = -31",
        "max_days_between('2024-04-01', ?month) = -1",
    ] {
        let query =
            format!("search [{{(?o,day)}},?leap,?month] where {condition} return ?o, ?leap;");
        assert_eq!(
            e.execute_collect(&query).unwrap().rows[0][1].text,
            "'2024-02-28'",
            "{condition}"
        );
        e.explain(&query).unwrap();
    }
    for condition in [
        "possibly_before('2024-02-29 00:00:00','2024-02-29 00:00:00')",
        "possibly_after('2024-02-29 00:00:00','2024-02-29 00:00:00')",
        "possibly_before('2024-02-29 23:59:59.999999999','2024-02-29')",
    ] {
        let query = format!("search [{{(?o,day)}},*,*] where {condition} return ?o;");
        assert_eq!(e.execute_collect(&query).unwrap().row_count, 0);
    }
    for condition in [
        "disjoint(@NOW, add_days(@BOT, 1))",
        "min_days_between(@EOT, '2024') > 1",
        "within(add_days(?leap,9223372036854775807),?month)",
        "overlaps(?o,?month)",
    ] {
        let query = format!("search [{{(?o,day)}},?leap,?month] where {condition} return ?o;");
        assert!(
            matches!(e.execute(&query), Err(DatabaseError::Comparison(_))),
            "{condition}"
        );
    }
    let opts = ExecutionOptions {
        parameters: [
            (
                "when".into(),
                ExecutionParameter::Time {
                    text: "'2024-02-29'".into(),
                },
            ),
            (
                "days".into(),
                ExecutionParameter::Literal { text: "1".into() },
            ),
        ]
        .into(),
        ..Default::default()
    };
    e.execute("add constraint \"leap\" { search [{(?o,day)},?d,*] where overlaps(add_days(?d,$days),$when) return ?o; };").unwrap();
    assert_eq!(
        e.execute_collect_with_options("check constraint \"leap\";", opts)
            .unwrap()
            .row_count,
        1
    );
}
