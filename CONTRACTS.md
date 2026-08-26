# Positorium Beta Contract Versions

The first published beta starts each compatibility boundary below. There is no
released SQLite, Traqula, Rust API, or wire-format predecessor to migrate. The
SQLite prototype is deliberately excluded by D031.

| Contract | Current version | Where it is carried |
| --- | ---: | --- |
| Crate API | `0.1` | Cargo package version; 0.x minor releases may contain documented beta breaks |
| Store | `1.0` | Manifest and every log-segment header; specified by `STORAGE.md` |
| Traqula | `1` | Execution metadata, required HTTP request field, HTTP response, and WASM response |
| HTTP | `v1` | `/v1/query` and every buffered response |
| SSE | `1` | `version` in every stream event |
| WASM | `1` | `interface_version` in every returned JavaScript object |
| Logical export | `1` | JSONL header; specified by `TRANSFER.md` |
| Identity remap | `1` | JSON remap artifact; specified by `TRANSFER.md` |

Storage, Traqula, HTTP, SSE, WASM, logical export, and identity remapping evolve
independently. A change to one does not silently change another. Patch releases
remain compatible. A beta minor may make a documented break; where practical,
published syntax and APIs receive one minor release of warnings and mechanical
rewrite guidance. Security, corruption, and never-published behavior may be
corrected immediately with release notes.

Every published store-format break must preserve logical data through a direct
or stepwise offline migration that writes and validates a new store beside the
source. Because store `1.0` is the first published format, there is currently no
older store reader or migrator to ship.
