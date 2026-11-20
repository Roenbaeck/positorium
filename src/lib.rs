//! Embed Positorium in a Rust application to store and query immutable claims using Traqula.
//!
//! [`Database::memory`] creates an ephemeral database. With the `persistence` Cargo
//! feature, `Database::open(path)` creates or reopens an append-only store directory.
//!
//! ```
//! use positorium::{Database, ExecutionOptions, Result, ResultCellKind};
//!
//! fn main() -> Result<()> {
//!     let db = Database::memory()?;
//!     db.execute("add role name;")?;
//!     let set = db.execute_one_with_options(
//!         "add posit [{(+p, name)}, $name, @NOW]; \
//!          search [{(?p, name)}, ?n, *] return ?p, ?n;",
//!         ExecutionOptions::default().bind("name", "Ada"),
//!     )?;
//!     let name = set.cell(0, "n").unwrap();
//!     assert_eq!(name.kind, ResultCellKind::Literal);
//!     assert_eq!(name.as_str(), r#""Ada""#);
//!     Ok(())
//! }
//! ```
//!
//! [`Database::execute`] returns ordered result sets. [`Database::execute_one`]
//! expects a single projection, which may have any number of rows. [`ResultCell`]
//! carries the kind and exact text of a value, including numeric scale and time precision.
//! [`ExecutionOptions`] accepts parameters, a fixed clock, a cancellation token and
//! query budgets. [`Engine`] also supports streaming results and explaining searches.
//!
//! For a durable embedded dependency, disable default features and select `persistence`.
//! For memory-only use, disable default features. The defaults also build the HTTP service
//! and the command-line tools.
//! [Embedding guide](https://github.com/Roenbaeck/positorium/blob/master/docs/guides/RUST.md)
//! · [Traqula reference](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/TRAQULA.md)

#[doc(hidden)]
pub mod construct;
pub mod datatype;
pub mod effect;
pub mod error;
pub mod interface;
pub mod literal;
#[cfg(feature = "persistence")]
pub mod maintenance;
#[cfg(feature = "python")]
mod python;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "persistence")]
mod storage;
pub mod terrain;
pub mod traqula;
#[cfg(feature = "wasm")]
pub mod wasm;

pub use construct::{Database, PersistenceMode};
pub use effect::{
    EffectCounters, EffectCut, EffectLimits, EffectiveAssertion, EffectiveSlice,
    MAX_EFFECT_ASSERTION_CANDIDATES, MAX_EFFECT_TEMPORAL_COMPARISONS,
};
pub use error::{DatabaseError, Result};
pub use interface::{CancelToken, QueryHandle, QueryId, QueryInterface, QueryOptions, Row};
pub use literal::{LiteralFamily, LiteralValue};
#[cfg(feature = "python")]
pub use python::PYTHON_INTERFACE_VERSION;
pub use terrain::{
    DEFAULT_PROJECTED_ROLE_LIMIT, MAX_PROJECTED_ROLE_LIMIT, TERRAIN_VERSION, TerrainAllocation,
    TerrainDatabaseTotals, TerrainFrame, TerrainFrameStats, TerrainFrames, TerrainIsopleth,
    TerrainLimits, TerrainOptions, TerrainProfile, TerrainProjectedRole, TerrainProjection,
    TerrainRelationship, TerrainRelationshipCatalog, TerrainRelationshipSignature, TerrainReport,
    TerrainRoleRef, TerrainRoleSupport, TerrainRoleTotal, TerrainScope,
};
pub use traqula::{
    CONSTRAINT_VERSION, CancellationToken, CollectedResult, CollectedResultSet, Engine,
    ExecutionMetadata, ExecutionOptions, ExecutionParameter, ExecutionWarning,
    MultiStreamCallbacks, PlanAccess, PlanBinding, PlanCutoff, PlanDomain, PlanNode, PlanOperator,
    PlanOrder, QUERY_PLAN_VERSION, QueryLimits, QueryPlan, ResultCell, ResultCellKind, RowSink,
    SinkFlow, TRAQULA_VERSION,
};
