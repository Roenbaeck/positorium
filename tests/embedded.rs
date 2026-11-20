use positorium::{
    Database, DatabaseError, ExecutionOptions, LiteralFamily, LiteralValue, ResultCellKind,
};

#[test]
fn application_values_round_trip_through_the_public_api() {
    let db = Database::memory().unwrap();
    assert!(db.execute("add role name, amount;").unwrap().is_empty());
    let name = "Line one\n\"quoted\"; add role unexpected; 🦀";
    let amount = LiteralValue::new("+0010.00", LiteralFamily::Decimal).unwrap();
    let opts = ExecutionOptions::default()
        .bind("name", name)
        .bind("amount", amount);
    let sets = db
        .execute_with_options(
            "add posit [{(+p,name)},$name,@NOW], [{(p,amount)},$amount,@NOW];
         search [{(?p,name)}, ?text, *] return ?text;
         search [{(?p,amount)}, ?n, *] return ?p, ?n;",
            opts,
        )
        .unwrap();
    assert_eq!(sets.len(), 2);
    let text = sets[0].cell(0, "text").unwrap();
    assert_eq!(text.kind, ResultCellKind::Literal);
    assert_eq!(serde_json::from_str::<String>(text.as_str()).unwrap(), name);
    assert_eq!(sets[1].cell(0, "p").unwrap().kind, ResultCellKind::Thing);
    assert_eq!(sets[1].cell(0, "n").unwrap().as_str(), "+0010.00");
    assert!(sets[1].cell(1, "n").is_none());
    assert!(sets[1].cell(0, "text").is_none());
    assert!(!db.contains_role("unexpected").unwrap());
    let empty = db
        .execute_one_with_options(
            "search [{(?p,amount)},$n,*] return ?p;",
            ExecutionOptions::default().bind("n", 11_i64),
        )
        .unwrap();
    assert_eq!(empty.row_count, 0);
    assert_eq!(empty.columns, ["p"]);
}

#[test]
fn single_result_shape_is_checked_before_writes() {
    let db = Database::memory().unwrap();
    for script in [
        "add role unused;",
        "add role unused; search [{(?p,unused)},*,*] return ?p; search [{(?p,unused)},*,*] return ?p;",
    ] {
        assert!(matches!(
            db.execute_one(script),
            Err(DatabaseError::Execution(_))
        ));
        assert!(!db.contains_role("unused").unwrap());
    }
    let saved = db.execute_one(
        r#"add role amount; add constraint "Quantity" { search ?p=[{(?o,amount)},?n,*] where ?n < 0 return ?o,?p; };"#,
    ).unwrap();
    assert_eq!(
        saved.cell(0, "revision").unwrap().kind,
        ResultCellKind::Posit
    );
    let revision = saved.reference(0, "revision").unwrap();
    let options = ExecutionOptions::default().bind("saved", revision);
    let pinned = db
        .execute_one_with_options("check constraint $saved;", options.clone())
        .unwrap();
    assert_eq!(
        pinned.reference(0, "revision").unwrap(),
        saved.reference(0, "revision").unwrap()
    );
    assert!(matches!(
        Database::memory()
            .unwrap()
            .execute_one_with_options("check constraint $saved;", options),
        Err(DatabaseError::Parameter(_))
    ));
    assert!(saved.reference(0, "store").is_err());
    assert!(
        db.execute_one(r#"check constraint "Quantity";"#)
            .unwrap()
            .cell(0, "report")
            .is_some()
    );
}
