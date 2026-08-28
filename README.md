<img src="./positorium.svg" width="333" alt="Positorium">

# Positorium

Positorium is an experimental database engine for information that can
conflict, vary over time, and carry source-specific certainty. It combines an
immutable posit model, the Traqula query language, an append-only native store,
a trusted-local HTTP service, and an in-browser WASM testbed.

The current release line is `0.1.4-beta.1`. Beta means the documented language,
storage, transfer, and interface versions have explicit compatibility rules; it
does not mean an Internet-facing or production-ready service.

[Try the browser testbed](https://roenbaeck.github.io/positorium/) ·
[Get started](docs/GETTING_STARTED.md) ·
[Browse all documentation](docs/README.md)

## Why Positorium?

Most databases converge on one current value. Positorium preserves evidence:

- contradictory posits remain available instead of overwriting each other;
- expressed time precision is part of the stored fact;
- assertions record who ascertained a posit, with signed certainty and time;
- snapshots and `in effect` queries make resolution policy explicit; and
- exact entered literals survive native storage, HTTP, SSE, and WASM results.

This is useful for exploring master data, regulated records, investigations,
and other domains where evidence accumulates and is revised.

## Five-minute source checkout

Install [rustup](https://rustup.rs/), then build and start the trusted-local
server from the repository root:

```sh
cargo build --release --locked
./target/release/positorium
```

In another terminal, resolve or create one identity:

```sh
curl --fail-with-body --silent --show-error \
  http://127.0.0.1:8080/v1/query \
  -H 'content-type: application/json' \
  --data '{"traqula_version":1,"script":"add role name; search [{(?person, name), ...}, \"Ada\", *] return ?person or add posit [{(+person, name)}, \"Ada\", @NOW];","stream":false}'
```

The response should have `"status":"ok"` and one `thing` result cell. Stop the
server with Ctrl-C. The default creates `positorium.store` and reopens it on the
next start.

For release archives, checksum verification, Windows commands, persistence
checks, backup, Query Studio setup, and troubleshooting, follow
[Getting started](docs/GETTING_STARTED.md).

## Beta boundaries

- The native server binds to `127.0.0.1` by default and has no authentication.
  Never expose it to an untrusted network.
- One process owns a store and executes scripts serially.
- The append-only store detects committed corruption but is not a tamper-proof
  audit log.
- Authentication, replication, distributed execution, and container packaging
  are outside the current beta.
- Independent contract versions are listed in
  [Contracts](docs/reference/CONTRACTS.md).

## Current capabilities

- Immutable Things, Roles, appearance sets, posits, and assertion envelopes
- Lossless literal tokens and precision-aware temporal relations
- Traqula joins, union, safe `not exists`, snapshots, typed predicates,
  `in effect`, parameters, distinctness, ordering, and limits
- Atomic durable mutation batches with deterministic replay and recovery
- Inspection, physical backup, logical export/import, and identity remapping
- Structured Rust, buffered HTTP, SSE, and WASM results
- Query Studio and authoritative Terrain structural reports

## Documentation

| Start here | Purpose |
| --- | --- |
| [Getting started](docs/GETTING_STARTED.md) | Install, run, query, restart, back up, and troubleshoot |
| [Traqula](docs/reference/TRAQULA.md) | Language and query reference |
| [Cookbook](docs/guides/COOKBOOK.md) | Worked modeling and maintenance recipes |
| [Operations](docs/guides/OPERATIONS.md) | Durability, recovery, limits, and deployment posture |
| [Core model](docs/reference/MODEL.md) | Identity, literal, temporal, and snapshot semantics |
| [Storage](docs/reference/STORAGE.md) | Append-only format contract |
| [Transfer](docs/reference/TRANSFER.md) | Backup, export, import, and identity remapping |
| [Terrain](docs/reference/TERRAIN.md) | Structural report and visualization contract |
| [Theory](docs/design/THEORY.md) | Philosophical foundations |
| [Roadmap](docs/development/ROADMAP.md) | Remaining and post-beta work |

The [documentation index](docs/README.md) also links compatibility decisions,
benchmarks, and maintainer specifications. The original paper,
[Modeling Conflicting, Unreliable, and Varying Information](https://www.researchgate.net/publication/329352497_Modeling_Conflicting_Unreliable_and_Varying_Information),
provides additional background.

## Development checks

The repository pins the stable Rust toolchain and runs native checks on Linux,
macOS, and Windows plus a browser WASM suite.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --no-fail-fast
cargo test --all-targets --no-default-features --no-fail-fast
node tests/terrain_client.test.js
```

See [Extending Traqula](docs/development/EXTEND_TRAQULA.md) before changing the
language or synchronized editor grammar.

## License

Dual-licensed under Apache 2.0 or MIT, at your option.

SPDX-License-Identifier: Apache-2.0 OR MIT
