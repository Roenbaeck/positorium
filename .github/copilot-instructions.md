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
- `storage.rs`: Private append-only framing, commit, replay, locking, and recovery machinery.
- `maintenance.rs`: Validated inspection, physical backup, and versioned logical transfer.
- `traqula.rs`: Pest-based parser and execution engine for the Traqula DSL.
- `traqula.pest`: Grammar definition for the query language.
- `interface.rs`: Minimal thread-per-query interface with cooperative cancellation and optional streaming of results.
- `error.rs`: Domain-specific error types (`DatabaseError`) and conversions.
- `server.rs`: HTTP server implementation using Axum for REST API and web console.
- `benches/benchmark.rs`: Criterion-based performance benchmarks.
- `traqula-vscode/`: Syntax highlighting extension for Traqula (keep grammar in sync with `traqula.pest`).
- `positorium.html` & `positorium.css`: Web-based query console for interactive Traqula execution.

## Development Workflow
- Build: use `cargo build` (Rust edition 2024).
- Run: prefer `cargo run` (binary reads `positorium.json` and starts an HTTP server on the configured interface and port, serving the web console and REST API).
- Config (`positorium.json`):
	- `database_file_and_path`: append-only store directory (created if missing).
	- `enable_persistence`: optional `true|false`; false selects an ephemeral in-memory engine.
	- `recreate_database_on_startup`: `true|false` to remove the store directory at startup.
	- `traqula_file_to_run_on_startup`: path to a Traqula script executed on boot.
	- `listen_interface`: IP address to bind the HTTP server (default: "127.0.0.1").
	- `listen_port`: Port number for the HTTP server (default: 8080).
- Diagnostics use `tracing`; startup and boundary failures are returned rather than printed and ignored.

## Coding Patterns
- Keeper pattern: roles, appearances, appearance sets, and posits are canonicalized internally. Keep mutation helpers crate-private and route public writes through versioned Traqula execution.
- Identity management: things and posits are identities; allocate them only through the crate-private database mutation path so replay, durability, and collision checks stay coupled.
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

## Persistence Contract
- The first published backend is the versioned append-only store specified in `STORAGE.md`.
- A store directory contains `manifest.pmf`, `store.lock`, and framed `.ptl` log segments.
- Each mutation command is one logical batch terminated by a Commit record. The manifest exposes only the flushed committed prefix.
- Replay validates headers, checksums, sequence numbers, batches, canonical encodings, identity invariants, and resource bounds before rebuilding in-memory indexes.
- Physical codecs are private. Logical transfer preserves lossless literal tokens through the versioned JSONL contract in `TRANSFER.md`.
- There is no SQLite compatibility boundary, importer, dependency, or migration path: no Positorium release used the prototype backend (D031).

### Operational persistence behavior
- Startup: validated replay reconstructs keepers, lookups, and the identity generator before the engine accepts work. Corruption and unsupported formats fail closed.
- Writes: persistence appends each successful mutation command as one durable batch before publishing it to in-memory state.
- Ownership: one `Database` owns one store and one execution mutex; query interfaces sharing it serialize execution. The OS store lock rejects a second writer.

### DataType maintenance
When adding a literal family or representation:
- Preserve the exact source token and update `LiteralFamily` behavior explicitly.
- Keep storage codec identifiers private and stable; add a codec only with complete bounded decoding and replay validation.
- Update logical transfer, structured-result, comparison, property, and malformed-input tests together.

## Performance Considerations
- Roaring bitmaps enable fast set operations without exploding joins.
- Indexes maintained: role→posit, appearance_set→posit, posit→appearance_set, posit→time, plus role-name→datatype partitions.
- Candidate tracking per bound variable (value variables, time variables) is used during search.
- Avoid premature allocation by relying on `ResultSetMode` and in-place roaring operations.

## Concurrency and Interface
`interface.rs` provides a query interface with bounded execution-lock waiting, cooperative cancellation, and optional streaming via channels. `server.rs` implements an Axum HTTP/SSE boundary and the web console. All frontends sharing a `Database` use its execution owner, so separately created interfaces cannot bypass serialization.
	- File-backed and in-memory modes have the same logical execution semantics.
	- Cancellation is cooperative and must be checked at bounded points in parsing, command execution, result production, and execution-lock waiting.
	- Shutdown stops intake, finishes or cancels active work according to the boundary contract, and flushes the owned store.

## Error Handling
Domain-specific errors (`DatabaseError`) are defined in `error.rs` with variants for config, persistence, data corruption, parse, execution, invariant, and lock errors. Propagate or extend this type for new boundary failures.

## Testing and Benchmarks
- Doctests exist in several modules (run with `cargo test`).
- Use Criterion benchmarks in `benches/benchmark.rs` (`cargo bench`) for set operation performance.
- Test with various result set sizes (empty, single element, large sets).

## Contributor PR Checklist
- Builds cleanly: `cargo build` (and optionally `cargo clippy`, `cargo fmt`).
- Doctests pass: `cargo test`.
- If grammar changed: update `traqula.pest` and keep `traqula-vscode/` syntax in sync.
- If changing literal behavior: update lossless transfer, comparison, result, and malformed-input tests.
- If touching persistence: follow `STORAGE.md`; preserve atomic batches, fail-closed replay, and bounded decoding. Do not add SQLite compatibility work.
- Keepers & lookups: keep construction internal and update canonical keepers plus every dependent lookup atomically.
- Add minimal examples in docs or `traqula/example.traqula` when introducing new syntax or behavior.

## License
Dual licensed under Apache-2.0 and MIT (see `LICENSE.*`).
