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
| Terrain | `1` | Rust report, `/v1/terrain`, and WASM Terrain response |
| Logical export | `1` | JSONL header; specified by `TRANSFER.md` |
| Identity remap | `1` | JSON remap artifact; specified by `TRANSFER.md` |

Storage, Traqula, HTTP, SSE, WASM, Terrain, logical export, and identity remapping
evolve independently. A change to one does not silently change another. Adding
Terrain 1 is additive to HTTP v1 and WASM interface 1 and does not change
Traqula 1 or SSE 1. Terrain rejects an unsupported `terrain_version`
independently. Patch releases remain compatible. A beta minor may make a
documented break; where practical, published syntax and APIs receive one minor
release of warnings and mechanical rewrite guidance. Security, corruption, and
never-published behavior may be corrected immediately with release notes.

Store `1.0` bootstraps five fixed Role mappings: `posit` (`1`), `ascertains`
(`2`), `thing` (`3`), `class` (`4`), and `subclass` (`5`). The first two support
the assertion contract. The latter three are stable classification vocabulary
only: no store, Traqula, HTTP, WASM, or Terrain contract assigns semantics to a
classification posit's literal value or performs implicit subclass inference.
Such interpretation belongs to an explicit consumer or presentation policy;
ordinary query filtering remains literal and neutral.

Traqula 1 includes atomic `and assert` mutation sugar and `search ... or add
posit ...` identity resolution. The latter may omit `return`; only searches
with a return projection create HTTP, SSE, WASM, or Rust result sets. Ordinary
query variables remain lexical. Within the compound operation, same-name Thing
or Posit query/allocation binders are explicitly unified into a script mutation
binding, with zero matches executing the fallback once and one or many matches
promoting the complete identity set.

Every published store-format break must preserve logical data through a direct
or stepwise offline migration that writes and validates a new store beside the
source. Because store `1.0` is the first published format, there is currently no
older store reader or migrator to ship.
