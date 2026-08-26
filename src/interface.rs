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
use crate::traqula::{
    CollectedResult, CollectedResultSet, Engine, ExecutionOptions, MultiStreamCallbacks, RowSink,
};

/// A single row emitted by the engine. For now it's just a line of text (stdout-compatible).
/// This can be evolved into a structured enum once projection returns tuples.
#[derive(Debug, Clone)]
pub struct Row(pub String);

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
    join: Option<JoinHandle<()>>,
    pub results: Option<Receiver<Row>>, // None when sink is stdout
}
impl QueryHandle {
    /// Request cancellation (cooperative). The worker may take a short time to observe it.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    /// Wait for the query to finish.
    pub fn join(mut self) {
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
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
    execution: Arc<Mutex<()>>,
}

impl QueryInterface {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            next_id: Mutex::new(0),
            active: Arc::new(Mutex::new(HashMap::new())),
            execution: Arc::new(Mutex::new(())),
        }
    }

    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    fn allocate_id(&self) -> QueryId {
        let mut g = self.next_id.lock().unwrap();
        *g += 1;
        QueryId(*g)
    }

    /// Submit a Traqula script for execution on a background thread.
    /// When `options.stream_results` is true, a channel is returned for rows.
    pub fn start_query(&self, script: String, options: QueryOptions) -> QueryHandle {
        let id = self.allocate_id();
        let cancel = CancelToken::new();
        self.active.lock().unwrap().insert(id, cancel.clone());

        // Optional results channel (not currently used by Engine which prints directly)
        let (tx, rx) = if options.stream_results {
            let (tx, rx) = mpsc::channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        // Execute on a background thread; Persistor performs serialized writes internally.
        let db = Arc::clone(&self.db);
        let cancel_for_thread = cancel.clone();
        let active = Arc::clone(&self.active);
        let execution = Arc::clone(&self.execution);
        let timeout = options.timeout;
        let join = std::thread::spawn(move || {
            if let Ok(_owner) = execution.lock() {
                let engine = Engine::new(&db);
                let _ = engine.execute_collect_multi_with_options(
                    &script,
                    ExecutionOptions {
                        timeout,
                        cancellation: Some(cancel_for_thread),
                        ..ExecutionOptions::default()
                    },
                );
            }
            if let Ok(mut active) = active.lock() {
                active.remove(&id);
            }
            let _ = tx; // placeholder to avoid unused warning when not streaming
        });

        QueryHandle {
            id,
            cancel,
            started: Instant::now(),
            join: Some(join),
            results: rx,
        }
    }

    /// Run a Traqula script synchronously on the current thread.
    ///
    /// This avoids any `Send`/`Sync` constraints on the underlying database/persistence
    /// and is appropriate for one-off startup scripts or environments using in-memory
    /// SQLite where cross-thread connections are not viable.
    pub fn run_sync(&self, script: &str) {
        let Ok(_owner) = self.execution.lock() else {
            return;
        };
        let engine = Engine::new(&self.db);
        engine.execute(script);
    }

    /// Execute a single-result script while holding the database owner lock.
    pub fn execute_collect_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<CollectedResult> {
        let _owner = self
            .execution
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        Engine::new(&self.db).execute_collect_with_options(script, options)
    }

    /// Execute a multi-result script while holding the database owner lock.
    pub fn execute_collect_multi_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<Vec<CollectedResultSet>> {
        let _owner = self
            .execution
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        Engine::new(&self.db).execute_collect_multi_with_options(script, options)
    }

    /// Stream one result set while holding the database owner lock.
    pub fn execute_stream_single_with_options<S: RowSink>(
        &self,
        script: &str,
        sink: &mut S,
        options: ExecutionOptions,
    ) -> Result<(Vec<String>, bool, usize)> {
        let _owner = self
            .execution
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        Engine::new(&self.db).execute_stream_single_with_options(script, sink, options)
    }

    /// Stream multiple result sets while holding the database owner lock.
    pub fn execute_stream_multi_with_options<C: MultiStreamCallbacks>(
        &self,
        script: &str,
        callbacks: &mut C,
        options: ExecutionOptions,
    ) -> Result<()> {
        let _owner = self
            .execution
            .lock()
            .map_err(|error| DatabaseError::Lock(error.to_string()))?;
        Engine::new(&self.db).execute_stream_multi_with_options(script, callbacks, options)
    }

    /// Cancel a query by id.
    pub fn cancel(&self, id: QueryId) -> bool {
        if let Some(tok) = self.active.lock().unwrap().get(&id) {
            tok.cancel();
            true
        } else {
            false
        }
    }
}
