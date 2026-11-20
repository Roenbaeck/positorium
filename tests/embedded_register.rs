use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, ExecutionParameter, PlanNode, PlanOperator,
    QueryLimits, ResultCellKind,
};
use serde_json::Value;
use std::fmt::Write;
use std::hash::Hasher;

const BY_KEY: &str = r#"search [{(?s, session_key)}, $key, *],
    ?m = [{(?i, item), (?s, session)}, "registered", *],
    ?a = [{(?m, posit), (?src, ascertains)}, ?c, *],
    [{(?src, authority)}, ?authority, *] return ?i, ?src, ?c, ?authority;"#;
const BY_ID: &str = r#"search ?m = [{(?i, item), (?s, session)}, "registered", ?introduced],
    [{(?i, statement)}, ?text, *] as of $t where ?s = $session return ?i, ?text;"#;

fn reference(
    db: &Database,
    rows: &positorium::CollectedResultSet,
    column: &str,
) -> ExecutionParameter {
    let cell = rows.cell(0, column).unwrap();
    let text = format!("{}/{}", db.store_id(), cell.text);
    match cell.kind {
        ResultCellKind::Thing => ExecutionParameter::Thing { text },
        ResultCellKind::Posit => ExecutionParameter::Posit { text },
        _ => panic!("{column} must be an identity"),
    }
}

fn operations(node: &PlanNode) -> Vec<&PlanOperator> {
    let mut ops = vec![&node.operation];
    ops.extend(node.inputs.iter().flat_map(operations));
    ops
}

#[test]
fn register_joins_start_at_the_key_or_identity_and_follow_the_session() {
    let db = Database::memory().unwrap();
    let mut script =
        String::from("add role session_key, session, item, statement, authority; add posit ");
    for i in 0..50 {
        write!(script, r#"[{{(+dev{i}, authority)}}, "dev-{i}", '2024'], "#).unwrap();
    }
    for i in 0..170 {
        write!(
            script,
            r#"[{{(+s{i}, session_key)}}, "sess-{i}", '2024'],
            [{{(+i{i}, item), (s{i}, session)}}, "registered", '2024']
                and assert by dev{} with 90% at '2024',
            [{{(i{i}, statement)}}, "original", '2024'],
            [{{(i{i}, statement)}}, "revised", '2026'], "#,
            i % 50
        )
        .unwrap();
    }
    script.push(';');
    db.execute(&script).unwrap();
    let session = reference(
        &db,
        &db.execute_one(r#"search [{(?s, session_key)}, "sess-0", *] return ?s;"#)
            .unwrap(),
        "s",
    );
    let opts = ExecutionOptions {
        query_limits: QueryLimits {
            max_work_units: Some(250),
            ..Default::default()
        },
        ..Default::default()
    }
    .bind("key", "sess-0")
    .bind("session", session)
    .bind(
        "t",
        ExecutionParameter::Time {
            text: "2025".into(),
        },
    );
    for (query, order) in [(BY_KEY, vec![1, 2, 3, 4]), (BY_ID, vec![1, 2])] {
        let plan = Engine::new(&db)
            .explain_with_options(query, opts.clone())
            .unwrap();
        let ops = operations(&plan.root);
        assert!(ops.iter().any(|op| matches!(op, PlanOperator::NestedLoopJoin {pattern_order, ..} if pattern_order == &order)));
        assert!(ops.iter().any(|op| matches!(
            op,
            PlanOperator::StructuralScan {
                source_pattern: 1,
                estimated_candidates: 1,
                ..
            }
        )));
    }
    let by_key = db.execute_one_with_options(BY_KEY, opts.clone()).unwrap();
    let by_id = db.execute_one_with_options(BY_ID, opts.clone()).unwrap();
    // A conflicting correlated anchor makes only the absence block empty.
    for (key, count) in [("sess-0", 0), ("sess-1", 1)] {
        let result = db
            .execute_one_with_options(
                r#"search [{(?s,session_key)},$key,*]
            not exists { [{(?s,session),(?i,item)},*,*] where ?s = $session } return ?s;"#,
                opts.clone().bind("key", key),
            )
            .unwrap();
        assert_eq!(result.row_count, count);
    }
    assert_eq!(by_key.row_count, 1);
    assert_eq!(by_id.row_count, 1);
    assert_eq!(by_key.cell(0, "i"), by_id.cell(0, "i"));
    assert_eq!(by_key.cell(0, "authority").unwrap().text, r#""dev-0""#);
    assert_eq!(by_id.cell(0, "text").unwrap().text, r#""original""#);
}

#[test]
fn nominal_postings_preserve_equivalent_tokens_errors_and_historical_states() {
    let db = Database::memory().unwrap();
    db.execute(
        r#"add role string_value, amount, document, certainty_value;
        add posit [{(+x,string_value)}, "\u0041", '2020'], [{(x,string_value)}, "B", '2022'],
          [{(x,amount)}, +010.00, '2020'],
          [{(x,document)}, {"b":1e3,"a":[true,2.000]}, '2020'],
          [{(x,certainty_value)}, +050%, '2020'];"#,
    )
    .unwrap();
    for (role, literal) in [
        ("string_value", r#""A""#),
        ("amount", "10"),
        ("document", r#"{"a":[true,2],"b":1000}"#),
        ("certainty_value", "50%"),
    ] {
        let q = format!("search [{{(?x,{role})}},{literal},*] return ?x;");
        assert_eq!(db.execute_one(&q).unwrap().row_count, 1);
    }
    for (latest, expected) in [("", 0), ("latest ", 1)] {
        let q =
            format!(r#"search {latest}[{{(?x,string_value)}}, "A", *] as of '2025' return ?x;"#);
        assert_eq!(db.execute_one(&q).unwrap().row_count, expected);
    }
    let x = reference(
        &db,
        &db.execute_one("search [{(?x,amount)},*,*] return ?x;")
            .unwrap(),
        "x",
    );
    db.execute(r#"add posit [{(+other,amount)}, "invalid family", '2024'];"#)
        .unwrap();
    assert!(matches!(
        db.execute_one_with_options(
            "search [{(?x,amount)},10,*] where ?x = $x return ?x;",
            ExecutionOptions::default().bind("x", x)
        ),
        Err(DatabaseError::Comparison(_))
    ));
}

#[test]
fn qualified_appearances_work_in_appends_searches_fallbacks_and_saved_definitions() {
    let db = Database::memory().unwrap();
    let seed = db
        .execute_one(
            r#"add role session, item, evidence;
        add posit [{(+s,session)},"seed",'2024'];
        search ?p=[{(?s,session)},"seed",*] return ?p,?s;"#,
        )
        .unwrap();
    let opts = ExecutionOptions::default()
        .bind("s", reference(&db, &seed, "s"))
        .bind("p", reference(&db, &seed, "p"));
    let insert = r#"add posit [{($s,session),(+i,item)},"registered",'2024'],
        [{($p,evidence)},1,'2024'];"#;
    db.execute_with_options(insert, opts.clone()).unwrap();
    let query = r#"search [{($s,session),(?i,item)},"registered",*] return ?i;"#;
    assert_eq!(
        db.execute_one_with_options(query, opts.clone())
            .unwrap()
            .row_count,
        1
    );
    let promoted = db
        .execute_one_with_options(
            r#"search [{($s,session),(?i,item)},"late",*] return ?i
        or add posit [{($s,session),(+i,item)},"late",'2024'];"#,
            opts.clone(),
        )
        .unwrap();
    assert_eq!(promoted.row_count, 1);
    let saved = db
        .execute_one(
            r#"add constraint "linked" {
      search ?w=[{($p,evidence)},1,*] return ?w; };"#,
        )
        .unwrap();
    let revision = saved.reference(0, "revision").unwrap();
    let check = db
        .execute_one_with_options("check constraint $rev;", opts.clone().bind("rev", revision))
        .unwrap();
    let report: Value = serde_json::from_str(&check.cell(0, "report").unwrap().text).unwrap();
    assert_eq!(report["findings"].as_array().unwrap().len(), 1);
    assert!(matches!(
        Database::memory()
            .unwrap()
            .execute_with_options(insert, opts.clone()),
        Err(DatabaseError::Parameter(_))
    ));
    for invalid in [
        ExecutionParameter::from("string"),
        ExecutionParameter::Thing {
            text: format!("{}/999999", db.store_id()),
        },
        ExecutionParameter::Posit {
            text: format!("{}/{}", db.store_id(), seed.cell(0, "s").unwrap().text),
        },
    ] {
        assert!(matches!(
            db.execute_with_options(insert, opts.clone().bind("s", invalid)),
            Err(DatabaseError::Parameter(_))
        ));
    }
    assert!(matches!(
        db.execute_one_with_options(query, opts.bind("s", 12_i64)),
        Err(DatabaseError::Parameter(_))
    ));
}

#[test]
fn strict_mutations_catch_only_promotions_consumed_by_a_write() {
    let db = Database::memory().unwrap();
    db.execute(
        r#"add role session_key, session, turn, cited_turn;
       add posit [{(+s1, session_key)}, "sess-1", '2024'], [{(+s2,session_key)}, "sess-2", '2024'],
          [{(+t1,turn),(s1,session)},5,'2024'], [{(+t2,turn),(s2,session)},5,'2024'];"#,
    )
    .unwrap();
    let lookups = r#"search [{(?s,session_key)},"sess-1",*] or add posit [{(+s,session_key)},"sess-1",@NOW];
       search [{(?t,turn),(?s,session)},5,*] or add posit [{(+t,turn),(s,session)},5,@NOW];"#;
    let write = r#"add posit [{(t,cited_turn)},"quote",'2024'];"#;
    db.execute(&format!("strict mutations; {lookups}")).unwrap();
    for use_of_t in [
        write,
        r#"add posit [{(+v,cited_turn)},1,@NOW] and assert by t with 50% at @NOW;"#,
    ] {
        let error = db
            .execute(&format!("strict mutations; {lookups} {use_of_t}"))
            .unwrap_err();
        assert!(
            error.to_string().contains("'t' holds 2 identities"),
            "{error}"
        );
    }
    let count = || {
        db.execute_one("search [{(?t,cited_turn)},*,*] return ?t;")
            .unwrap()
            .row_count
    };
    assert_eq!(count(), 0);
    db.execute(&format!("{lookups} {write}")).unwrap();
    assert_eq!(count(), 2);
}

#[test]
fn compact_audits_preserve_the_version_one_digest_and_the_actual_read_set() {
    let db = Database::memory().unwrap();
    db.execute(
        r#"add role unrelated, reviewer;
        add posit [{(+r,reviewer)},1,'2024'];
        add constraint "empty" { search ?v=[{(*,unrelated)},-1,*] return ?v; };"#,
    )
    .unwrap();
    let reviewer = reference(
        &db,
        &db.execute_one("search [{(?r,reviewer)},*,*] return ?r;")
            .unwrap(),
        "r",
    );
    let check = |full: bool, record: bool| {
        let q = format!(
            "check constraint \"empty\"{}{};",
            if full { " with full manifest" } else { "" },
            if record { " record by $r" } else { "" }
        );
        let result = db
            .execute_one_with_options(&q, ExecutionOptions::default().bind("r", reviewer.clone()))
            .unwrap();
        let text = &result.cell(0, "report").unwrap().text;
        (serde_json::from_str::<Value>(text).unwrap(), text.len())
    };
    let (before, bytes_before) = check(false, true);
    let mut add = String::from("add posit ");
    for i in 0..1000 {
        write!(
            add,
            r#"[{{(+u{i},unrelated)}},{i},'2024'],[{{(u{i},name)}},"name-{i}",'2024'],"#
        )
        .unwrap();
    }
    add.push(';');
    db.execute(&add).unwrap();
    let (full, _) = check(true, false);
    let (compact, bytes_after) = check(false, true);
    assert_eq!(compact["version"], 2);
    assert_eq!(compact["definition"]["constraint"], 1);
    assert!(compact["snapshot"].get("posits").is_none());
    assert_eq!(compact["snapshot"]["digest"], full["snapshot"]["digest"]);
    assert_ne!(compact["snapshot"]["digest"], before["snapshot"]["digest"]);
    assert_eq!(compact["read_posits"], full["read_posits"]);
    assert_eq!(before["read_posits"], compact["read_posits"]);
    assert!(bytes_after.abs_diff(bytes_before) < 128);
    let manifest: Vec<u64> = full["snapshot"]["posits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| id.as_str().unwrap().parse().unwrap())
        .collect();
    assert!(manifest.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(manifest.len() as u64, compact["snapshot"]["count"]);
    let mut hash = seahash::SeaHasher::new();
    for id in manifest {
        hash.write(&id.to_le_bytes());
    }
    assert_eq!(
        compact["snapshot"]["digest"],
        format!("{:016x}", hash.finish())
    );
}

#[cfg(feature = "persistence")]
#[test]
fn same_command_targets_assertions_and_meta_assertions_are_one_durable_batch() {
    use positorium::maintenance::inspect_store;
    let path = std::env::temp_dir().join(format!("positorium-register-{}", std::process::id()));
    let db = Database::open(&path).unwrap();
    let seed = db
        .execute_one(
            r#"add role item, note;
        add posit [{(+i,item)},"old",'2024'];
        search ?p=[{(?i,item)},"old",*] return ?p,?i;"#,
        )
        .unwrap();
    let opts = ExecutionOptions::default().bind("i", reference(&db, &seed, "i"));
    drop(db);
    let before = inspect_store(&path).unwrap();
    let db = Database::open(&path).unwrap();
    let command = r#"add posit +m [{($i,item)},"old",'2024'],
        +a [{(m,posit),(+dev,ascertains)},100%,'2024'] and assert +b by dev with 90% at '2025',
        [{(b,note)},"checked",'2025'];"#;
    db.execute_with_options(command, opts.clone()).unwrap();
    let query = r#"search [{(?m,posit),(?src,ascertains)},100%,*],
        ?a=[{(?m,posit),(?src,ascertains)},100%,*],
        ?b=[{(?a,posit),(?src,ascertains)},90%,*], [{(?b,note)},"checked",*] return ?m,?b;"#;
    let rows = db.execute_one(query).unwrap();
    assert_eq!(rows.row_count, 1);
    assert_eq!(
        rows.cell(0, "m").unwrap().text,
        seed.cell(0, "p").unwrap().text
    );
    assert_eq!(rows.cell(0, "b").unwrap().kind, ResultCellKind::Posit);
    let bad = command.replace("(b,note)", "(b,missing)");
    assert!(matches!(
        db.execute_with_options(&bad, opts),
        Err(DatabaseError::UnknownRole(_))
    ));
    drop(db);
    let after = inspect_store(&path).unwrap();
    assert_eq!(after.committed_batches, before.committed_batches + 1);
    assert_eq!(after.posits, before.posits + 3);
    assert_eq!(
        Database::open(&path)
            .unwrap()
            .execute_one(query)
            .unwrap()
            .rows,
        rows.rows
    );
    std::fs::remove_dir_all(path).unwrap();
}
