<img src="https://raw.githubusercontent.com/Roenbaeck/positorium/master/positorium.svg" width="333" alt="Positorium">

# Positorium

> **A database for facts that disagree.**

Positorium preserves conflicting claims instead of forcing them into one current
value. Every assertion can retain its source, certainty, appearance time, and
assertion time, so corrections never erase the evidence that came before.

It is an experimental embedded evidence database with an immutable posit model,
the Traqula query language, an append-only native store, Python bindings, a
trusted-local HTTP service, and an in-browser WASM testbed.

The repository is preparing `0.1.4-beta.2`; the latest tagged release is
[`0.1.4-beta.1`](https://github.com/Roenbaeck/positorium/releases/tag/v0.1.4-beta.1).
Beta means the documented language, storage, transfer, and interface versions
have explicit compatibility rules; it does not mean an Internet-facing or
production-ready service.

[Run the 60-second browser example](https://roenbaeck.github.io/positorium/) ·
[Get started](https://github.com/Roenbaeck/positorium/blob/master/docs/GETTING_STARTED.md) ·
[Browse all documentation](https://github.com/Roenbaeck/positorium/blob/master/docs/README.md)

## Python

The source tree contains an embedded Python package for CPython 3.9+ on Linux,
macOS, and Windows. It needs no separate Positorium server. Until beta.2 is
published to PyPI, install it from a source checkout with Rust available:

```sh
python -m pip install .
```

After beta.2 is published, the prerelease install will be:

```sh
python -m pip install --pre positorium
```

```python
import positorium

with positorium.Database.memory() as database:
    result = database.execute_one(
        'add role name; add posit [{(+person, name)}, "Ada", @NOW]; '
        'search [{(?person, name)}, ?name, *] return ?person, ?name;'
    )
    print(result[0]["person"].text, result[0]["name"].text)
```

Use `Database.open("positorium.store")` for an append-only persistent store.
Results retain each cell's kind and exact entered text. See the
[Python guide](https://github.com/Roenbaeck/positorium/blob/master/docs/guides/PYTHON.md) for typed parameters, Terrain, Pandas,
exceptions, and lifecycle rules.

## Why Positorium?

Most databases converge on one current value. Positorium preserves evidence:

- contradictory posits remain available instead of overwriting each other;
- expressed time precision is part of the stored fact;
- assertions record who ascertained a posit, with signed certainty and time;
- snapshots and `in effect` queries make resolution policy explicit; and
- exact entered literals survive native storage, HTTP, SSE, and WASM results.

This is useful for compliance evidence, investigations, conflicting master
data, and other domains where evidence accumulates and is revised. Positorium
can serve as a focused evidence layer alongside existing operational systems;
it does not need to replace them.

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
[Getting started](https://github.com/Roenbaeck/positorium/blob/master/docs/GETTING_STARTED.md).

## Beta boundaries

- The native server binds to `127.0.0.1` by default and has no authentication.
  Never expose it to an untrusted network.
- One process owns a store and executes scripts serially.
- The append-only store detects committed corruption but is not a tamper-proof
  audit log.
- Authentication, replication, distributed execution, and container packaging
  are outside the current beta.
- Independent contract versions are listed in
  [Contracts](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/CONTRACTS.md).

## Current capabilities

- Immutable Things, Roles, appearance sets, posits, and assertion envelopes
- Lossless literal tokens and precision-aware temporal relations
- Traqula joins, union, safe `not exists`, snapshots, typed predicates,
  `in effect`, parameters, distinctness, ordering, and limits
- Atomic durable mutation batches with deterministic replay and recovery
- Inspection, physical backup, logical export/import, and identity remapping
- Structured Rust, buffered HTTP, SSE, and WASM results
- Embedded Python 3.9+ bindings with structured lossless results
- Query Studio and authoritative Terrain structural reports

## Documentation

| Start here | Purpose |
| --- | --- |
| [Getting started](https://github.com/Roenbaeck/positorium/blob/master/docs/GETTING_STARTED.md) | Install, run, query, restart, back up, and troubleshoot |
| [Python](https://github.com/Roenbaeck/positorium/blob/master/docs/guides/PYTHON.md) | Install the wheel, embed a database, bind parameters, and consume results |
| [The Blackthorn Ruby](https://github.com/Roenbaeck/positorium/blob/master/docs/guides/BLACKTHORN_CASE.md) | Interactive detective story and full Query Studio/Terrain showcase |
| [Traqula](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/TRAQULA.md) | Language and query reference |
| [Cookbook](https://github.com/Roenbaeck/positorium/blob/master/docs/guides/COOKBOOK.md) | Worked modeling and maintenance recipes |
| [Operations](https://github.com/Roenbaeck/positorium/blob/master/docs/guides/OPERATIONS.md) | Durability, recovery, limits, and deployment posture |
| [Core model](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/MODEL.md) | Identity, literal, temporal, and snapshot semantics |
| [Storage](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/STORAGE.md) | Append-only format contract |
| [Transfer](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/TRANSFER.md) | Backup, export, import, and identity remapping |
| [Terrain](https://github.com/Roenbaeck/positorium/blob/master/docs/reference/TERRAIN.md) | Structural report and visualization contract |
| [Theory](https://github.com/Roenbaeck/positorium/blob/master/docs/design/THEORY.md) | Philosophical foundations |
| [Roadmap](https://github.com/Roenbaeck/positorium/blob/master/docs/development/ROADMAP.md) | Remaining and post-beta work |

The [documentation index](https://github.com/Roenbaeck/positorium/blob/master/docs/README.md) also links compatibility decisions,
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

See [Extending Traqula](https://github.com/Roenbaeck/positorium/blob/master/docs/development/EXTEND_TRAQULA.md) before changing the
language or synchronized editor grammar.

## License

Dual-licensed under Apache 2.0 or MIT, at your option.

SPDX-License-Identifier: Apache-2.0 OR MIT
