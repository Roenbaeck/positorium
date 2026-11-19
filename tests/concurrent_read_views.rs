use positorium::{
    Database, DatabaseError, Engine, ExecutionOptions, MultiStreamCallbacks, PersistenceMode,
    PlanOperator, ResultCell, RowSink, SinkFlow,
};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);
const STATUS: &str = "search [{(*, status)}, ?value, *] return ?value;";
const EFFECTIVE: &str =
    "search [{(*, status)}, ?value, *] in effect '2025-01-01', '2025-01-01' return ?value;";

fn bounded() -> ExecutionOptions {
    ExecutionOptions {
        timeout: Some(WAIT),
        ..ExecutionOptions::default()
    }
}

#[test]
fn a_stream_keeps_its_view_across_searches_while_reads_and_writes_finish() {
    struct Stream {
        paused: SyncSender<()>,
        resume: Receiver<()>,
        sets: Vec<Vec<String>>,
    }

    impl MultiStreamCallbacks for Stream {
        fn on_result_set_start(&mut self, index: usize, _: &[String], _: &str) {
            assert_eq!(index, self.sets.len());
            self.sets.push(Vec::new());
        }

        fn on_row(&mut self, index: usize, row: Vec<ResultCell>) -> bool {
            self.sets[index].push(row[0].text.clone());
            true
        }

        fn on_result_set_end(&mut self, index: usize, _: usize, _: bool) {
            if index == 0 {
                self.paused.send(()).unwrap();
                self.resume.recv_timeout(WAIT * 2).unwrap();
            }
        }
    }

    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    engine
        .execute(
            "add role status;
             add posit [{(+case, status)}, 1, '2024-01-01']
                and assert by +source with 80% at '2024-02-01';",
        )
        .unwrap();

    let (paused, paused_rx) = sync_channel(1);
    let (resume_tx, resume) = sync_channel(1);
    thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut stream = Stream {
                paused,
                resume,
                sets: Vec::new(),
            };
            engine
                .execute_stream_multi(
                    &format!("{STATUS} {STATUS} search [*, ?v, *] return ?v; {EFFECTIVE}"),
                    &mut stream,
                )
                .unwrap();
            stream.sets
        });

        paused_rx.recv_timeout(WAIT).unwrap();
        assert_eq!(
            engine
                .execute_collect_with_options(STATUS, bounded())
                .unwrap()
                .row_count,
            1
        );
        engine
            .execute_collect_with_options(
                "add posit [{(+case, status)}, 2, '2024-01-01']
                    and assert by +source with 90% at '2024-02-01';",
                bounded(),
            )
            .unwrap();
        assert_eq!(
            engine
                .execute_collect_with_options(EFFECTIVE, bounded())
                .unwrap()
                .row_count,
            2
        );
        resume_tx.send(()).unwrap();
        assert_eq!(
            reader.join().unwrap(),
            [vec!["1"], vec!["1"], vec!["1", "80%"], vec!["1"],]
        );
    });
}

#[test]
fn a_mixed_script_sees_its_writes_and_publishes_completed_commands_on_error() {
    struct PauseAtRow(SyncSender<String>, Receiver<()>);

    impl RowSink for PauseAtRow {
        fn push(&mut self, row: Vec<ResultCell>) -> SinkFlow {
            self.0.send(row[0].text.clone()).unwrap();
            self.1.recv_timeout(WAIT * 2).unwrap();
            SinkFlow::Continue
        }
    }

    let database = Database::new(PersistenceMode::InMemory).unwrap();
    let engine = Engine::new(&database);
    let (paused, paused_rx) = sync_channel(1);
    let (resume_tx, resume) = sync_channel(1);
    thread::scope(|scope| {
        let writer = scope.spawn(|| {
            engine.execute_stream_single(
                "add role status;
                 add posit [{(+case, status)}, 1, '2024'];
                 search [{(*, status)}, ?value, *] return ?value;
                 add posit [{(case, status)}, 2, '2025'],
                           [{(case, unknown_role)}, 3, '2025'];",
                &mut PauseAtRow(paused, resume),
            )
        });
        assert_eq!(paused_rx.recv_timeout(WAIT).unwrap(), "1");

        assert!(!database.contains_role("status").unwrap());
        assert_eq!(
            engine
                .execute_collect_with_options(STATUS, bounded())
                .unwrap()
                .row_count,
            0
        );
        let explain = "explain search [{(?case, status)}, 1, *]
                       or add posit [{(+case, status)}, 1, '2024'];";
        assert!(matches!(
            engine
                .explain_with_options(explain, bounded())
                .unwrap()
                .root
                .operation,
            PlanOperator::SearchOrAdd { .. }
        ));
        assert_eq!(
            engine
                .execute_collect_with_options(explain, bounded())
                .unwrap()
                .row_count,
            1
        );

        resume_tx.send(()).unwrap();
        assert!(matches!(
            writer.join().unwrap(),
            Err(DatabaseError::UnknownRole(_))
        ));
        assert!(database.contains_role("status").unwrap());
        let result = engine.execute_collect(STATUS).unwrap();
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0][0].text, "1");
    });
}
