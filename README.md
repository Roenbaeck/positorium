<img src="./positorium.svg" width="333">
<p/>

Positorium is an experimental database engine based on Transitional Modeling, designed to capture conflicting, unreliable, and varying information over time. It blends ideas from relational, graph, columnar, and key–value stores.

- [The Philosophical Foundations of Positorium](THEORY.md)
- [Paper: Modeling Conflicting, Unreliable, and Varying Information](https://www.researchgate.net/publication/329352497_Modeling_Conflicting_Unreliable_and_Varying_Information)

## Why Positorium?

Most databases assume a single, consistent truth. In reality, facts are messy: they conflict across sources, change over time, and sometimes carry uncertainty. Positorium treats this as a first‑class concern:

- Contradictions are preserved, not overwritten (assertions can be affirmed or negated with certainty).
- Time is built into every posit, so “what was true when” is natural to ask.
- Set‑based evaluation with roaring bitmaps keeps pattern matching fast without exploding joins.

This makes Positorium well‑suited for master data management, regulated domains, investigations/intel, and any workflow where evidence accumulates and is revised.

<br/>

## Traqula DSL

Traqula is Positorium's domain-specific language for defining roles, positing facts with time, and querying data through pattern matching. Query variables are lexical to one `search`; allocation bindings remain available only to later mutation commands in the same script.

For the complete language reference, see [TRAQULA.md](TRAQULA.md). Worked
patterns for correction, disagreement, assertions, external identification,
multi-valued attributes, repeated participants, backup, and transfer are in
[COOKBOOK.md](COOKBOOK.md).

The normative design for the append-only beta store is in [STORAGE.md](STORAGE.md).
File-backed mode uses that framed, checksummed, append-only format. The
unpublished SQLite prototype and its dependency have been removed; the first
append-only beta format begins the compatibility window.

The storage- and language-independent identity, value-slot, temporal, snapshot,
and external-identification contract is in [MODEL.md](MODEL.md).

## Build and run

Prerequisite: rustup. The repository selects the stable Rust
toolchain declared in `rust-toolchain.toml`.

Build:

```sh
cargo build
```

Run the binary; it reads `positorium.json` and executes its configured Traqula
startup script:

```sh
target/debug/positorium
```

Config (positorium.json):

```json
{
	"listen_interface": "127.0.0.1",
	"listen_port": 8080,
	"database_file_and_path": "positorium.store",
	"enable_persistence": true,
	"recreate_database_on_startup": false,
	"traqula_file_to_run_on_startup": "traqula/adds.traqula"
}
```

## Initialization Modes

The engine uses an explicit persistence mode enum:

```rust
use positorium::{Database, PersistenceMode};

// Ephemeral: nothing is written, all data lost when process exits
let db = Database::new(PersistenceMode::InMemory);

// File-backed persistence (creates or reuses an append-only store directory)
let db = Database::new(PersistenceMode::File("positorium.store".to_string()));

// Derive from config style flags
let enable = true; // imagine read from config
let mode = PersistenceMode::from_config(enable, "positorium.store");
let db2 = Database::new(mode);
```

When running the provided binary, the `enable_persistence` flag in `positorium.json` selects between these modes internally.

## Store integrity

File-backed stores frame every record with CRC32C, monotonic sequence numbers,
atomic commit frames, and a manifest-recorded committed length. Startup rejects
committed corruption and writable recovery truncates only an uncommitted tail.
This detects corruption; it is not a tamper-proof audit mechanism.

## Store inspection, backup, and logical transfer

The native maintenance binary validates stores under a read lock and prints JSON
reports:

```bash
cargo run --bin positorium-store -- inspect positorium.store
cargo run --bin positorium-store -- backup positorium.store backup.store
cargo run --bin positorium-store -- dump positorium.store export.jsonl
cargo run --bin positorium-store -- import export.jsonl imported.store remap.json
```

Physical backup excludes uncommitted tail bytes without changing the source.
Logical import creates a new store UUID and emits the complete identity remap.
The versioned formats and failure rules are specified in [TRANSFER.md](TRANSFER.md).
Independent beta boundary versions are listed in [CONTRACTS.md](CONTRACTS.md).
Operational startup, durability, recovery, and resource-limit procedures are in
[OPERATIONS.md](OPERATIONS.md).
The repeatable architecture benchmark and current indicative baseline are in
[BENCHMARKS.md](BENCHMARKS.md).

## Client / Server Architecture

Positorium can run as a library or an HTTP server. The server layer (Axum + Tokio) exposes a JSON endpoint:

`POST /v1/query`

Request body:
```jsonc
{ "traqula_version": 1, "script": "search [{(*, name), ...}, ?n, *] return ?n;", "stream": false, "timeout_ms": 5000 }
```

Response (single result set):
```jsonc
{
	"api_version": "v1",
	"traqula_version": 1,
	"id": 0,
	"status": "ok",
	"elapsed_ms": 1.23,
	"columns": ["n"],
	"row_count": 2,
	"limited": false,
	"rows": [
		[{"kind":"literal","text":"\"Alice\""}],
		[{"kind":"literal","text":"\"Bob\""}]
	]
}
```

If the script contains multiple `search` commands, the response omits top-level `columns/rows` and instead returns `result_sets` (array of result set objects) with cumulative `row_count`.

The beta HTTP service is a trusted local interface, not an Internet-facing API.
It binds to `127.0.0.1` by default, permits no cross-origin browser access by
default, and only accepts explicitly configured exact loopback CORS origins.
Binding to a non-loopback address does not add authentication or make the
service safe for public exposure.

Requests default to a 1 MiB body limit and a five-second execution deadline.
Configuration may raise those limits only as far as the 16 MiB and 30-second
hard caps. A request's `timeout_ms` can lower, but cannot raise, the configured
deadline. Scripts are limited to 1,000 commands and each search to 100,000 rows;
buffered responses and versioned SSE completion events report whether rows were
actually truncated. Scripts submitted through the HTTP interface execute
serially against one database owner.

### Starting the server

You can run the server directly with the binary or use the convenience scripts provided for different platforms.

Windows (PowerShell):
```powershell
. .\scripts\positorium.ps1                  # dot-source to load functions
Start-Positorium -LogProfile normal -Tail   # run and stream logs live
Stop-Positorium                             # stop
Restart-Positorium -LogProfile verbose      # restart with different profile
```

macOS / Linux (bash):
```bash
chmod +x scripts/positorium.sh            # first time
./scripts/positorium.sh start --profile normal --tail   # foreground (logs to console)
./scripts/positorium.sh stop
./scripts/positorium.sh restart --profile verbose --force-rebuild
./scripts/positorium.sh start --log 'warn,positorium=info'  # custom RUST_LOG filter
./scripts/positorium.sh tail               # follow log file if started in background
```

Both scripts support a common set of logging profiles mapped to `RUST_LOG`:

LogProfile | RUST_LOG
:--|:--
quiet | `error`
normal | `info`
verbose | `debug,positorium=info`
trace | `trace`

You can override the profile with an explicit `--log` / `-Log` argument (EnvFilter syntax) such as `warn,axum=info,positorium=debug`.

The bash script maintains a PID file at `.positorium.pid` and writes background logs to `positorium.out`; use `--tail` (bash) or `-Tail` (PowerShell) to stream logs directly instead.

Logging uses `tracing` with `RUST_LOG` filtering.

### Web UI (positorium.html)

A focused static query studio (`positorium.html`) supports composing Traqula scripts, submitting them to the server or local WASM engine, and inspecting table, JSON, and activity views. Query/Results and Terrain are alternate workspaces, while the endpoint and Local WASM execution mode live under the header settings button. Both settings persist in browser `localStorage`. Open the studio in a browser or host it, then point the endpoint setting to your server's `/v1/query` URL; Terrain derives the sibling `/v1/terrain` endpoint automatically.

The repository does not contain generated `pkg/` artifacts. On a loopback development
server such as VS Code Live Preview, Local WASM prefers a workspace `pkg/` build and
falls back to the compatible package published on GitHub Pages. To test local Rust
changes instead, generate the workspace package before starting the preview:

```text
wasm-pack build --release --target web --out-dir pkg . --no-default-features --features wasm
```

Query Studio has an independent beta SemVer in the `studioVersion` element in
`positorium.html`. Increment it when the console's behavior or published assets
change. The version is visible in the header; the browser remembers the last-seen
version for that origin and reports upgrades in the Activity view.

The Terrain tab automatically requests an authoritative structural report from
Rust—through `POST /v1/terrain` or `WasmEngine.terrain(...)`—so it requires no
preparatory Traqula query and is not affected by query row limits or streaming.
History and Current share one Role projection and relationship catalog; the
browser retains ownership of SVG layout, filtering, selection, and prepared
queries. Refresh, stale, and error states are explicit, and a failed refresh
keeps the previous complete snapshot visible. The versioned report contract,
semantics, limits, interfaces, and golden backend fixture are documented in
[TERRAIN.md](TERRAIN.md).

## Updated Status and Roadmap

Implemented:
* Immutable core constructs, exact literal tokens, and precision-aware time
* Framed append-only persistence with atomic durable command batches, hidden
  lossless codecs, deterministic replay, recovery, backup, and logical transfer
* Declarative Traqula joins, union, safe `not exists`, snapshots, typed
  comparisons and parameters, `distinct`, ordering, and limits
* Structured native, HTTP/SSE, and WASM results with cooperative limits and cancellation
* Trusted/local Axum service, browser testbed, lifecycle scripts, and synchronized editor grammar

Planned/next:
* WHERE enhancements: OR, grouping, BETWEEN, IN
* Aggregations and tuple-shaped / structured returns
* Projection type annotations stabilization (avoid dynamic probing)
* Authentication / access control for a future non-local server posture
* Optimization: caching value extraction during predicate evaluation
* Optional CSV-oriented export helpers beyond the stable JSONL logical transfer

## Long-term Goals

These are aspirational features that align with the full vision of Transitional Modeling, extending Positorium beyond its current experimental state:

* **Advanced Query Capabilities**: Implement all theoretical query types from Transitional Modeling, including probabilistic searches (e.g., "find facts with at least 75% certainty"), audit trails (e.g., "show all corrections between dates"), and log-like queries (e.g., "all model changes by a specific identity").
* **Classification Presentation**: Use the reserved `thing`, `class`, and
  `subclass` vocabulary in explicit query and visualization policies while
  keeping lifecycle values neutral in storage. Terrain will initially shade one
  selected direct class at a time.
* **Constraint Research**: Define reproducible, versioned constraint programs
  over immutable effective snapshots before adding enforcement or specialized
  cardinality syntax.
* **Multi-tenant and Collaborative Features**: Enhance multi-tenant support for disagreements and consensus tracking, allowing collaborative modeling where different observers can maintain concurrent, conflicting models.
* **Uncertainty Theory Integration**: Extend certainty handling to full uncertainty theory, supporting complex logical consistency checks across collections of opinions.
* **Performance and Scalability**: Optimize for large-scale deployments with distributed persistence, advanced indexing, and parallel query execution.
* **Ecosystem Expansion**: Develop integrations with other databases, visualization tools, and APIs; add more data types (e.g., geospatial, multimedia); and build a plugin system for custom extensions.
* **Production Readiness**: Extend the existing backup/restore tools with
  replication, monitoring, and compliance workflows.

## License

This work is dual-licensed under Apache 2.0 and MIT. You can choose between one of them if you use this work.

SPDX-License-Identifier: Apache-2.0 OR MIT
