use chrono::NaiveDate;
use positorium::construct::{Database, PersistenceMode};
use positorium::datatype::Time;
use positorium::traqula::{Engine, ExecutionOptions};

#[test]
fn now_override_is_shared_by_the_complete_script_and_reported() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let resolved_now = Time::from_naive_datetime(
        NaiveDate::from_ymd_opt(2026, 8, 26)
            .unwrap()
            .and_hms_nano_opt(12, 34, 56, 789_123_456)
            .unwrap(),
    );

    let result = engine
        .execute_collect_with_options(
            "add role observed; add posit [{(+event, observed)}, @NOW, @NOW]; search [{(*, observed)}, +value, +time] return value, time;",
            ExecutionOptions {
                now: Some(resolved_now.clone()),
            },
        )
        .expect("script executes");

    assert_eq!(result.metadata.resolved_now, resolved_now);
    assert_eq!(
        result.rows,
        vec![vec![
            "2026-08-26 12:34:56.789123456".to_string(),
            "2026-08-26 12:34:56.789123456".to_string(),
        ]]
    );
}

#[test]
fn multi_searches_report_one_shared_now_value() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    let results = engine
        .execute_collect_multi(
            "add role observed; add posit [{(+event, observed)}, @NOW, @NOW]; search [{(*, observed)}, +value, *] return value; search [{(*, observed)}, *, +time] return time;",
        )
        .expect("script executes");

    assert_eq!(results.len(), 2);
    assert_eq!(results[0].metadata, results[1].metadata);
    assert_eq!(results[0].rows[0][0], results[1].rows[0][0]);
}
