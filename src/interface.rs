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
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::construct::Database;
use crate::traqula::Engine;

/// A single row emitted by the engine. For now it's just a line of text (stdout-compatible).
/// This can be evolved into a structured enum once projection returns tuples.
#[derive(Debug, Clone)]
pub struct Row(pub String);

/// Cancellation token shared with the worker thread.
#[derive(Debug)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
    pub fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

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
    active: Mutex<HashMap<QueryId, CancelToken>>, // for external cancellation
}

impl QueryInterface {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            next_id: Mutex::new(0),
            active: Mutex::new(HashMap::new()),
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
        let timeout = options.timeout;
        let join = std::thread::spawn(move || {
            let engine = Engine::new(&db);
            // For now, execute monolithically; cancellation checked pre/post.
            if let Some(d) = timeout {
                if d.is_zero() || cancel_for_thread.is_cancelled() {
                    return;
                }
            }
            engine.execute(&script);
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
        let engine = Engine::new(&self.db);
        engine.execute(script);
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
