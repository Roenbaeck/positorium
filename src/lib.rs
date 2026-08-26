//! Positorium – a lightweight experimental implementation of Transitional Modeling concepts.
//!
//! Positorium centers on immutable posit propositions: an appearance set, a
//! lossless literal, and a time, with a separately assigned store-local identity.
//! The public beta surface deliberately hides keeper, index, and storage details.
//!
//! ## Modules
//! * [`datatype`] – The [`datatype::DataType`] trait plus provided concrete types
//!   (string, numeric, temporal, certainty, JSON, decimal, etc.).
//! * `storage` – private append-only framing, replay, and durability machinery.
//! * [`maintenance`] – versioned logical transfer, inspection, and physical backup.
//! * [`traqula`] – Traqula execution and structured result contracts.
//!
//! ## Data Types
//! Durable beta commands preserve complete lossless literal tokens. Physical
//! encodings remain private and never appear in query results.
//!
//! ## Persistence
//! [`Database`] owns either an ephemeral engine or one append-only
//! store, durably commits each mutation command, and rebuilds all keepers and
//! indexes through validated replay on startup.
//!
//! ## Traqula DSL
//! [`Engine`] parses and executes
//! scripts consisting of semicolon‑separated commands (e.g. `add role`,
//! `add posit`, future `search`). Grammar details live in `traqula.pest`.
//!
//! ## Quick Start
//! ```
//! use positorium::{Database, Engine, PersistenceMode};
//! let db = Database::new(PersistenceMode::InMemory).unwrap();
//! let engine = Engine::new(&db);
//! engine
//!     .execute("add role person; add posit [{(+a, person)}, \"Alice\", @NOW];")
//!     .unwrap();
//! assert!(db.contains_role("person").unwrap());
//! ```
//!
//! ## Status & Roadmap
//! This is exploratory code; the query portion (result aggregation & variable
//! binding) is still evolving. Expect API changes while the public surface is
//! being refined. Contributions around search semantics, logging, and
//! documentation are welcome.
//!
//! ## License
//! Dual licensed under Apache-2.0 and MIT (see included `LICENSE.*` files).
//!
//! ## See Also
//! * Transitional Modeling background: <http://www.anchormodeling.com/tag/transitional/>
//! * Scientific paper: <https://www.researchgate.net/publication/329352497_Modeling_Conflicting_Unreliable_and_Varying_Information>
//!
//! ---
//! Generated crate docs intentionally aggregate conceptual guidance formerly
//! kept in `main.rs` so that library consumers see them directly on docs.rs.

#[doc(hidden)]
pub mod construct;
pub mod datatype;
pub mod error;
pub mod interface;
pub mod literal;
#[cfg(feature = "persistence")]
pub mod maintenance;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "persistence")]
mod storage;
pub mod traqula;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use construct::{Database, PersistenceMode};
pub use error::{DatabaseError, Result};
pub use interface::{CancelToken, QueryHandle, QueryId, QueryInterface, QueryOptions, Row};
pub use literal::{LiteralFamily, LiteralValue};
pub use traqula::{
    CancellationToken, CollectedResult, CollectedResultSet, Engine, ExecutionMetadata,
    ExecutionOptions, ExecutionWarning, MultiStreamCallbacks, ResultCell, ResultCellKind, RowSink,
    SinkFlow,
};
