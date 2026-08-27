//! Asynchronous/Threaded interface for submitting and controlling Traqula queries.
//!
//! This module provides a minimal, thread-per-query runner that accepts Traqula
//! scripts, executes them on a background thread, and optionally streams results
//! back to the caller. It uses cooperative cancellation via an `Arc<AtomicBool>`.
//!
//! The goal is to keep threading concerns here without invasive changes to the
//! engine. Callers can submit queries and cancel them by id.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::construct::Database;
use crate::error::{DatabaseError, Result};
use crate::terrain::{TerrainOptions, TerrainReport};
use crate::traqula::{
    CollectedResult, CollectedResultSet, Engine, ExecutionOptions, MultiStreamCallbacks,
    ResultCell, RowSink,
};

/// A structured row emitted by the engine.
#[derive(Debug, Clone)]
pub struct Row(pub Vec<ResultCell>);

/// Cancellation token shared with the worker thread.
pub use crate::traqula::CancellationToken as CancelToken;

/// Opaque query identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QueryId(u64);
impl QueryId {
    pub fn value(&self) -> u64 {
        self.0
    }
}

/// Handle to a running or completed query.
pub struct QueryHandle {
    pub id: QueryId,
    cancel: CancelToken,
    started: Instant,
    join: Option<JoinHandle<Result<()>>>,
    pub results: Option<Receiver<Row>>, // None when result streaming was not requested
}
impl QueryHandle {
    /// Request cancellation (cooperative). The worker may take a short time to observe it.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    /// Wait for the query to finish.
    pub fn join(mut self) -> Result<()> {
        if let Some(j) = self.join.take() {
            return j.join().map_err(|_| {
                DatabaseError::Execution("query worker thread panicked".to_string())
            })?;
        }
        Ok(())
    }
    /// Elapsed time since start.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }
}

/// Query submission options.
pub struct QueryOptions {
    pub stream_results: bool,
    pub timeout: Option<Duration>,
}
impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            stream_results: true,
            timeout: None,
        }
    }
}

/// Registry managing query lifecycles.
pub struct QueryInterface {
    db: Arc<Database>, // shared database
    next_id: Mutex<u64>,
    active: Arc<Mutex<HashMap<QueryId, CancelToken>>>, // for external cancellation
}

impl QueryInterface {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            next_id: Mutex::new(0),
            active: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Build an authoritative Terrain report from one coherent database capture.
    pub fn terrain_with_options(&self, options: TerrainOptions) -> Result<TerrainReport> {
        self.db.terrain_with_options(options)
    }

    fn allocate_id(&self) -> Result<QueryId> {
        let mut next = self
            .next_id
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        *next = next
            .checked_add(1)
            .ok_or_else(|| DatabaseError::Invariant("query identifier exhausted".to_string()))?;
        Ok(QueryId(*next))
    }

    /// Submit a Traqula script for execution on a background thread.
    /// When `options.stream_results` is true, a channel is returned for rows.
    pub fn start_query(&self, script: String, options: QueryOptions) -> Result<QueryHandle> {
        let id = self.allocate_id()?;
        let cancel = CancelToken::new();
        self.active
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .insert(id, cancel.clone());

        // Optional results channel (not currently used by Engine which prints directly)
        let (tx, rx) = if options.stream_results {
            let (tx, rx) = mpsc::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        // Execute on a background thread; the database owner serializes scripts.
        let db = Arc::clone(&self.db);
        let cancel_for_thread = cancel.clone();
        let active = Arc::clone(&self.active);
        let timeout = options.timeout;
        let worker = std::thread::Builder::new()
            .name(format!("positorium-query-{}", id.value()))
            .spawn(move || {
                let execution_result = Engine::new(&db).execute_collect_multi_with_options(
                    &script,
                    ExecutionOptions {
                        timeout,
                        cancellation: Some(cancel_for_thread),
                        ..ExecutionOptions::default()
                    },
                );
                if let (Ok(result_sets), Some(sender)) = (&execution_result, tx.as_ref()) {
                    for row in result_sets
                        .iter()
                        .flat_map(|result_set| result_set.rows.iter())
                    {
                        if sender.send(Row(row.clone())).is_err() {
                            break;
                        }
                    }
                }
                let cleanup_result = active
                    .lock()
                    .map_err(|error| DatabaseError::Lock(error.to_string()))
                    .map(|mut active| {
                        active.remove(&id);
                    });
                drop(tx);
                execution_result.and(cleanup_result)
            })
            .map_err(|error| {
                DatabaseError::Execution(format!("failed to spawn query worker: {error}"))
            });
        let join = match worker {
            Ok(join) => join,
            Err(error) => {
                self.active
                    .lock()
                    .map_err(|lock_error| DatabaseError::Lock(lock_error.to_string()))?
                    .remove(&id);
                return Err(error);
            }
        };

        Ok(QueryHandle {
            id,
            cancel,
            started: Instant::now(),
            join: Some(join),
            results: rx,
        })
    }

    /// Run a Traqula script synchronously on the current thread.
    ///
    /// This is appropriate for one-off startup scripts and embedded callers that
    /// already execute on the database owner's thread.
    pub fn run_sync(&self, script: &str) -> Result<()> {
        Engine::new(&self.db)
            .execute_collect_multi(script)
            .map(|_| ())
    }

    /// Execute a single-result script while holding the database owner lock.
    pub fn execute_collect_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<CollectedResult> {
        Engine::new(&self.db).execute_collect_with_options(script, options)
    }

    /// Execute a multi-result script while holding the database owner lock.
    pub fn execute_collect_multi_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<Vec<CollectedResultSet>> {
        Engine::new(&self.db).execute_collect_multi_with_options(script, options)
    }

    /// Stream one result set while holding the database owner lock.
    pub fn execute_stream_single_with_options<S: RowSink>(
        &self,
        script: &str,
        sink: &mut S,
        options: ExecutionOptions,
    ) -> Result<(Vec<String>, bool, usize)> {
        Engine::new(&self.db).execute_stream_single_with_options(script, sink, options)
    }

    /// Stream multiple result sets while holding the database owner lock.
    pub fn execute_stream_multi_with_options<C: MultiStreamCallbacks>(
        &self,
        script: &str,
        callbacks: &mut C,
        options: ExecutionOptions,
    ) -> Result<()> {
        Engine::new(&self.db).execute_stream_multi_with_options(script, callbacks, options)
    }

    /// Cancel a query by id.
    pub fn cancel(&self, id: QueryId) -> Result<bool> {
        if let Some(tok) = self
            .active
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?
            .get(&id)
        {
            tok.cancel();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_execution_errors_are_returned_by_join() {
        let db = Arc::new(Database::new(crate::construct::PersistenceMode::InMemory).unwrap());
        let interface = QueryInterface::new(db);
        let handle = interface
            .start_query("this is not Traqula;".to_string(), QueryOptions::default())
            .unwrap();
        let id = handle.id;

        assert!(matches!(handle.join(), Err(DatabaseError::Parse { .. })));
        assert!(!interface.cancel(id).unwrap());
    }

    #[test]
    fn timeout_while_waiting_for_database_owner_prevents_mutation() {
        let db = Arc::new(Database::new(crate::construct::PersistenceMode::InMemory).unwrap());
        let first = QueryInterface::new(Arc::clone(&db));
        let second = QueryInterface::new(Arc::clone(&db));
        let owner = db.execution_owner.lock().unwrap();

        let first_handle = first
            .start_query(
                "add role first_never_created;".to_string(),
                QueryOptions {
                    timeout: Some(Duration::from_millis(10)),
                    ..QueryOptions::default()
                },
            )
            .unwrap();
        let second_handle = second
            .start_query(
                "add role second_never_created;".to_string(),
                QueryOptions {
                    timeout: Some(Duration::from_millis(10)),
                    ..QueryOptions::default()
                },
            )
            .unwrap();

        assert!(matches!(first_handle.join(), Err(DatabaseError::Timeout)));
        assert!(matches!(second_handle.join(), Err(DatabaseError::Timeout)));
        drop(owner);

        let roles = db.role_keeper.lock().unwrap();
        assert!(roles.get("first_never_created").is_none());
        assert!(roles.get("second_never_created").is_none());
    }

    #[test]
    fn cancellation_is_observed_while_waiting_for_database_owner() {
        let db = Arc::new(Database::new(crate::construct::PersistenceMode::InMemory).unwrap());
        let interface = QueryInterface::new(Arc::clone(&db));
        let owner = db.execution_owner.lock().unwrap();
        let handle = interface
            .start_query(
                "add role cancelled_never_created;".to_string(),
                QueryOptions::default(),
            )
            .unwrap();

        handle.cancel();
        assert!(matches!(handle.join(), Err(DatabaseError::Cancelled)));
        drop(owner);
        assert!(
            db.role_keeper
                .lock()
                .unwrap()
                .get("cancelled_never_created")
                .is_none()
        );
    }

    #[test]
    fn concurrent_clients_are_serialized_without_losing_commands() {
        let db = Arc::new(Database::new(crate::construct::PersistenceMode::InMemory).unwrap());
        let interface = QueryInterface::new(Arc::clone(&db));
        let handles = (0..32)
            .map(|index| {
                interface
                    .start_query(
                        format!("add role concurrent_{index};"),
                        QueryOptions {
                            stream_results: false,
                            timeout: Some(Duration::from_secs(2)),
                        },
                    )
                    .unwrap()
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.join().unwrap();
        }
        for index in 0..32 {
            assert!(db.contains_role(&format!("concurrent_{index}")).unwrap());
        }
    }
}
