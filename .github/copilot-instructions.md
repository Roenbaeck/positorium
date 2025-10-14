# Positorium AI Coding Guidelines

## Architecture Overview
Positorium is a database engine implementing Transitional Modeling concepts for handling conflicting, unreliable, and varying information over time. The core data model consists of:

- Thing: opaque u64 identity
- Role: named semantic placeholder (e.g., "wife", "name")
- Appearance: (Thing, Role) pairing
- AppearanceSet: sorted, duplicate-free set of appearances (at most one per role)
- Posit: proposition of (AppearanceSet, Value, Time) with its own Thing identity

All constructs follow a keeper pattern for canonical storage and deduplication using Arc-wrapped instances.

## Modules and Key Components
- `lib.rs`: Crate-level docs and module wiring.
- `construct.rs`: Core data structures and keepers (`Database`, `RoleKeeper`, `AppearanceKeeper`, `AppearanceSetKeeper`, `PositKeeper`), lookups, identity generator.
- `datatype.rs`: `DataType` trait and built-ins: `String`, `i64`, `Decimal`, `JSON`, `Time`, `Certainty`.
- `persist.rs`: SQLite persistence layer with schema management and (re)hydration.
- `traqula.rs`: Pest-based parser and execution engine for the Traqula DSL.
- `traqula.pest`: Grammar definition for the query language.
- `interface.rs`: Minimal thread-per-query interface with cooperative cancellation and optional streaming of results.
- `error.rs`: Domain-specific error types (`PositoriumError`) and conversions.
- `server.rs`: HTTP server implementation using Axum for REST API and web console.
- `benches/benchmark.rs`: Criterion-based performance benchmarks.
- `traqula-vscode/`: Syntax highlighting extension for Traqula (keep grammar in sync with `traqula.pest`).
- `positorium.html` & `positorium.css`: Web-based query console for interactive Traqula execution.

## Development Workflow
- Build: use `cargo build` (Rust edition 2024).
- Run: prefer `cargo run` (binary reads `positorium.json` and starts an HTTP server on the configured interface and port, serving the web console and REST API).
- Config (`positorium.json`):
	- `database_file_and_path`: SQLite file path (or create if missing).
	- `enable_persistence`: `true|false` to enable/disable file-based persistence at runtime. When false, no writes occur.
	- `recreate_database_on_startup`: `true|false` to remove the DB file at startup.
	- `traqula_file_to_run_on_startup`: path to a Traqula script executed on boot.
	- `listen_interface`: IP address to bind the HTTP server (default: "127.0.0.1").
	- `listen_port`: Port number for the HTTP server (default: 8080).
- Debug: the binary prints the integrity ledger head (superhash and posit count) after running the startup script. In debug builds, additional logging is available via tracing.

## Coding Patterns
- Keeper pattern: do not construct roles/appearances/posits directly. Always call `Database::{create_role, create_apperance, create_appearance_set, create_posit}` or the corresponding `keep_*` variants when rehydrating.
- Identity management: things and posits are identities; use `ThingGenerator` via `Database::create_thing()` or the `create_*` helpers.
- AppearanceSet ordering: maintain sorted order by `(role, thing)`; ensure at most one appearance per role (enforced by `AppearanceSet::new`).
- Data type indexing: record data types per role set in `role_name_to_data_type_lookup` to avoid runtime type probing.
- Bitmaps: use roaring bitmaps (`RoaringTreemap`) for set operations; prefer union/intersection methods over per-element loops.
- Time is built-in: every posit includes a `Time`; use constants `@NOW`, `@BOT`, `@EOT` and accepted literals (year, year-month, date, datetime).
- Hasher choice: use `SeaHasher` (`BuildHasherDefault`) for hash maps/sets of non-Thing keys to keep hashing consistent with existing lookups.

## Traqula DSL Notes
- Variable binding: `+var` declares new, `var` recalls existing, `*` is wildcard.
- Union in roles: `(w|h, name)` matches either recalled wife or husband identities.
- Pattern matching: search patterns mirror posit insertion structure.
- WHERE clauses: time-only comparisons supported with `AND` conjunctions (e.g., `t <= '1999-12-31'`).
- Result sets: engine uses tri-state `ResultSetMode` (Empty/Thing/Multi) backed by roaring bitmaps for efficient set algebra.

## Persistence Schema
- Thing(Thing_Identity)
- Role(Role_Identity, Role, Reserved)
- DataType(DataType_Identity, DataType)
- Posit(Posit_Identity, AppearanceSet, AppearingValue, ValueType_Identity, AppearanceTime)
- AppearanceSets are serialized as pipe-separated `thing_id,role_id` pairs in natural order.
- SQLite tables use `STRICT` mode; WAL is enabled when file-backed.

### Operational persistence behavior
- Startup: The SQLite database is read once to (re)hydrate the in-memory `Database` (keepers, lookups, generator). After this, the in-memory engine is the source of truth for reads.
- Writes: Persistence writes occur only on "add" operations for previously unseen constructs (roles, posits). Updates are idempotent and effectively serialized.
- Concurrency stance: Because writes are logically serialized and reads are served from memory, it is acceptable (and simpler) to keep the `Persistor` serialized behind a single owner/lock instead of making it fully `Send + Sync`.

### DataType maintenance
When adding a new `DataType` implementation:
- Pick a stable, unused numeric `UID` and name string.
- Implement `DataType::convert` for restoration from `ValueRef`.
- Extend `persist::Persistor::restore_posits` to reconstruct the value based on `DATA_TYPE`.
- Ensure the new type is inserted into the `DataType` catalog on first use (handled in `persist_posit`).

## Performance Considerations
- Roaring bitmaps enable fast set operations without exploding joins.
- Indexes maintained: role→posit, appearance_set→posit, posit→appearance_set, posit→time, plus role-name→datatype partitions.
- Candidate tracking per bound variable (value variables, time variables) is used during search.
- Avoid premature allocation by relying on `ResultSetMode` and in-place roaring operations.

## Concurrency and Interface
`interface.rs` provides a minimal query interface with cooperative cancellation and optional streaming via channels. `server.rs` implements an HTTP server using Axum for REST API endpoints and serving the web console. Recommended deployment: a single worker thread that owns the `Database` and serializes query execution. REST or other frontends enqueue scripts to this worker and optionally receive streamed results.
	- Rationale: writes are serialized anyway and reads are in-memory, so a single owner avoids `Send/Sync` requirements on the SQLite connection while preserving correctness.
	- For file-backed DBs, the `Persistor` can open a fresh SQLite connection per call (WAL enabled) when needed, but with a single worker most writes still occur sequentially.
	- For in-memory DBs, use the primary connection owned by the worker.
	- Engine cancellation is coarse (between commands); long-running commands may not be interruptible yet.

## Error Handling
Domain-specific error types (`PositoriumError`) are defined in `error.rs` with variants for config, persistence, data corruption, parse, execution, invariant, and lock errors. Conversions from `rusqlite::Error` and others are provided. Use `anyhow` for custom error handling in new code where appropriate.

## Testing and Benchmarks
- Doctests exist in several modules (run with `cargo test`).
- Use Criterion benchmarks in `benches/benchmark.rs` (`cargo bench`) for set operation performance.
- Test with various result set sizes (empty, single element, large sets).

## Contributor PR Checklist
- Builds cleanly: `cargo build` (and optionally `cargo clippy`, `cargo fmt`).
- Doctests pass: `cargo test`.
- If grammar changed: update `traqula.pest` and keep `traqula-vscode/` syntax in sync.
- If adding a `DataType`: update `persist::restore_posits` and ensure stable `UID`/`DATA_TYPE`.
- If touching persistence: consider schema migrations; keep `STRICT` and uniqueness constraints intact.
- Keepers & lookups: always use `Database::create_*`/`keep_*` and update lookups consistently.
- Add minimal examples in docs or `traqula/example.traqula` when introducing new syntax or behavior.

## License
Dual licensed under Apache-2.0 and MIT (see `LICENSE.*`).