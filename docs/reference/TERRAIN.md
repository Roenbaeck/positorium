# Authoritative Role Terrain

Role Terrain is an isopleth-inspired structural view of one coherent Positorium
database snapshot. Rust reads the canonical database structures directly and
returns a versioned, bounded report containing database totals, shared Role
projection, History and Current frames, exact profiles and supports, and a
catalog of relationship signatures. It does not execute Traqula, inspect query
result rows, or consume normal query row limits.

Open **Terrain** in `positorium.html` to load a report automatically. In server
mode the browser calls `POST /v1/terrain`; in Local WASM mode it calls
`WasmEngine.terrain(...)`. No preparatory query is required. The source badge
distinguishes **Database snapshot** from **Browser database snapshot**, and the
Refresh button requests a new coherent report.

Query execution marks an existing report stale because the browser does not try
to infer whether a script mutates the database. A failed refresh keeps the last
complete report visible as stale and shows the error; it never substitutes a
partial report or falls back to query-derived aggregation. The Query Studio
Stream setting has no effect on Terrain.

## Frames and snapshot semantics

One report contains two frames over the same structural capture:

- **History** includes every canonical recorded posit, including future-dated
  posits.
- **Current** resolves one exact cutoff and retains every eligible maximal posit
  for each exact appearance set using Positorium's partial-order snapshot
  semantics.

The default cutoff is one resolution of `@NOW`. The resolved canonical cutoff is
returned as `resolved_as_of`; prepared Current queries reuse that exact token
rather than evaluating a new `@NOW`. Equal-time conflicting values and
incomparable mixed-precision maxima remain together. A later definitely ordered
time dominates an earlier one; identity and append order never break ties.

Database totals are independent of the frames. They count all catalog Roles,
canonical Appearances, AppearanceSets, Posits, and retained referenced Things.
Frame statistics count only active endpoint Things, Roles, appearance sets,
posits, and incidences.

In this document, **active** means retained in the selected History or Current
frame. It does not refer to a classification literal named `"active"`; Terrain
assigns no special meaning to that or any other posit value.

## Role projection, profiles, and isopleths

A Role is an attribute candidate when it occurs in an active singleton
appearance set. Its support is the number of distinct endpoint Things in those
sets; repeated posits do not multiply support.

History and Current share one projected Role universe, so a scope switch does
not change Role bits, profile identities, isopleth identities, or layout. The
union of candidates is ranked by Current support, History support, Role name,
and Role identity. Terrain 1 projects at most eight Roles. `projection.complete`
and `total_attribute_roles` explicitly report whether anything was omitted.

For projected universe `U`, every endpoint Thing receives one exact bit mask:

```text
profile_U(t) = { r in U | t occurs in an active singleton set for r }
```

Mask zero is retained for relationship-only Things. Every populated nonzero
profile `M` emits an isopleth whose support is the number of endpoint Things with
a superset profile:

```text
support(M) = sum population(P) where (P & M) == M
```

Support is therefore monotone: adding a Role cannot increase it. When the
projection is incomplete, an absent bit means absent only from the projected
universe, not necessarily from the full database.

## Relationships and allocations

An active appearance set with at least two appearances is a relationship
candidate. Its exact signature is the sorted set of backend Role identities.
The shared History/Current catalog is ranked by Current set count, History set
count, arity, Role-name sequence, and Role-identity sequence. Terrain returns 16
signatures by default, with a hard maximum of 128, and reports catalog
completeness and the untruncated total.

The selected signature appears in both frames even when one frame has zero
matching sets. For each relationship Role:

- `participations` counts appearances across distinct matching appearance sets;
- `distinct_things` counts unique endpoint Things;
- allocations group endpoints by exact projected profile.

Repeated historical posits increase the relationship posit count, not endpoint
participation. Mask-zero allocations remain selectable in the inspector but
have no SVG edge because there is no mask-zero isopleth.

## Interfaces

The Rust API is `Database::terrain()`,
`Database::terrain_with_options(TerrainOptions)`, or the forwarding
`QueryInterface::terrain_with_options(...)`. `TerrainReport.terrain_version` is
`1`; Thing and Role identities are canonical unsigned-decimal strings, counters
are checked `u64` values, and every output vector has deterministic ordering.

The HTTP request is:

```http
POST /v1/terrain
Content-Type: application/json

{
  "terrain_version": 1,
  "as_of": "@NOW",
  "timeout_ms": 5000,
  "projected_role_limit": 8,
  "max_relationship_signatures": 16
}
```

Only `terrain_version` is required. `as_of` accepts the same time tokens and
constants as Traqula. Responses carry HTTP API version `v1`, Terrain version `1`,
status, elapsed time, and either a complete report or a structured error. They
also set `Cache-Control: no-store`. Terrain 1 intentionally has no SSE route.

The additive WASM method accepts the same Terrain fields and returns WASM
interface version `1`, Terrain version `1`, and the report. It enforces deadlines
and hard limits cooperatively. Because the WASM call is synchronous on the
browser thread, JavaScript cannot deliver a same-thread cancellation while it is
running.

Unknown Terrain versions are rejected independently of Traqula, HTTP, SSE, and
WASM interface versions.

## Bounds and consistency

Terrain starts its timeout before waiting for the database execution owner,
captures structural sources under that owner in a fixed lock order, and releases
all database locks before aggregation. Concurrent queries and writers therefore
serialize with capture, while the more expensive aggregation operates on an
immutable snapshot.

Terrain 1 hard limits are one million scanned posits, 250,000 active appearance
sets, one million distinct endpoint Things, five million temporal comparisons,
eight projected Roles, relationship arity 256, 128 returned signatures, and
4,096 detailed allocations. Exceeding a hard limit fails the whole report with a
typed resource-limit error. Only the Role projection and relationship catalog
may truncate, and both expose completeness metadata.

## Golden fixture

[`traqula/terrain.traqula`](../../traqula/terrain.traqula) is retained as a population
fixture for backend tests, not as a browser workflow. It produces:

| Measurement | History | Current |
| --- | ---: | ---: |
| Endpoint Things | 6 | 6 |
| Roles in active sets | 8 | 8 |
| Appearance sets | 24 | 24 |
| Posits | 26 | 24 |
| Incidences | 30 | 27 |

The shared attribute projection contains `name`, `hair color`, `height`,
`social security number`, `RFID`, and `beard color`. For `{owner, pet}`, History
contains four posits and Current contains three over the same three appearance
sets. Cass contributes two owner participations, Ada one, and Mochi and Pixel
contribute three pet participations together.

## Client responsibilities

Rust owns all semantic counts and identities. The browser owns SVG geometry,
colors, label placement, minimum-support filtering, selection, relationship
visibility, and query preparation. Selecting a Role, isopleth, relationship, or
allocation opens its measurements; the relationship selector exposes every
signature returned by the backend catalog. The layout derives a stable subset
hierarchy from the union of History and Current isopleths: shared Roles form a
core, Roles introduced by supersets fan into separate branches, smooth hulls
follow those branches, and relationship allocations connect below the Role
topology. Scope changes and support filtering do not move Role positions.

## Selected-class overlay

Terrain 1's authoritative report remains structural and value-independent. A
class overlay is a separate browser presentation over ordinary classification
query results; it does not change Rust counts, profile identities, isopleth
identities, or the Terrain report contract.

The overlay allows exactly one selected class. In the Current view, the browser
uses the report's resolved cutoff for both operands of a separate `in effect`
query over direct `{thing, class}` evidence. It then applies visible controls
for exact lifecycle value, positor/source, and certainty sign. The History view
does not shade a class. Values such as `"active"` belong to the selected UI
policy and are never database truth. Optional traversal of `{subclass, class}`
posits remains deferred and would require an equally explicit policy.

Render the selected class as translucent shaded areas behind existing isopleth
lines and text:

- when the selected members exhaust a projected profile, reuse that profile's
  isopleth interior exactly;
- otherwise form padded member regions within the matching profile;
- allow natural overlap but retain disconnected islands instead of drawing a
  bridge that implies a relationship; and
- keep labels, isopleth strokes, connections, selection, and inspection legible
  above the fill.

The selector currently uses the class Thing identity; descriptive labels are a
future client enhancement. The selected class, value, source treatment,
certainty rule, member count, and resolved cuts remain visible so shading cannot
be mistaken for an inference made by Positorium. Class-member Role profiles are
obtained by a second ordinary query and mapped to Terrain's structural geometry;
unmapped members remain visible as disconnected islands.
