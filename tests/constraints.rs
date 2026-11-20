use positorium::traqula::parse_time;
use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, ExecutionParameter, PersistenceMode,
    ResultCellKind,
};
use serde_json::Value;

const MISSING: &str = r#"
    /* a brace } and a quoted literal are part of the source */
    search ?record = [{(?order, order_record)}, *, *]
    not exists { [{(?order, order_date)}, *, *] }
    return ?order, ?record;
"#;
const NEGATIVE: &str =
    "search ?record = [{(?order, quantity)}, ?n, *] where ?n < 0 return ?order, ?record, ?n;";

fn options() -> ExecutionOptions {
    ExecutionOptions {
        now: parse_time("'2026-11-20 12:00:00'"),
        ..Default::default()
    }
}

fn add(e: &Engine<'_>, name: &str, at: &str, source: &str) -> Vec<String> {
    let rows = e
        .execute_collect_with_options(
            &format!("add constraint \"{name}\" at {at} {{{source}}};"),
            options(),
        )
        .unwrap()
        .rows;
    rows[0].iter().map(|c| c.text.clone()).collect()
}

fn report(e: &Engine<'_>, script: &str, opts: ExecutionOptions) -> Value {
    let sets = e.execute_collect_multi_with_options(script, opts).unwrap();
    let set = sets.last().unwrap();
    assert_eq!(set.rows[0][0].kind, ResultCellKind::Thing);
    assert_eq!(set.rows[0][1].kind, ResultCellKind::Posit);
    serde_json::from_str(&set.rows[0][3].text).unwrap()
}

#[cfg(feature = "persistence")]
#[test]
fn revisions_survive_reopen_and_select_by_appearance_time() {
    let path = std::env::temp_dir().join(format!("positorium-constraints-{}", std::process::id()));
    let open = || Database::new(PersistenceMode::File(path.to_str().unwrap().into())).unwrap();
    let (old, new, store) = {
        let db = open();
        let engine = Engine::new(&db);
        engine.execute("add role order_record, order_date, quantity; add posit [{(+o, order_record)}, 1, @NOW], [{(o, quantity)}, -5, @NOW];").unwrap();
        let old = add(&engine, "orders", "'2026-01-01'", MISSING);
        assert_eq!(old, add(&engine, "orders", "'2026-01-01'", MISSING));
        let new = add(&engine, "orders", "'2026-12-01'", NEGATIVE);
        assert_eq!(old[0], new[0]);
        assert_ne!(old[1], new[1]);
        (old[1].clone(), new[1].clone(), db.store_id().to_owned())
    };
    {
        let db = open();
        assert_eq!(db.store_id(), store);
        let e = Engine::new(&db);
        let r = report(&e, "check constraint \"orders\";", options());
        assert_eq!(r["revision"], old);
        assert_eq!(r["verdict"], "indeterminate");
        assert_eq!(r["findings"].as_array().unwrap().len(), 1);
        let source = e
            .execute_collect("search [{(*, source)}, ?s, '2026-01-01'] return ?s;")
            .unwrap();
        assert_eq!(
            serde_json::from_str::<String>(&source.rows[0][0].text).unwrap(),
            MISSING
        );
        assert_eq!(
            report(&e, "check constraint \"orders\" as of '2027';", options())["revision"],
            new
        );
        assert!(
            e.execute("check constraint \"orders\" as of '2025';")
                .unwrap_err()
                .to_string()
                .contains("no applicable")
        );
        let backdated = add(&e, "orders", "'2026-10-01'", NEGATIVE);
        assert_eq!(
            report(&e, "check constraint \"orders\";", options())["revision"],
            backdated[1]
        );
        let mut opts = options();
        opts.parameters.insert(
            "pin".into(),
            ExecutionParameter::Posit {
                text: format!("{store}/{old}"),
            },
        );
        assert_eq!(report(&e, "check constraint $pin;", opts)["revision"], old);
        add(&e, "orders", "'2026-10-01'", MISSING);
        let error = e
            .execute_collect_with_options("check constraint \"orders\";", options())
            .unwrap_err();
        assert!(error.to_string().contains("ambiguous constraint revision"));
        add(&e, "overlap", "'2026-03'", MISSING);
        add(&e, "overlap", "'2026-03-12'", NEGATIVE);
        assert!(
            e.execute("check constraint \"overlap\" as of @EOT;")
                .unwrap_err()
                .to_string()
                .contains("ambiguous")
        );
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn checks_are_informative_atomic_and_exhaustive() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    assert!(
        e.execute("check constraint \"missing\";")
            .unwrap_err()
            .to_string()
            .contains("unknown constraint")
    );
    for body in [
        "search [{(?o,thing)},*,*];",
        "search [{(?o,thing)},*,*] return ?oops;",
        "search [{(?o,thing)},*,*] return ?o limit 1;",
        "search [{(?o,thing)},*,*] return ?o or add posit [{(+x,thing)},1,@NOW];",
    ] {
        assert!(
            e.execute(&format!("add constraint \"bad\" {{{body}}};"))
                .is_err()
        );
    }
    assert_eq!(db.role_count().unwrap(), 5);
    let setup = format!(
        r#"add role order_record, order_date, quantity, delivery, policy, manifest;
        add posit +record [{{(+o,order_record)}},1,@NOW], [{{(o,quantity)}},-5,@NOW];
        add constraint +rev "orders" {{{MISSING}}};
        add posit +entry [{{(record,delivery)}},"included",@NOW];
        add posit +ready [{{(rev,policy),(entry,manifest)}},"ready",@NOW];
        check constraint rev;
        check constraint rev with readiness ready;
        add posit [{{(o,order_date)}},'2026-01-01',@NOW];
        check constraint rev with readiness ready;"#
    );
    let sets = e.execute_collect_multi(&setup).unwrap();
    let checked: Vec<Value> = sets[1..]
        .iter()
        .map(|s| serde_json::from_str(&s.rows[0][3].text).unwrap())
        .collect();
    assert_eq!(checked[0]["verdict"], "indeterminate");
    assert_eq!(checked[1]["verdict"], "violated");
    assert_eq!(checked[2]["verdict"], "satisfied");
    add(&e, "quantity", "'2025'", NEGATIVE);
    assert_eq!(
        report(&e, "check constraint \"quantity\";", options())["verdict"],
        "violated"
    );
    e.execute("add posit [{(+another,quantity)},-1,@NOW];")
        .unwrap();
    let error = e
        .execute_collect_with_options(
            "check constraint \"quantity\";",
            ExecutionOptions {
                max_rows_per_search: Some(1),
                ..options()
            },
        )
        .unwrap_err();
    assert!(matches!(error, DatabaseError::ResourceLimit { .. }));
}

#[test]
fn ordinary_posits_discovery_pins_and_explicit_audits() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    e.execute(
        r#"add role name, source, quantity;
        add posit [{(+kind,class)},"declared",@BOT], [{(kind,name)},"Constraint",@BOT],
                  [{(+report,name)},"amounts",@BOT], [{(report,source)},"plain report",@BOT];
        add posit [{(+q,quantity)},-2,@NOW];"#,
    )
    .unwrap();
    let added = add(&e, "amounts", "@BOT", NEGATIVE);
    assert_eq!(
        e.execute_collect(r#"search [{(?p,name)},"amounts",*] return distinct ?p;"#)
            .unwrap()
            .row_count,
        2
    );
    assert_eq!(
        e.execute_collect(r#"search [{(?p,name)},"Constraint",*] return distinct ?p;"#)
            .unwrap()
            .row_count,
        1
    );
    let mut opts = options();
    opts.parameters.insert(
        "p".into(),
        ExecutionParameter::Thing {
            text: format!("{}/{}", db.store_id(), added[0]),
        },
    );
    let explicit = e
        .execute_collect_with_options(
            &format!("add constraint of $p at '2026-01-01' {{{NEGATIVE}}};"),
            opts.clone(),
        )
        .unwrap();
    assert_eq!(explicit.rows[0][0].text, added[0]);
    let engine_script = r#"search [{(?kind,name)},"Constraint",*] or add posit [{(+kind,name)},"Constraint",@NOW];
        add posit [{(+other,thing),(kind,class)},"included",@NOW], [{(other,name)},"amounts",@NOW];"#;
    e.execute(engine_script).unwrap();
    assert!(
        e.execute(r#"check constraint "amounts";"#)
            .unwrap_err()
            .to_string()
            .contains("ambiguous constraint name")
    );
    assert!(
        e.execute(&format!("add constraint \"amounts\" {{{MISSING}}};"))
            .unwrap_err()
            .to_string()
            .contains("ambiguous constraint name")
    );
    opts.parameters.insert(
        "rev".into(),
        ExecutionParameter::Posit {
            text: format!("{}/{}", db.store_id(), added[1]),
        },
    );
    let audited = e
        .execute_collect_multi_with_options(
            r#"add posit [{(+reviewer,name)},"Reviewer",@NOW];
        check constraint $rev with full manifest record by reviewer;"#,
            opts.clone(),
        )
        .unwrap();
    let row = &audited[0].rows[0];
    assert_eq!(audited[0].columns.last().unwrap(), "audit");
    let saved: Value = serde_json::from_str(&row[3].text).unwrap();
    assert!(
        !saved["snapshot"]["posits"]
            .as_array()
            .unwrap()
            .contains(&Value::String(row[4].text.clone()))
    );
    let audits = e
        .execute_collect(
            r#"search ?audit=[{(*,policy)},?result,*],
        [{(?audit,posit),(?evaluator,ascertains)},100%,*] return ?audit,?evaluator,?result;"#,
        )
        .unwrap();
    assert_eq!(audits.rows[0][0], row[4]);
    assert_eq!(audits.rows[0][2], row[3]);
    let other = Database::new(PersistenceMode::InMemory).unwrap();
    assert!(matches!(
        Engine::new(&other).execute_collect_with_options("check constraint $rev;", opts),
        Err(DatabaseError::Parameter(_))
    ));
}

#[test]
fn raw_imports_share_the_contract_and_malformed_rules_cannot_write() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    let mut opts = options();
    opts.parameters.insert(
        "code".into(),
        ExecutionParameter::Literal {
            text: serde_json::to_string(NEGATIVE).unwrap(),
        },
    );
    e.execute_collect_with_options(r#"add role name, source, quantity;
        add posit [{(+c,class)},"declared",@BOT],[{(c,name)},"Constraint",@BOT],
            [{(+p,thing),(c,class)},"included",@BOT],[{(p,name)},"raw",@BOT];
        add posit +rev [{(p,source)},$code,@BOT];
        add posit [{(rev,posit),(+a,ascertains)},100%,@NOW],[{(rev,posit),(+b,ascertains)},-100%,@NOW];"#,opts).unwrap();
    let checked = report(&e, r#"check constraint "raw";"#, options());
    assert!(checked["declarations"].as_array().unwrap().is_empty());
    for code in [
        "add role should_never_exist;",
        "search [{(?o,quantity)},*,*] return ?o or add posit [{(+x,quantity)},1,@NOW];",
    ] {
        let mut opts = options();
        opts.parameters.insert(
            "code".into(),
            ExecutionParameter::Literal {
                text: serde_json::to_string(code).unwrap(),
            },
        );
        let result = e.execute_collect_with_options(
            r#"search [{(?p,name)},"raw",*] or add posit [{(+p,name)},"raw",@BOT];
              add posit +r [{(p,source)},$code,@NOW]; check constraint r;"#,
            opts,
        );
        assert!(result.is_err());
        assert!(!db.contains_role("should_never_exist").unwrap());
    }
}

#[test]
fn first_saves_are_serialized_and_checks_share_the_script_snapshot() {
    use positorium::{MultiStreamCallbacks, ResultCell};
    use std::sync::{Barrier, mpsc};
    use std::time::Duration;
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    let start = Barrier::new(6);
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..6)
            .map(|_| {
                s.spawn(|| {
                    start.wait();
                    add(&e, "orders", "@BOT", MISSING)
                })
            })
            .collect();
        let saved: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert!(saved.windows(2).all(|w| w[0] == w[1]));
    });
    e.execute("add role order_record,order_date; add posit [{(+o,order_record)},1,@NOW];")
        .unwrap();
    struct Gate {
        ready: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
        counts: Vec<usize>,
    }
    impl MultiStreamCallbacks for Gate {
        fn on_result_set_start(&mut self, _: usize, _: &[String], _: &str) {}
        fn on_row(&mut self, _: usize, row: Vec<ResultCell>) -> bool {
            let result: Value = serde_json::from_str(&row[3].text).unwrap();
            self.counts
                .push(result["findings"].as_array().unwrap().len());
            true
        }
        fn on_result_set_end(&mut self, index: usize, _: usize, _: bool) {
            if index == 0 {
                self.ready.send(()).unwrap();
                self.resume.recv_timeout(Duration::from_secs(5)).unwrap();
            }
        }
    }
    let (ready, entered) = mpsc::sync_channel(1);
    let (release, resume) = mpsc::sync_channel(1);
    std::thread::scope(|s| {
        let reader = s.spawn(|| {
            let mut gate = Gate {
                ready,
                resume,
                counts: vec![],
            };
            e.execute_stream_multi(
                r#"check constraint "orders"; check constraint "orders";"#,
                &mut gate,
            )
            .unwrap();
            gate.counts
        });
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        e.execute(
            r#"search [{(?o,order_record)},1,*] or add posit [{(+o,order_record)},1,@NOW];
            add posit [{(o,order_date)},'2024-01-01',@NOW];"#,
        )
        .unwrap();
        release.send(()).unwrap();
        assert_eq!(reader.join().unwrap(), [1, 1]);
    });
    assert!(
        report(&e, r#"check constraint "orders";"#, options())["findings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn readiness_verifies_visible_deliveries_and_the_selected_population() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    let script = format!(
        r#"add role receipt,delivery,previous,policy,manifest;
        add constraint +rev "dates" {{{MISSING}}};
        add posit +receipt [{{(+loader,receipt)}},"empty export",@NOW];
        add posit +entry [{{(receipt,delivery)}},"included",@NOW], +waiting [{{(+missing,delivery)}},"included",@NOW];
        add posit +tail [{{(receipt,delivery),(waiting,previous)}},"included",@NOW];
        add posit +ready [{{(rev,policy),(entry,manifest)}},"ready",@NOW], +early [{{(rev,policy),(tail,manifest)}},"ready",@NOW];
        check constraint rev; check constraint rev with readiness early; check constraint rev with readiness ready;"#
    );
    let sets = e.execute_collect_multi(&script).unwrap();
    let verdicts: Vec<_> = sets[1..]
        .iter()
        .map(|set| {
            let report: Value = serde_json::from_str(&set.rows[0][3].text).unwrap();
            assert!(report["findings"].as_array().unwrap().is_empty());
            report["verdict"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(verdicts, ["indeterminate", "indeterminate", "satisfied"]);
    let mismatch = e
        .execute(
            r#"add constraint +revA "first" { search ?p=[*,*,*] return ?p; };
        add posit +declaration [{(revA,policy),(+node,manifest)},"ready",@NOW];
        add constraint +revB "second" { search ?p=[*,*,*] return ?p; };
        check constraint revB with readiness declaration;"#,
        )
        .unwrap_err();
    assert!(
        mismatch
            .to_string()
            .contains("does not cover this revision")
    );
}

#[test]
fn explicit_class_selection_and_incompatible_parameter_declarations() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    let result = e
        .execute_collect_multi(&format!(
            r#"add role name;
        add posit [{{(+a,class)}},"declared",@BOT],[{{(a,name)}},"Constraint",@BOT],
                  [{{(+b,class)}},"declared",@BOT],[{{(b,name)}},"Constraint",@BOT];
        add constraint "orders" in class a {{{MISSING}}};
        check constraint "orders" in class a;"#
        ))
        .unwrap();
    assert_eq!(result[0].rows[0][1], result[1].rows[0][1]);
    assert!(
        e.execute(r#"check constraint "orders";"#)
            .unwrap_err()
            .to_string()
            .contains("ambiguous Constraint class")
    );
    let err=e.execute(r#"add constraint "bad" { search ?p=[*,?v,$ambiguous] where ?v = $ambiguous return ?p; };"#).unwrap_err();
    assert!(matches!(err, DatabaseError::Parameter(_)));
}

#[test]
fn result_limits_and_stream_counts_follow_the_emitting_command() {
    use positorium::{ResultCell, RowSink, SinkFlow};
    struct Sink(Vec<Vec<ResultCell>>);
    impl RowSink for Sink {
        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
            self.0.push(row);
            SinkFlow::Continue
        }
    }
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    let opts = ExecutionOptions {
        max_rows_per_search: Some(1),
        ..options()
    };
    let script = format!("add constraint +r \"orders\" at @BOT {{{MISSING}}}; check constraint r;");
    let result = e
        .execute_collect_with_options(&script, opts.clone())
        .unwrap();
    assert_eq!(result.row_count, 2);
    let mut sink = Sink(Vec::new());
    let (columns, limited, rows) = e
        .execute_stream_single_with_options(
            r#"check constraint "orders"; search [*,*,*];"#,
            &mut sink,
            opts,
        )
        .unwrap();
    assert_eq!(columns.last().unwrap(), "report");
    assert_eq!((limited, rows, sink.0.len()), (false, 1, 1));
}
