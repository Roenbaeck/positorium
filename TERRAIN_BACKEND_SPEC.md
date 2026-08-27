# Authoritative Terrain Backend Specification

**Status:** implementation handoff  
**Terrain contract:** version 1  
**Replaces:** query-derived Terrain aggregation introduced in `cffc7d0`

## Objective

Terrain must be an authoritative structural read model produced by Rust from one
coherent database state. It must not be reconstructed from user-visible Traqula
rows, depend on a special query script, or consume normal query row limits.

One request returns:

- immutable all-database totals;
- one shared projected Role universe;
- a recorded-history frame;
- an `as of` frame using Positorium snapshot semantics;
- a bounded catalog of exact multi-Role relationship signatures;
- exact projected profiles, support isopleths, and relationship allocations.

The same semantic report must be available through Rust, HTTP, and WASM. SVG
geometry, colors, label placement, filtering, selection, and query preparation
remain client responsibilities.

## Why Query-Derived Terrain Must Be Removed

The current browser aggregation in `positorium-terrain.js` is useful as a
prototype, but is not an authoritative contract:

1. It accepts any two result sets with matching column names; predicates,
   cutoffs, source populations, and intervening mutations are not proven equal.
2. HTTP row limits may truncate an appearance set, changing a relationship into
   an apparent singleton attribute and corrupting every downstream count.
3. It checks column aliases but not required typed cell kinds.
4. Its counters describe returned rows, not the complete database.
5. History and current frames independently choose projected Roles and the
   dominant relationship, so switching scope can change the map vocabulary.
6. Display text and a client-side 32-bit hash stand in for authoritative Role
   and structural identities.
7. Relationship endpoints with no projected attribute profile are omitted.
8. It exports potentially large incidence result sets only to aggregate them
   again in JavaScript.
9. Streamed Terrain depends on recognizing a result-pair completion rather than
   a dedicated bounded response.

Terrain therefore needs a dedicated structural analyzer, not a privileged
Traqula recipe.

## Versioning

Add an independent contract to `CONTRACTS.md`:

| Contract | Current version | Where it is carried |
| --- | ---: | --- |
| Terrain | `1` | Rust report, `/v1/terrain`, and WASM Terrain response |

Define:

```rust
pub const TERRAIN_VERSION: u16 = 1;
```

Adding `/v1/terrain` and a WASM method is additive. It does not by itself require
changes to Traqula 1, HTTP v1, SSE 1, or WASM interface 1. Terrain rejects an
unknown Terrain version independently.

## Public Rust Contract

Create `src/terrain.rs`, export it from `src/lib.rs`, and re-export the stable
DTOs. All report DTOs derive at least `Debug`, `Clone`, `PartialEq`, `Eq`, and
`serde::Serialize`. Input DTOs also derive `Deserialize` where appropriate.

Recommended high-level API:

```rust
pub const TERRAIN_VERSION: u16 = 1;
pub const DEFAULT_PROJECTED_ROLE_LIMIT: usize = 8;
pub const MAX_PROJECTED_ROLE_LIMIT: usize = 8;

#[derive(Debug, Clone)]
pub struct TerrainOptions {
    pub as_of: Option<Time>,
    pub timeout: Option<Duration>,
    pub cancellation: Option<CancellationToken>,
    pub projected_role_limit: usize,
    pub max_relationship_signatures: usize,
    pub limits: TerrainLimits,
}

impl Default for TerrainOptions { /* specified defaults below */ }

impl Database {
    pub fn terrain(&self) -> Result<TerrainReport, DatabaseError>;
    pub fn terrain_with_options(
        &self,
        options: TerrainOptions,
    ) -> Result<TerrainReport, DatabaseError>;
}

impl QueryInterface {
    pub fn terrain_with_options(
        &self,
        options: TerrainOptions,
    ) -> Result<TerrainReport, DatabaseError>;
}
```

`Database::terrain_with_options` owns lock acquisition. `QueryInterface` simply
forwards to the shared `Database`; separately created interfaces must not bypass
serialization.

### Report DTOs

Names may be adjusted to local Rust style, but the fields and semantics are
normative.

```rust
pub struct TerrainReport {
    pub terrain_version: u16,
    pub resolved_as_of: String,
    pub database: TerrainDatabaseTotals,
    pub projection: TerrainProjection,
    pub relationship_catalog: TerrainRelationshipCatalog,
    pub frames: TerrainFrames,
}

pub struct TerrainFrames {
    pub history: TerrainFrame,
    pub current: TerrainFrame,
}

pub struct TerrainDatabaseTotals {
    pub referenced_things: u64,
    pub roles: u64,
    pub appearances: u64,
    pub appearance_sets: u64,
    pub posits: u64,
}

pub struct TerrainProjection {
    pub complete: bool,
    pub total_attribute_roles: u64,
    pub roles: Vec<TerrainProjectedRole>,
}

pub struct TerrainProjectedRole {
    pub id: String,
    pub name: String,
    pub bit: u8,
    pub history_support: u64,
    pub current_support: u64,
}

pub struct TerrainRelationshipCatalog {
    pub complete: bool,
    pub total_signatures: u64,
    pub default_signature_id: Option<String>,
    pub signatures: Vec<TerrainRelationshipSignature>,
}

pub struct TerrainRelationshipSignature {
    pub id: String,
    pub roles: Vec<TerrainRoleRef>,
}

pub struct TerrainRoleRef {
    pub id: String,
    pub name: String,
}

pub struct TerrainFrame {
    pub scope: TerrainScope,
    pub stats: TerrainFrameStats,
    pub role_supports: Vec<TerrainRoleSupport>,
    pub profiles: Vec<TerrainProfile>,
    pub isopleths: Vec<TerrainIsopleth>,
    pub relationships: Vec<TerrainRelationship>,
}

pub enum TerrainScope {
    History,
    Current,
}

pub struct TerrainFrameStats {
    pub endpoint_things: u64,
    pub roles: u64,
    pub appearance_sets: u64,
    pub posits: u64,
    pub incidences: u64,
}

pub struct TerrainRoleSupport {
    pub role_id: String,
    pub distinct_things: u64,
}

pub struct TerrainProfile {
    pub id: String,
    pub mask: u16,
    pub present_role_ids: Vec<String>,
    pub absent_role_ids: Vec<String>,
    pub things: u64,
    pub isopleth_id: Option<String>,
}

pub struct TerrainIsopleth {
    pub id: String,
    pub mask: u16,
    pub included_role_ids: Vec<String>,
    pub support: u64,
}

pub struct TerrainRelationship {
    pub signature_id: String,
    pub appearance_sets: u64,
    pub posits: u64,
    pub role_totals: Vec<TerrainRoleTotal>,
    pub allocations: Vec<TerrainAllocation>,
}

pub struct TerrainRoleTotal {
    pub role_id: String,
    pub distinct_things: u64,
    pub participations: u64,
}

pub struct TerrainAllocation {
    pub id: String,
    pub role_id: String,
    pub profile_id: String,
    pub profile_mask: u16,
    pub isopleth_id: Option<String>,
    pub distinct_things: u64,
    pub participations: u64,
}
```

Thing and Role identities are serialized as canonical unsigned decimal strings,
not JavaScript numbers. Counts are checked `u64` values. `mask` is safe as a JSON
number because Terrain 1 permits at most eight projected Roles.

`resolved_as_of` is the exact resolved UTC datetime token used to build the
current frame. The default `None` resolves once at analysis start, equivalent to
`@NOW`. Never relabel an arbitrary supplied cutoff as now.

## Normative Semantics

Let `A(p)` be posit `p`'s exact appearance set and `T(p)` its stored `Time`.
Terrain is value-independent: literal values and codecs do not affect structural
membership.

### Immutable database totals

`database` describes the complete captured database, independent of the selected
Terrain frame:

- `roles`: all catalog Roles, including `posit` and `ascertains`;
- `appearances`: all canonical Appearances;
- `appearance_sets`: all canonical AppearanceSets;
- `posits`: all canonical Posits;
- `referenced_things`: distinct retained identities, including Role and Posit
  identities and endpoint Things.

These are not inferred from frame incidences and must not change when the user
switches History/Current.

### History frame

History contains every canonical recorded posit from the captured database,
including future-dated posits. Each posit appears once.

### Current frame

Resolve cutoff `C` once. A posit is eligible when:

```text
T(p).definitely_at_or_before(C)
```

For each exact appearance set, retain every eligible maximal posit. Eligible
`p` is dominated only when another eligible `q` with the same appearance set
satisfies:

```text
T(p).definitely_before(T(q))
```

Consequences:

- equal stored times remain together;
- equal-time conflicting values remain together;
- incomparable mixed-precision times may both remain maximal;
- identity and append order never break ties;
- future-only appearance sets are absent from the current frame.

This must share the same semantics and helper code as ordinary Traqula `as of`.
Do not use `Ord`, posit identity, or a single `max_by_key` selection.

### Frame statistics

Frame statistics describe active structures in that frame:

- `endpoint_things`: distinct Things appearing in active appearance sets;
- `roles`: distinct Role identities appearing in active appearance sets;
- `appearance_sets`: distinct active exact appearance sets;
- `posits`: active posit identities;
- `incidences`: sum of `|A(p)|` over active posits.

Repeated history for one appearance set increases `posits` and `incidences`, but
not `appearance_sets`.

### Attribute Roles

A Role is an attribute-role candidate in a frame when it occurs in at least one
active appearance set of cardinality one. Support is the number of distinct
endpoint Things appearing in singleton sets for that Role; repeated posits do
not multiply support.

### Shared Role projection

History and Current must use one shared projected Role universe `U` so scope
switching preserves Role bits, profiles, isopleth identity, and layout.

Build the union of attribute-role candidates from both frames and rank by:

1. current support descending;
2. history support descending;
3. NFC Role name ascending;
4. Role identity ascending.

Select `projected_role_limit` Roles, default and maximum eight. Set:

- `total_attribute_roles` to the pre-limit union count;
- `complete` to whether the union fits the limit;
- `bit` to the stable position `0..7` in the shared projection.

### Exact projected profiles

For every endpoint Thing in a frame, construct a bit mask over `U`. A bit is set
when that Thing appears in an active singleton appearance set for the projected
Role.

Group Things by exact mask. Include mask zero so relationship-only Things remain
represented. Profile populations must sum exactly to `frame.stats.endpoint_things`.

For each profile:

- `present_role_ids` and `absent_role_ids` are relative to `U`;
- `id` is deterministic from the shared projection version and mask;
- nonzero masks have `isopleth_id`; mask zero has `None`.

If `projection.complete == false`, an absent bit means absent only in the
projection. It is never a claim that the Thing has no omitted Role.

### Support isopleths

Emit one isopleth for every populated nonzero exact profile mask `M`.
Its support is the number of frame Things whose profile is a superset of `M`:

```text
support(M) = sum population(P) for every P where (P & M) == M
```

Isopleth support must be monotone: adding an included Role cannot increase
support. Mask zero is not rendered as an isopleth.

### Relationship signatures

An active appearance set of cardinality at least two is a relationship
candidate. Its exact signature is the sorted set of Role identities in that
appearance set. Role names are display metadata and never identity keys.

Build a shared bounded signature catalog from the union of History and Current.
Rank signatures by:

1. current appearance-set count descending;
2. history appearance-set count descending;
3. arity descending;
4. NFC Role-name sequence ascending;
5. Role-identity sequence ascending.

Return at most `max_relationship_signatures`. Set catalog `complete` and
`total_signatures` explicitly. `default_signature_id` is the first ranked
signature, or `None`.

Each frame emits a `TerrainRelationship` for every returned catalog signature,
even when its frame count is zero. This preserves selection across scope changes.

For a selected signature:

- `appearance_sets`: distinct active appearance sets with exactly that signature;
- `posits`: active posits attached to those sets;
- a Role's `participations`: appearances in that Role across distinct matching
  appearance sets, counted once per set, not once per posit;
- a Role's `distinct_things`: unique Things in that Role across matching sets.

### Relationship allocations

Allocation key is `(relationship Role identity, endpoint Thing profile mask)`.
Count each endpoint once per distinct matching appearance set:

- `participations` counts matching sets for that key;
- `distinct_things` deduplicates endpoint Things for that key.

Allocations for one relationship Role must satisfy:

```text
sum allocation.participations == role_total.participations
sum allocation.distinct_things == role_total.distinct_things
```

The distinct equality relies on exact profile groups being disjoint.

Include mask-zero allocations with `isopleth_id: None`. They remain inspectable
but have no line to an isopleth. Repeated historical posits for an unchanged
appearance set increase relationship `posits`, not participation.

## Structural Capture And Algorithm

Do not execute Traqula and do not materialize `ResultCell` rows.

### Capture phase

1. Resolve cutoff, deadline, cancellation, and limits once.
2. Acquire `Database::execution_owner` using the same bounded one-millisecond
   cooperative wait loop as `Engine`.
3. Under the owner, lock structural sources in a documented fixed order.
4. Capture database totals and clone an immutable vector of:

   ```rust
   struct CapturedPosit {
       posit: Thing,
       appearance_set: Arc<AppearanceSet>,
       time: Time,
   }
   ```

   The reverse appearance-set and time maps must have identical posit keys;
   mismatch is `DatabaseError::Invariant`.
5. Release fine-grained locks and `execution_owner` after the immutable capture.
   All aggregation runs against that capture.

Add crate-private read-only iteration/snapshot helpers to keepers as needed. Do
not expose keepers or maps through the public API.

### Aggregation phase

1. Group captured posits by canonical `Arc<AppearanceSet>`.
2. History retains all captured records.
3. Current builds the partial-order maximal eligible frontier for each group.
4. Build frame statistics and singleton Role-to-Thing `RoaringTreemap`s.
5. Select the shared projected Role universe from both frame supports.
6. Assign each frame Thing an at-most-eight-bit exact profile.
7. Count profile populations.
8. Compute all superset supports with a zeta transform in
   `O(|U| * 2^|U|)` rather than pairwise profile scans.
9. Group active multi-appearance sets by sorted Role identities. Count sets once
   and posits separately.
10. Build the shared relationship catalog, then each frame's totals and
    allocations.
11. Sort every emitted vector explicitly. Hash-map iteration order is never part
    of the contract.

No literal/value lookup is required.

## Locking, Cancellation, And Limits

Timeout starts before owner-lock acquisition. Check cancellation/deadline while
waiting, every 1,024 captured/aggregated records, and during temporal frontier
comparisons.

Recommended Terrain 1 defaults and hard maxima:

| Resource | Default | Hard maximum |
| --- | ---: | ---: |
| Posits scanned | 1,000,000 | 1,000,000 |
| Active appearance sets | 250,000 | 250,000 |
| Distinct endpoint Things | 1,000,000 | 1,000,000 |
| Temporal comparisons | 5,000,000 | 5,000,000 |
| Projected attribute Roles | 8 | 8 |
| Relationship arity | 256 | 256 |
| Relationship signatures returned | 16 | 128 |
| Detailed allocations returned | 4,096 | 4,096 |

Use checked arithmetic for every counter. Exceeding scan, active-set, Thing,
temporal-comparison, arity, or allocation maxima fails the complete report with
a typed resource-limit error. Never return partial core statistics.

Only bounded Role projection and signature-catalog ranking may truncate. Those
surfaces must report `complete: false` and the untruncated total count.

Suggested error variant:

```rust
DatabaseError::ResourceLimit {
    resource: &'static str,
    limit: u64,
}
```

## HTTP Contract

Add a dedicated endpoint; do not overload `/v1/query`:

```text
POST /v1/terrain
```

Request:

```json
{
  "terrain_version": 1,
  "as_of": "@NOW",
  "timeout_ms": 5000,
  "projected_role_limit": 8,
  "max_relationship_signatures": 16
}
```

All fields except `terrain_version` are optional and use server defaults.
`as_of` accepts the same time token/constants as Traqula. Resolve `@NOW` once.
Reject an unsupported Terrain version before acquiring the database owner.

Response:

```json
{
  "api_version": "v1",
  "terrain_version": 1,
  "status": "ok",
  "elapsed_ms": 1.25,
  "report": {}
}
```

Use existing HTTP request-size, five-second default timeout, thirty-second hard
timeout, loopback CORS, and structured error conventions. Add
`Cache-Control: no-store`.

Use `spawn_blocking` and a cancellation token. Install a cancel-on-drop guard so
client disconnect cancels lock waiting/aggregation cooperatively.

Do not add Terrain SSE in version 1. The bounded report requires aggregation
before it is useful and has no meaningful row stream. The Query Studio Stream
setting does not affect Terrain refreshes.

## WASM Contract

Add an additive method:

```rust
#[wasm_bindgen]
impl WasmEngine {
    pub fn terrain(&self, options: JsValue) -> Result<JsValue, JsValue>;
}
```

An omitted/`undefined` options object uses defaults. Deserialize a WASM input DTO
containing `terrain_version`, optional `as_of`, projected Role limit, and
signature limit. Return:

```json
{
  "interface_version": "1",
  "terrain_version": 1,
  "report": {}
}
```

WASM enforces deadlines and hard limits. Same-thread browser execution cannot be
interrupted by a JavaScript cancellation call while blocked; document that
boundary.

## Frontend Migration

1. Retain current SVG layout, interactions, support filtering, side relationship
   rail, and prepared-query behavior.
2. Replace `buildTerrainData(resultSets)` with a thin adapter from
   `TerrainReport`; ideally make the renderer consume the report directly.
3. Remove incidence-column aliases, row normalization, display-text IDs, and all
   calls to `captureTerrainResultSets` from buffered query, streaming query, and
   WASM query paths.
4. Remove the requirement to run `traqula/terrain.traqula` before opening Terrain.
   Keep that fixture only as a golden backend test.
5. When Terrain opens:
   - HTTP mode calls `/v1/terrain`;
   - Local WASM calls `wasmEngine.terrain(...)`;
   - show loading, empty, ready, stale, and error states explicitly.
6. Add a compact refresh command in Terrain. Query execution may mark an existing
   report stale; entering Terrain refreshes it. Do not infer mutation effects
   from source strings.
7. Preserve `resolved_as_of` in the UI. A prepared query from Current uses that
   exact cutoff token, not a newly evaluated `@NOW`.
8. Use backend Role, profile, isopleth, and signature IDs. Never hash display
   names for identity.
9. Support multiple relationship signatures in the report, with the backend's
   `default_signature_id` initially selected.
10. Render mask-zero allocations in the inspector without an isopleth edge.
11. Feature-detect `wasmEngine.terrain`. If the loopback fallback package lacks
    it, show a clear incompatible-package error; never silently fall back to
    query-derived Terrain.
12. Rename source status from `Query data` to `Database snapshot` (HTTP) or
    `Browser database snapshot` (WASM).

## Golden Fixture

Keep `traqula/terrain.traqula`, but the test should populate the database and call
the Rust Terrain API directly. It must no longer depend on the fixture's two
final incidence searches.

Expected values for the existing fixture:

| Measurement | History | Current |
| --- | ---: | ---: |
| Endpoint Things | 6 | 6 |
| Roles in active sets | 8 | 8 |
| Appearance sets | 24 | 24 |
| Posits | 26 | 24 |
| Incidences | 30 | 27 |

Shared projected attribute Roles are `name`, `hair color`, `height`,
`social security number`, `RFID`, and `beard color`, ranked by the normative
shared-projection rule. Supports are `6`, `3`, `2`, and `1` in both frames.

For `{owner, pet}`:

| Measurement | History | Current |
| --- | ---: | ---: |
| Appearance sets | 3 | 3 |
| Posits | 4 | 3 |
| Owner distinct / participation | 2 / 3 | 2 / 3 |
| Pet distinct / participation | 2 / 3 | 2 / 3 |

Allocations:

- Cass as owner: `1` distinct, `2` participations;
- Ada as owner: `1` distinct, `1` participation;
- Mochi and Pixel as pet: `2` distinct, `3` participations.

## Required Tests

### Core and frames

- Empty database containing only built-in Roles.
- Golden fixture exact deterministic report.
- In-memory and replayed stores produce byte-equivalent reports for fixed cutoff.
- Future posits absent from Current and present in History.
- `@BOT`, finite cutoff, `@NOW` override, and `@EOT`.
- Equal-time conflicts remain jointly current.
- Incomparable mixed-precision maxima remain jointly current.
- Later definitely ordered posit dominates earlier posit for the same set.
- No identity/append-order tie-breaking.

### Statistics and profiles

- Database totals include built-in and unused Roles; frame stats do not invent
  incidence for unused Roles.
- Singleton versus multi-appearance-set Role classification.
- Repeated posits do not multiply attribute support.
- Profile populations sum to endpoint Things.
- Relationship-only Thing receives mask-zero profile.
- Zero, one, eight, and more-than-eight candidate attribute Roles.
- Shared projection and bit assignments are identical across frames.
- NFC/unusual/backtick/comma Role names remain distinct by Role identity.

### Isopleths

- Every populated nonzero exact profile emits one isopleth.
- Mask zero emits no isopleth.
- Superset support is exact and monotone.
- Deterministic ordering and IDs.
- Projection truncation sets `complete: false` and correct total count.

### Relationships

- No relationships.
- Binary and n-ary exact signatures.
- Multiple signatures with deterministic shared catalog/default.
- Signature present in one frame and absent in the other.
- Repeated historical posits increase relationship posit count only.
- Repeated endpoint participation distinguishes total from distinct Things.
- Allocation totals reconcile with each Role total.
- Mask-zero allocations are retained with no isopleth ID.
- Catalog truncation is deterministic and marked incomplete.

### Concurrency and limits

- A concurrent writer yields an entirely old or entirely new report, never a
  mixed capture.
- Separate `QueryInterface`s sharing one Database serialize with Terrain.
- Timeout/cancel while waiting for owner.
- Timeout/cancel during capture and aggregation.
- Every hard limit fails without partial core data.
- Counter overflow and structural-index key mismatch fail closed.

### Wire parity

- Unsupported Terrain version rejected before analysis.
- HTTP cutoff parsing, timeout clamp, no-store header, and structured errors.
- Rust, HTTP, and WASM reports are semantically equal for fixed cutoff/options.
- No Terrain SSE route/events.
- Frontend opens Terrain without running a query pair.
- HTTP and WASM render the same report.
- Stream toggle has no effect on Terrain.
- Old WASM fallback without `terrain` fails visibly.
- Refresh after mutation replaces the report; failures preserve the previous
  report as stale rather than displaying partial data.

## Definition Of Done

Terrain is complete when:

1. opening Terrain requires no Traqula script;
2. Rust directly computes one atomic versioned report from canonical structures;
3. History and Current share one projection and relationship catalog;
4. the report is bounded, cancellable, deterministic, and fail-closed;
5. Rust, HTTP, and WASM pass parity tests;
6. the frontend contains no query-row aggregation fallback;
7. the existing visual and geometric interaction tests pass against the backend
   report;
8. `TERRAIN.md`, `README.md`, and `CONTRACTS.md` describe the authoritative
   implementation rather than a paste-to-populate workflow.
