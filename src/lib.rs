//! Positorium – a lightweight experimental implementation of Transitional Modeling concepts.
//!
//! Positorium centers on the *posit* concept: a proposition of the form
//! `(AppearanceSet, Value, Time)`, where:
//! * A [`construct::Thing`] is an opaque identity (a simple `u64`).
//! * A [`construct::Role`] names a semantic placeholder a thing can occupy.
//! * An [`construct::Appearance`] pairs a thing with a role.
//! * An [`construct::AppearanceSet`] is a duplicate‑free, role‑unique set of appearances.
//! * A [`construct::Posit`] couples an appearance set with a typed value (`V: DataType`) and time (`Time`).
//!
//! These core constructs are owned and deduplicated by "keeper" structures (see
//! the `construct` module) enabling canonical sharing through `Arc` while
//! providing efficient lookup indexes for query evaluation.
//!
//! ## Modules
//! * [`construct`] – Fundamental identity / posit building blocks and keepers.
//! * [`datatype`] – The [`datatype::DataType`] trait plus provided concrete types
//!   (string, numeric, temporal, certainty, JSON, decimal, etc.).
//! * [`persist`] – SQLite persistence & restoration layer.
//! * [`traqula`] – A minimal DSL (parser + engine) for adding roles, posits and performing searches.
//!
//! ## Data Types
//! Any type implementing [`datatype::DataType`] can be used as the value in a posit.
//! Built‑ins demonstrate patterns for stable identifiers (`UID`) and constant
//! `DATA_TYPE` strings enabling heterogeneous indexing.
//!
//! ## Persistence
//! The [`persist::Persistor`] encapsulates SQLite schema creation and durable
//! storage for things, roles and posits. The [`construct::Database`] wires a
//! persistor together with in‑memory keepers and restores prior state on startup.
//!
//! ## Traqula DSL
//! The `traqula` module exposes an [`traqula::Engine`] capable of parsing simple
//! scripts consisting of semicolon‑separated commands (e.g. `add role`,
//! `add posit`, future `search`). Grammar details live in `traqula.pest`.
//!
//! ## Quick Start
//! ```
//! use positorium::{construct::{Database, PersistenceMode}, traqula::Engine};
//! let db = Database::new(PersistenceMode::InMemory).unwrap();
//! let engine = Engine::new(&db);
//! engine.execute("add role person; add posit [{(+a, person)}, \"Alice\", @NOW];");
//! assert!(db.role_keeper().lock().unwrap().len() >= 1);
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

pub mod construct;
pub mod datatype;
pub mod interface;
#[cfg(feature = "persistence")]
pub mod persist;
pub mod traqula;
pub mod error;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "wasm")]
pub mod wasm;
