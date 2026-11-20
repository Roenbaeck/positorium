use positorium::{Database, Engine, ExecutionOptions, ExecutionParameter, PersistenceMode};
use serde_json::{Value, json};

const POLICY: &str = include_str!("../traqula/cancellation-constraint.traqula");

fn evaluate(day: u32, window: u32, scenario: &str, complete: bool) -> Value {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let e = Engine::new(&db);
    let date = chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap() + chrono::Days::new(day.into());
    let coarse = scenario.starts_with("coarse");
    let date = if coarse {
        "2024-02".into()
    } else {
        date.to_string()
    };
    let placed = if coarse { "2024-01-15" } else { "2024-01-01" };
    let mut script = POLICY.replacen("add constraint ", "add constraint +rev ", 1);
    script.push_str(&format!(
        r#"
        add role request, subject, policy, agent, destination, relies_on, delivery, manifest;
        add posit +placed [{{(+ord,subject)}},"placed",'{placed}'],
                  [{{(ord,subject),(+country,destination)}},"at placement",'2024-01-01']
                      and assert by +shipper with 100% at '2024-01-01',
                  [{{(country,destination),(rev,policy)}},{window},@BOT],
                  [{{(+agent,agent),(rev,policy)}},"authorized",@BOT]
                      and assert by +manager with 100% at @BOT,
                  [{{(+req,request)}},"requested",'2024-01-01'];
    "#
    ));
    if scenario == "later delegation" {
        script.push_str(
            r#"add posit +withdrawn [{(agent,agent),(rev,policy)},"withdrawn",'2024-01-10']
            and assert by manager with 100% at '2024-01-10';"#,
        );
    }
    if scenario == "disputed country" {
        script.push_str(r#"add posit [{(ord,subject),(+alternative,destination)},"at placement",'2024-01-01']
            and assert by +importer with 100% at '2024-01-02', [{(alternative,destination),(rev,policy)},30,@BOT];"#);
    }
    let mut cite = String::new();
    if [
        "approved",
        "denied too",
        "disputed approval",
        "coarse approved",
        "withdrawn",
        "late decision",
        "wrong request",
        "backdated",
        "unauthorized",
        "later delegation",
    ]
    .contains(&scenario)
    {
        let request = if scenario == "wrong request" {
            "+another"
        } else {
            "req"
        };
        let reviewer = if scenario == "unauthorized" {
            "+stranger"
        } else {
            "agent"
        };
        let decided = if scenario == "late decision" {
            "2024-02-02"
        } else {
            "2024-01-31"
        };
        script.push_str(&format!(
            r#"add posit +decision [{{({request},request),(rev,policy)}},"approved",'{decided}']
                            and assert +signed by {reviewer} with 100% at '2024-02-15';"#
        ));
        cite = ",(signed,relies_on)".into();
        if scenario == "denied too" {
            script.push_str(r#"add posit [{(+other,agent),(rev,policy)},"authorized",@BOT] and assert by manager with 100% at @BOT,
                [{(req,request),(rev,policy)},"denied",'2024-01-31'] and assert by other with 100% at '2024-02-15';"#);
        }
        if scenario == "disputed approval" {
            script.push_str(r#"add posit [{(+other,agent),(rev,policy)},"authorized",@BOT] and assert by manager with 100% at @BOT,
                [{(decision,posit),(other,ascertains)},-100%,'2024-02-15'];"#);
        }
        if scenario == "withdrawn" {
            script.push_str("add posit [{(decision,posit),(agent,ascertains)},0%,'2024-02-20'];");
        }
    }
    if scenario != "pending" {
        script.push_str(&format!(
            r#"add posit [{{(req,request),(ord,subject),(rev,policy){cite}}},"cancelled",'{date}']
                and assert by +operator with 100% at '{date}';"#
        ));
    }
    let known = if scenario == "backdated" {
        "'2024-02-10'"
    } else {
        "@EOT"
    };
    let scope = json!({"ready":{"clock":null, "parameters":{"known_at":{"kind":"time","text":known},
            "action_cut":{"kind":"time","text":"@EOT"}}}});
    script.push_str(&format!(
        r#"add posit +entry [{{(placed,delivery)}},"included",@NOW];
        add posit +ready [{{(rev,policy),(entry,manifest)}},{scope},@NOW];
        check constraint rev {};"#,
        if complete { "with readiness ready" } else { "" }
    ));
    if scenario == "later delegation" {
        script.push_str(
            r#"add posit [{(agent,agent),(rev,policy)},"authorized",'2024-01-20']
            and assert by manager with 100% at '2024-01-20';
            check constraint rev with readiness ready;"#,
        );
    }
    let opts = ExecutionOptions {
        parameters: [
            (
                "known_at".into(),
                ExecutionParameter::Time { text: known.into() },
            ),
            (
                "action_cut".into(),
                ExecutionParameter::Time {
                    text: "@EOT".into(),
                },
            ),
        ]
        .into(),
        ..Default::default()
    };
    let sets = e.execute_collect_multi_with_options(&script, opts).unwrap();
    if scenario == "later delegation" {
        let prior: Value = serde_json::from_str(&sets[sets.len() - 2].rows[0][3].text).unwrap();
        assert_eq!(prior["verdict"], "violated");
    }
    serde_json::from_str(&sets.last().unwrap().rows[0][3].text).unwrap()
}

#[test]
fn country_windows_and_discretionary_approval() {
    for (day, window, scenario, complete, expected) in [
        (2, 2, "ordinary", true, "satisfied"),
        (3, 2, "ordinary", true, "violated"),
        (5, 2, "ordinary", true, "violated"),
        (5, 30, "ordinary", true, "satisfied"),
        (5, 2, "disputed country", true, "indeterminate"),
        (30, 30, "ordinary", true, "satisfied"),
        (31, 2, "approved", true, "satisfied"),
        (31, 30, "missing", true, "violated"),
        (31, 30, "missing", false, "indeterminate"),
        (31, 30, "pending", true, "satisfied"),
        (31, 30, "denied too", true, "indeterminate"),
        (31, 2, "withdrawn", true, "violated"),
        (31, 30, "wrong request", true, "violated"),
        (31, 30, "late decision", true, "violated"),
        (31, 30, "unauthorized", true, "violated"),
        (31, 2, "backdated", true, "violated"),
        (31, 2, "later delegation", true, "satisfied"),
        (31, 30, "disputed approval", true, "indeterminate"),
        (31, 30, "coarse approved", true, "satisfied"),
        (31, 2, "coarse approved", true, "indeterminate"),
        (31, 30, "coarse missing", true, "indeterminate"),
        (31, 2, "coarse missing", true, "violated"),
    ] {
        let r = evaluate(day, window, scenario, complete);
        assert_eq!(
            r["verdict"], expected,
            "day {day} window {window} {scenario}: {}",
            r["findings"]
        );
        if scenario == "approved" {
            assert!(r["evidence"][0]["witnesses"].as_array().unwrap().len() >= 5);
        }
    }
}
