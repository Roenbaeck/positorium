use std::time::Duration;

use positorium::construct::{Database, PersistenceMode};
use positorium::error::DatabaseError;
use positorium::traqula::{CancellationToken, Engine, ExecutionOptions};

#[test]
fn pre_cancelled_execution_does_not_mutate_the_database() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = Engine::new(&db)
        .execute_collect_with_options(
            "add role never-created;",
            ExecutionOptions {
                cancellation: Some(cancellation),
                ..ExecutionOptions::default()
            },
        )
        .expect_err("pre-cancelled execution must fail");

    assert!(matches!(error, DatabaseError::Cancelled));
    assert!(
        db.role_keeper()
            .lock()
            .unwrap()
            .get("never-created")
            .is_none()
    );
}

#[test]
fn zero_timeout_fails_before_the_first_command() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let error = Engine::new(&db)
        .execute_collect_with_options(
            "add role never-created;",
            ExecutionOptions {
                timeout: Some(Duration::ZERO),
                ..ExecutionOptions::default()
            },
        )
        .expect_err("expired execution must fail");

    assert!(matches!(error, DatabaseError::Timeout));
    assert!(
        db.role_keeper()
            .lock()
            .unwrap()
            .get("never-created")
            .is_none()
    );
}

#[test]
fn execution_row_cap_reports_only_actual_truncation() {
    let db = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&db);
    engine.execute(
        "add role name; add posit [{(+a, name)}, \"Alice\", @NOW]; add posit [{(+b, name)}, \"Bob\", @NOW];",
    ).unwrap();

    let result = engine
        .execute_collect_with_options(
            "search [{(*, name)}, +name, *] return name;",
            ExecutionOptions {
                max_rows_per_search: Some(1),
                ..ExecutionOptions::default()
            },
        )
        .expect("capped query executes");

    assert_eq!(result.row_count, 1);
    assert!(result.limited);
}
