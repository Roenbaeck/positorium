# Positorium Beta Decisions

This document records the decisions that must be made before Positorium's beta
contracts are implemented and frozen. It is a decision workbook, not a final
specification. Edit the response blocks directly.

The roadmap and implementation should follow accepted decisions in this file.
When a decision changes, record the replacement rather than silently rewriting
history after a beta format or API has been published.

## How To Use This Document

For each decision:

1. Choose an option, combine options, or describe another choice.
2. Change `Status` from `Unresolved` to `Accepted`, `Deferred`, or `Rejected`.
3. Complete the response block, especially when departing from the recommendation.
4. Add the acceptance date before implementation depends on the decision.
5. Turn accepted decisions into specifications and executable contract tests.

Suggested statuses:

- `Unresolved`: discussion is still open.
- `Accepted`: this is the contract implementation should follow.
- `Deferred`: explicitly outside the first beta.
- `Rejected`: considered and intentionally not selected.
- `Superseded by Dxxx`: replaced by a later recorded decision.

## Implementation Gate

The following must be accepted before their respective implementation begins:

- Core model and time: D001-D011
- Traqula beta semantics: D012-D019
- Append-only persistence format: D020-D027 and D031
- Public beta compatibility: D028-D030

Non-format groundwork can begin earlier: property tests, typed AST construction,
storage interfaces, error propagation, and single-owner execution.

---

## Established Value Philosophy

**Status:** Accepted on 2026-08-20

Positorium uses a WYSIWYG value model. A value's user-visible literal is logical
data; an internal datatype or codec is not. The engine may inspect a literal and
select the most compact lossless encoding, including different encodings for
small and large numbers, provided retrieval reconstructs the entered value as
defined by the lexical-fidelity contract.

Keep these layers separate:

1. **Literal representation:** the user-visible value and its expressed
  precision, scale, spelling, or structure.
2. **Semantic interpretation:** nominal and possible-value meanings used by
  comparison operators where the literal family supports them.
3. **Physical codec:** an invisible, versioned storage optimization that may
  change without changing proposition identity or query behavior.
4. **Constraint:** an optional, subjective rule describing what values are
  expected or permitted in a modeling context. Constraints, not datatypes,
  express conformance.

The governing round-trip law is:

```text
render(decode(encode(literal))) = literal
```

Physical codec tags, integer widths, compression choices, and codec versions are
excluded from proposition identity. Re-encoding or compacting a value must not
create a new posit. User-facing Traqula does not expose casts, storage types, or
datatype declarations merely to constrain data.

As accepted in D002, lexical fidelity preserves the complete UTF-8 value token,
including leading zeros, explicit signs, JSON whitespace/key order, and escape
choices. Whitespace and comments outside the value token are not part of the
value.

---

## Core Model

### D001: Role Identity And Names

**Status:** Accepted

**Blocks:** Core equality, role catalog, import, appearance-set encoding

A Role is itself a Thing. The model must decide whether role identity or role
name determines equality.

**Options:**

- A. Roles are equal by immutable Role Thing identity. The catalog enforces a
  unique canonical name for each role and a unique role for each canonical name.
- B. Roles are equal by normalized name. The numeric identity is only an internal
  storage handle.
- C. Another rule described in the response.

**Recommendation:** A. It is consistent with the claim that roles are Things and
allows names to be treated as catalog metadata rather than hidden identity.

**Questions to settle:**

- Are role names case-sensitive?
- Is Unicode normalization applied?
- Can a role be renamed, or does renaming create a new role?
- Are aliases allowed, and if so, are they catalog metadata or ordinary posits?

**Suggestion:** A. Names case-sensitive, NFC-normalized at the parser boundary;
roles are immutable, so "rename" creates a new role and aliases are deferred to
ordinary posits after beta. Note that the current implementation is internally
inconsistent either way: `Role::PartialEq` compares the name case-sensitively
while `Hash` uppercases the name and mixes in `reserved` (see TODO.md), so
accepting A requires reworking `Eq`/`Hash`/`Ord` to identity-based equality.

**Response:**

- Choice: A. Role equality, ordering, and hashing use immutable Role Thing
  identity. The catalog enforces one canonical name per role and one role per
  canonical name.
- Name normalization: Names are case-sensitive and NFC-normalized at the parser
  and catalog boundary.
- Rename/alias policy: Roles are immutable. A rename creates a new role. Aliases
  are represented as ordinary posits; dedicated alias features are deferred
  beyond beta.
- Reasoning: Identity-based equality is consistent with roles being Things and
  keeps mutable catalog metadata out of equality. The implementation must replace
  the current name-based `Eq`/`Hash`/`Ord` behavior.
- Accepted on: 2026-08-26

### D002: Posit Proposition Equality

**Status:** Accepted

**Blocks:** Deduplication, identity assignment, replay, import

A posit has a Thing identity, but that identity should not make two otherwise
identical propositions different.

**Options:**

- A. Proposition equality is exactly `(AppearanceSet, LiteralValue, Time)`. The
  canonical posit Thing is assigned to that proposition and excluded from
  equality and ordering.
- B. Every append creates a distinct posit, even when all proposition fields are
  identical.
- C. Another rule described in the response.

**Recommendation:** A. Re-adding identical content should be idempotent. Separate
assertion posits should represent repeated observation, provenance, or evidence.

**Questions to settle:**

- Which portions of the entered value token belong to `LiteralValue` identity?
- Are JSON whitespace, key order, and escape spelling proposition-significant?
- Is an unasserted duplicate add completely invisible?

**Suggestion:** A. `10` and `10.0` are different propositions because the literal
preserves expressed precision, not because the engine assigned different storage
types. Decide separately whether non-precision spelling such as `10`, `010`, and
`+10`, or structurally equivalent JSON spellings, is also identity-bearing. A
duplicate literal returns the canonical posit and is otherwise invisible.
Accepting A also requires fixing `Posit`'s derived `Ord`, which currently includes
the identity field that `PartialEq` excludes (see TODO.md).

**Response:**

- Choice: A. Proposition equality is exactly `(AppearanceSet, LiteralValue,
  Time)`; the canonical posit Thing is excluded from equality and ordering.
- Lexical-fidelity boundary: The complete UTF-8 value token is identity-bearing,
  including numeric spelling, explicit signs, leading zeros, string escape
  choices, and whitespace internal to structured literals. Comments and
  whitespace outside the token are excluded. No Unicode normalization is
  applied to literal values.
- JSON literal identity policy: JSON token spelling, whitespace, object key order,
  number spelling, and escape choices are identity-bearing. Structural equality
  is a separate nominal comparison under D017.
- Reasoning: Raw-token fidelity is the only unambiguous WYSIWYG contract and
  cleanly separates literal identity from semantic comparison and physical
  codecs. Re-adding an identical proposition returns its canonical posit and has
  no other effect. `Posit::Ord` must use the same proposition key as equality.
- Accepted on: 2026-08-26

### D003: Thing Identity Scope And Import

**Status:** Accepted

**Blocks:** Store UUID, backup, export/import, database merging

Numeric Thing identities can collide when two independent stores are combined.

**Options:**

- A. Things are store-local `u64` values. Every store has an immutable UUID, and
  import remaps identities while preserving all internal references.
- B. Replace Things with globally unique identifiers.
- C. Keep local identifiers but expose a composite `(store UUID, local Thing)`
  identity outside the engine.
- D. Another rule described in the response.

**Recommendation:** A internally, with C in logical exports and external APIs
where stable cross-store references are needed.

**Suggestion:** Follow the recommendation (A internally, C externally). Put the
store UUID in the manifest and every file header (ties into D023/D024); logical
exports address things as `(store UUID, local u64)`; import builds an explicit
remap table and rewrites every internal reference during replay — foreign local
ids are never preserved verbatim.

**Response:**

- Choice: A internally and C in logical exports and external APIs.
- External identity representation: `(store UUID, local u64)` wherever a stable
  cross-store reference is required. The immutable store UUID is recorded in the
  manifest and each log file header.
- Collision/remapping policy: Import always creates an explicit remap table and
  rewrites every internal reference. Foreign local identifiers are never retained
  verbatim.
- D006 compatibility clarification: The five fixed built-in Roles map to the
  same destination identities in every store: `posit` = 1, `ascertains` = 2,
  `thing` = 3, `class` = 4, and `subclass` = 5. Every other foreign local
  identity is remapped and receives a different local number.
- Reasoning: Compact local identities suit the in-memory engine, while composite
  external identities prevent accidental cross-store collisions and make import
  semantics explicit.
- Accepted on: 2026-08-26

### D004: Identity Equivalence And Merging

**Status:** Accepted

**Blocks:** Identification cookbook, imports, future canonicalization policies

Later evidence may suggest that two Things refer to the same external entity.
Destructively merging log history would rewrite appearance sets and assertions.

**Options:**

- A. Destructive merge is forbidden. Equivalence, replacement, and disagreement
  are represented using ordinary posits and interpreted by explicit query policy.
- B. Permit a permanent alias/remap record in the storage layer.
- C. Permit destructive offline rewriting into a new store only.
- D. Another rule described in the response.

**Recommendation:** A for beta. An offline export/import transformation can be
considered later without changing the logical model.

**Suggestion:** A. Since an appearance set holds at most one thing per role
(D005), symmetric equivalence needs a documented cookbook pattern: a reified
identification Thing with one membership posit per equated Thing, plus posits
carrying the evidence/certainty for the claim. Queries opt into equivalence
explicitly; no storage-layer alias or remap records in beta.

**Response:**

- Choice: A. Destructive merge and storage-layer aliases are forbidden in beta.
- Required equivalence roles/patterns, if any: The modeling cookbook will define
  a reified identification Thing, one membership posit for each identified Thing,
  and separate posits for evidence and certainty. No additional built-in roles
  are reserved by this decision.
- Reasoning: Equivalence is evidence that can itself conflict or change. Keeping
  it in ordinary posits preserves history and makes equivalence an explicit query
  policy rather than hidden storage behavior.
- Accepted on: 2026-08-26

### D005: Appearance-Set Cardinality And Value Slots

**Status:** Accepted

**Blocks:** Core model, modeling guidance, exact matching, snapshots

An AppearanceSet currently permits at most one Thing for each Role. Posits with
one exact appearance set form a sequence of possible values over time.

**Options:**

- A. Preserve this rule. An appearance set is a finite partial function
  `Role -> Thing` and one value-bearing transition slot. Simultaneous aliases,
  tags, memberships, or repeated participants use reified relation/member Things.
- B. Allow a role to appear more than once, making an appearance set a general
  set of `(Thing, Role)` pairs.
- C. Add a separate slot/key construct to distinguish repeated roles.
- D. Another rule described in the response.

**Recommendation:** A. It gives change and `as of` a precise grouping key. The
cost is that repeated/multivalued structures need documented modeling patterns.

**Suggestion:** A. Multivalued attributes: one reified member Thing per value so
each member gets its own appearance set and timeline. Repeated participants: a
reified relation Thing plus one posit per participant role. The invariant is
already enforced by `AppearanceSet::new`, but `Database::create_appearance_set`
unwraps its result and panics on violation — that must become an error before
the rule is a public contract (see TODO.md).

**Response:**

- Choice: A. An appearance set is a finite partial function `Role -> Thing` and
  identifies one value-bearing transition slot.
- Intended multivalue modeling pattern: Use one reified member Thing per value so
  every member has its own appearance set and timeline.
- Intended repeated-participant pattern: Use a reified relation Thing and one
  posit per participant role. Do not repeat a role within one appearance set.
- Reasoning: The invariant gives exact matching and snapshots a stable grouping
  key. Duplicate-role construction must return a domain error and never panic.
- Accepted on: 2026-08-26

### D006: Reserved Role Vocabulary

**Status:** Accepted

**Blocks:** Initial role catalog, assertions, classification examples

**Response:**

- Exact vocabulary: Reserve five Roles with fixed identities and names:
  `posit` (`1`), `ascertains` (`2`), `thing` (`3`), `class` (`4`), and
  `subclass` (`5`). Do not reserve `classification`, `named`, `superclass`, or
  constraint-specific roles.
- Assertion semantics: `posit` and `ascertains` define the exact assertion shape
  used by assertion-aware operators such as `in effect`.
- Classification boundary: `thing`, `class`, and `subclass` are stable vocabulary
  for interchange, raw queries, and presentation conventions. Reserving them does
  not make the core interpret a posit's value, validate classifier shapes, derive
  membership, traverse subclass relationships, or reject a write.
- Conventional shapes: Consumers may treat unary `{class}` as a class
  declaration, binary `{thing, class}` as direct classification, and binary
  `{subclass, class}` as a subclass statement. A consumer must state its value,
  source, certainty, temporal, and inheritance policies explicitly.
- Value neutrality: Strings such as `"active"` and `"inactive"` are ordinary
  literal values. They have no database-defined classification meaning.
- Descriptions: Names, labels, and descriptions use ordinary application Roles.
  Class identity is the Thing identity, never a current display name.
- Compatibility: No Positorium release predates this vocabulary. Update the
  unreleased store bootstrap and contracts directly; do not add a migration.
- Reasoning: Stable Role identities make classification posits portable and
  discoverable without turning Positorium into an ontology engine or assigning
  hidden truth semantics to application values.
- Revised and accepted on: 2026-08-28

---

## Time And Snapshots

### D007: Meaning Of Imprecise Time

**Status:** Accepted

**Blocks:** Time encoding, comparisons, snapshots, indexes

Year, year-month, date, and datetime values have different precision. Treating a
year as an exact point discards the imprecision represented by the input.

**Options:**

- A. Each value denotes a closed or half-open interval/granule. Comparisons use
  interval relations and may be indeterminate.
- B. Each value is a point tagged with precision. Values at different precision
  are incomparable unless explicitly coerced.
- C. Normalize every value to the beginning of its represented period and retain
  precision only for display.
- D. Another rule described in the response.

**Recommendation:** A, preferably half-open intervals such as
`2024 = [2024-01-01, 2025-01-01)`. This preserves meaning and composes with
explicit `definitely`, `possibly`, and overlap predicates.

**Questions to settle:**

- Are intervals half-open or closed?
- What timezone does a datetime use?
- Are leap seconds relevant?
- Do `@BOT` and `@EOT` denote unbounded interval endpoints?

**Suggestion:** A with half-open intervals (`2024 = [2024-01-01, 2025-01-01)`).
Datetimes are UTC without leap seconds (chrono's `NaiveDateTime` already ignores
them); clients convert local time before submission. `@BOT`/`@EOT` are unbounded
endpoints below/above everything. Interval semantics also cleanly fixes today's
law violation where `TimeType::partial_cmp` answers `Equal` for a Year and a
Date inside it while `PartialEq` says they differ, and where the derived `Ord`
disagrees with the manual `PartialOrd` entirely (see TODO.md).

**Response:**

- Choice: A. Every time literal denotes a precision-preserving interval.
- Interval boundary convention: Intervals are half-open. For example, `2024`
  denotes `[2024-01-01T00:00:00Z, 2025-01-01T00:00:00Z)`.
- Datetime/timezone policy: Datetimes are interpreted as UTC. Traqula has no local
  timezone or offset coercion in beta; clients convert before submission. Leap
  seconds are not supported.
- BOT/EOT policy: `@BOT` and `@EOT` are unbounded sentinels below and above every
  finite interval, respectively.
- Reasoning: Half-open intervals preserve stated precision without manufacturing
  an exact point and give comparison, overlap, and snapshot operations one
  consistent model. `Time` equality and ordering implementations must obey these
  semantics without conflicting derived traits.
- Accepted on: 2026-08-26

### D008: Temporal Comparison Vocabulary

**Status:** Accepted

**Blocks:** `where`, mixed-precision queries, query planner

Under interval or partial-order semantics, ordinary `<` may be definitely true,
possibly true, or indeterminate.

**Options:**

- A. `<`, `<=`, `>`, and `>=` mean definitely before/after. Add explicit
  `possibly before`, `overlaps`, `contains`, and related predicates.
- B. Comparisons include any possible ordering, so `<` means possibly before.
- C. Reject mixed-precision comparisons unless the user selects a relation.
- D. Another rule described in the response.

**Recommendation:** A. Conservative ordinary comparisons avoid returning facts
that are not definitely within the requested temporal boundary.

**Suggestion:** A. Minimal predicate set: `<`, `<=`, `>`, `>=` meaning definitely
before/after, plus `possibly before/after`, `overlaps`, `contains`, and `within`.
`=` means identical stored time (same precision and value); granule questions use
`overlaps`/`contains`. Indeterminate relations evaluate to false under the
definite operators rather than erroring, so mixed-precision data stays queryable.

**Response:**

- Choice: A. Ordinary ordering operators are conservative, definite relations.
- Required temporal predicates: `<` and `>` mean definitely before and after.
  `<=` and `>=` additionally include identical stored times. Beta also provides
  `possibly before`, `possibly after`, `overlaps`, `contains`, and `within`.
- Equality meaning: `=` means identical stored time, including precision and
  value. It is not interval overlap.
- Reasoning: Mixed-precision data remains queryable without treating a possible
  ordering as fact. An indeterminate definite relation evaluates to false;
  explicit interval predicates express broader questions.
- Accepted on: 2026-08-26

### D009: Snapshot Maxima And Conflicts

**Status:** Accepted

**Blocks:** `as of`, ordinary state queries, deterministic tests

At a cutoff, several posits for one appearance set can share the latest time or
have incomparable imprecise times.

**Options:**

- A. Return every maximal applicable posit. Preserve equal-time conflicts and
  incomparable maxima.
- B. Select one posit by deterministic identity order.
- C. Treat multiple maxima as a query error.
- D. Another rule described in the response.

**Recommendation:** A. Positorium is intended to preserve conflict. Identity or
append order must not acquire truth semantics.

**Suggestion:** A. Applicability at cutoff `T`: a posit applies when its time is
definitely at-or-before `T` (its half-open interval lies entirely at or below
the cutoff). Return every maximal applicable posit under the definite partial
order, preserving equal-time and incomparable maxima as multiple rows; ties are
visible in results and never resolved by identity or append order.

**Response:**

- Choice: A. Return all maximal applicable posits for each appearance set.
- Applicability rule at cutoff: A posit applies when its time is definitely before
  the cutoff or is the identical stored time. Equivalently, it satisfies the D008
  definite `<=` relation.
- Tie/incomparability behavior: Preserve equal-time conflicts and incomparable
  maximal intervals as separate result rows. Identity and append sequence are
  never tie-breakers.
- Reasoning: Selecting one row would silently add truth semantics to an internal
  identifier or storage order, contrary to Positorium's conflict-preserving model.
- Accepted on: 2026-08-26

### D010: Snapshot And Value-Filter Order

**Status:** Accepted

**Blocks:** `as of` expansion and optimizer correctness

These are different questions:

- "What was the state at T, and was that state married?"
- "What was the latest historical `married` posit by T?"

**Options:**

- A. `pattern as of T` first finds the appearance sets selected by the appearance
  pattern, reduces all their values to the snapshot, and then applies value
  predicates. Provide separate syntax for latest matching history.
- B. Filter by the entire pattern first, then take the latest matching posit.
- C. Expose both operations explicitly and make `as of` unavailable until one is
  selected by name.

**Recommendation:** A for ordinary state queries, plus an explicit history
operation for B.

**Suggestion:** A. `pattern as of T` reduces each matching appearance set to its
snapshot first, then applies value predicates. Provide the B semantics through
an explicit form (e.g. `latest` on a filtered pattern, desugaring to filter →
group by appearance set → max time in the D015 algebra) so both questions are
expressible, and add the equivalence tests between shorthand and expansion that
the roadmap already calls for.

**Response:**

- Choice: A. Ordinary `[pattern] as of T` selects appearance sets by appearance
  structure, reduces each to its D009 snapshot, and only then applies value and
  other posit-field predicates.
- Latest-matching-history syntax/operation: `latest [pattern] as of T` performs
  pattern filtering first and then returns every maximal matching posit per
  appearance set. It desugars to filter, group by appearance set, and partial-order
  maximal selection in the D015 algebra.
- Reasoning: State-at-time and latest-matching-history answer different questions;
  distinct syntax prevents optimizer order from changing meaning. Both forms
  require shorthand-versus-expansion contract tests.
- Accepted on: 2026-08-26

### D011: Evaluation Of `@NOW`

**Status:** Accepted

**Blocks:** Repeatability, multi-pattern snapshots, testing

**Options:**

- A. Evaluate once at the beginning of a complete script.
- B. Evaluate once per command/search.
- C. Evaluate independently at every occurrence.
- D. Require clients to provide the current time; do not evaluate it in the engine.

**Recommendation:** A, with the resolved value available in execution metadata.
It makes every use within a script consistent and tests reproducible.

**Suggestion:** A — resolve once per script, expose the resolved value in
execution metadata, and accept an API-supplied override time for reproducible
tests. Precision: full datetime, as `Time::new()` produces today. Note the
current engine evaluates `@NOW` independently at every occurrence
(`parse_time_constant` constructs `Time::new()` per parse site in traqula.rs),
so two `@NOW`s in one script already differ — a change is required regardless
of the option chosen (see TODO.md).

**Response:**

- Choice: A. Resolve `@NOW` once at the beginning of a complete script and reuse
  that value at every occurrence.
- Precision of generated time: Full UTC datetime at the engine's supported
  subsecond precision.
- Override/parameter policy: Execution options may supply the resolved value for
  deterministic tests and replay. The engine exposes the resolved value in
  execution metadata.
- Reasoning: Script-scoped resolution makes multi-command and multi-pattern
  behavior coherent and reproducible without requiring every client to provide a
  clock value.
- Accepted on: 2026-08-26

---

## Traqula

### D012: Allocation And Query Variable Syntax

**Status:** Accepted

**Blocks:** Traqula grammar, AST, documentation, migration

The current `+x` syntax means allocation during `add` and binding during
`search`, and its domain depends on its position.

**Options:**

- A. Keep `+x` for allocation in `add`; use `?x` for declarative query variables.
  Bind posit identity explicitly as `?p = [ ... ]`.
- B. Keep `+x` in both contexts and rely on command/position for meaning.
- C. Use keywords such as `new x` and `bind x`.
- D. Another syntax described in the response.

**Recommendation:** A. It preserves compact posit patterns while separating
mutation from unification.

**Example:**

```traqula
add posit [{(+alice, name)}, "Alice", '2023-01-01'];

search ?p = [{(?person, name)}, ?name, ?since]
return ?p, ?person, ?name, ?since;
```

**Suggestion:** A. `+x` stays allocation-only inside `add`; `?x` is the query
variable; `?p = [...]` binds posit identity explicitly. Migration: one beta
release parses legacy search-side `+x`/bare recalls with a deprecation warning
and a mechanical rewrite hint, then removes them. Update `traqula.pest` and both
copies of `traqula.tmLanguage.json` in the same change.

**Response:**

- Choice: A.
- Preferred syntax: `+x` allocates only inside `add`; `?x` is a non-allocating
  query variable; and `?p = [...]` explicitly binds posit identity. Variables
  have checked Thing, Role, AppearanceSet, LiteralValue, Time, or Posit domains.
- Compatibility/deprecation approach: The default grammar adopts the new syntax.
  An explicitly selected legacy Traqula version accepts search-side `+x` and bare
  variable recalls for one beta minor release, emits deprecation warnings with
  mechanical rewrite hints, and is then removed. The Pest and VS Code grammars
  change together.
- Reasoning: Allocation and unification must be visibly distinct, and explicit
  posit binding removes position-dependent meaning. A versioned compatibility
  mode avoids silently changing old source semantics.
- Accepted on: 2026-08-26

### D013: Query Variable Scope

**Status:** Accepted

**Blocks:** Multi-command scripts, optimizer, client behavior

Current search bindings can persist implicitly across later searches.

**Options:**

- A. Variables are local to one `search`. Cross-search reuse requires an explicit
  `let`, named result, subquery, or `using` construct.
- B. Variables remain script-global and accumulate/intersect as commands execute.
- C. Both are supported, with explicit declaration of script-global variables.
- D. Another rule described in the response.

**Recommendation:** A, eventually adding explicit named results only when a real
workflow requires them. Ordered scripts remain useful without making each search
stateful.

**Suggestion:** A. Scope query variables to one `search`; keep `add posit`
binders script-visible for later `add` commands, which is a separate concern
from search scope. Defer `let`/named results until a real workflow demands them.
tests/cross_search_binding.rs currently pins the implicit retention behavior and
must be migrated with this decision.

**Response:**

- Choice: A. Query variables are lexical to one `search`. Allocation binders from
  `add` remain script-visible to later mutation commands but do not become query
  variables.
- Explicit cross-search mechanism, if required for beta: None. Defer `let`, named
  results, and `using` until a concrete workflow requires one.
- Reasoning: Local query scope makes each search independently understandable and
  optimizable. Ordered mutation remains possible without preserving hidden query
  result state. Existing cross-search-retention tests must be migrated.
- Accepted on: 2026-08-26

### D014: Exact And Open Appearance-Set Matching

**Status:** Accepted

**Blocks:** Pattern meaning, generic exploration, backwards compatibility

A pattern containing one appearance can mean either "exactly this set" or
"contains this appearance, possibly with others."

**Options:**

- A. A closed set is exact: `{(?x, name)}`. An ellipsis opens it for subset
  matching: `{(?x, name), ...}`.
- B. Current braces always mean subset matching; add an `exact` modifier.
- C. Current braces always mean exact matching; add a `contains` modifier.
- D. Another syntax described in the response.

**Recommendation:** A. The notation visually communicates whether the complete
stored structure is known.

**Questions to settle:**

- Can `?appearances` bind the complete set?
- Can `(?thing, ?role)` bind a Role for schema-free queries?
- Does `*` match one unknown field while `...` matches unknown set members?

**Suggestion:** A. Answers to the questions: yes, allow a variable to bind the
complete stored set (patterns already resolve to canonical appearance sets, so
this is cheap); yes, allow `(?thing, ?role)` for schema-free exploration; `*`
matches exactly one anonymous field while `...` means "and any other members".
Ship the exact/open split in the same migration as the D012 syntax change so
users rewrite scripts once.

**Response:**

- Choice: A. A closed appearance set is exact; a trailing ellipsis makes it an
  open subset pattern.
- Complete-set binding syntax: `?appearances = { ... }` binds the complete stored
  appearance set while the right-hand pattern controls exact or open matching,
  for example `?appearances = {(?thing, ?role), ...}`.
- Role-variable syntax: `(?thing, ?role)` binds a Role-valued query variable and
  supports schema-free exploration.
- Wildcard/ellipsis meaning: `*` consumes exactly one anonymous field and never
  binds it. `...` permits zero or more additional appearance-set members.
- Compatibility approach: Ship with D012. For one beta minor release, old subset
  semantics are available only through the explicitly selected legacy Traqula
  version; the default grammar never gives closed braces two meanings.
- Reasoning: Exactness is visible at the use site, complete-set and Role bindings
  enable generic tooling, and version selection prevents a silent semantic break.
- Accepted on: 2026-08-26

### D015: Query Algebra Required For Beta

**Status:** Accepted

**Blocks:** AST, planner, `as of` desugaring

**Candidate operations:**

- Typed posit scan with exact/open appearance matching
- Natural join through repeated variables
- Typed selection/filter
- Projection
- Union
- Safe anti-join / `NOT EXISTS`
- Grouping and maximal value selection
- Distinctness
- Ordering
- Limit
- Optional / left join
- General aggregation (`count`, `min`, `max`, `collect`)

**Options:**

- A. Implement all candidates except Optional and general aggregation for beta.
- B. Implement only scan, join, filter, projection, grouping/max, and limit.
- C. Another subset described in the response.

**Recommendation:** A. It is enough to expand `as of`, express absence of evidence,
and make result limiting deterministic. Optional and broad aggregation can wait.

**Suggestion:** A. The nucleus (scan, join, filter, projection, union, safe
anti-join, grouping/max, distinct, ordering, limit) is exactly what the D010
desugaring, D016 negation, and D018 deterministic limiting require. Defer
OPTIONAL and general aggregation; if pressure appears post-beta, add `count`
first since it composes with the existing bitmap indexes.

**Response:**

- Choice: A.
- Required operations: Typed posit scan with exact/open appearance matching,
  natural join through repeated variables, typed selection, projection, union,
  safe anti-join, grouping with partial-order maximal selection, `DISTINCT`,
  `ORDER BY`, and `LIMIT`.
- Deferred operations: Optional/left join and general aggregation. `count` is the
  first aggregation candidate after beta.
- Reasoning: This is the smallest coherent algebra that can define snapshot and
  latest-history expansion, absence queries, duplicate behavior, and deterministic
  limiting without evaluator-order accidents.
- Accepted on: 2026-08-26

### D016: Negation And Open-World Semantics

**Status:** Accepted

**Blocks:** `NOT EXISTS`, contradiction queries, documentation

Absence of a posit is not evidence that its proposition is false.

**Options:**

- A. Support only safe `NOT EXISTS`: no recorded match exists for the correlated
  pattern. Explicit negative certainty remains the way to represent contrary
  evidence.
- B. Treat missing data as false within a query's selected domain.
- C. Defer all negation from beta.

**Recommendation:** A, with prominent documentation that it is an absence query,
not logical negation.

**Suggestion:** A. Syntax: `not exists { <patterns> }` correlated with outer
variables, restricted to safe use (every variable inside is either bound outside
or local). Documentation should state plainly that it reports absence of
recorded evidence under an open world; negative certainty remains the way to
assert contrary evidence.

**Response:**

- Choice: A. Support safe absence queries only; missing data is never interpreted
  as a false proposition.
- Preferred syntax: `not exists { <patterns> }`. Correlated variables must already
  be bound by the outer query, variables introduced inside are local existential
  variables, and no inner binding may escape the block.
- Reasoning: This expresses “no recorded match exists” under an open world without
  conflating absence with contrary evidence. Negative certainty remains the way
  to record such evidence.
- Accepted on: 2026-08-26

### D017: Literal Interpretation And Comparison

**Status:** Accepted

**Blocks:** Predicates, posit equality, value results

**Options:**

- A. Literal families provide hidden semantic interpretations and explicitly
  supported relations. Queries use those relations without exposing storage
  datatypes or casts.
- B. Compare only identical literal representations and leave all semantic
  comparison to future constraint policies.
- C. Treat every literal as text for comparison.
- D. Another rule described in the response.

**Recommendation:** A. This preserves the WYSIWYG literal while allowing useful
queries. Semantic comparison never changes literal or proposition identity.

**Agreed operator semantics:**

- `a = b` means nominal equality under a relation supported by the two literal
  families. It may match values with different expressed precision, such as `6`
  and `6.00`, without making those values identical propositions.
- `a ?= b` means compatible with: the possible-value sets declared by the two
  literal values intersect. It never uses a hidden epsilon or implementation-defined
  tolerance. For two exact singleton values, compatibility reduces to nominal
  equality.
- Exact literal identity remains a separate relation. It compares all
  identity-bearing presentation information; its Traqula syntax is still to be
  chosen.
- A literal family that cannot define compatible possible values must reject
  `?=` or expose an explicit semantic relation rather than guessing.
- Failed or unsupported interpretation is not an implicit string conversion.
  Decide whether it is a predicate non-match or a query error, especially when a
  role contains heterogeneous unconstrained values.

**Questions to settle:**

- Are strings ordered, or equality-only?
- Is JSON structural equality supported?
- Does a constraint refine the possible-value interpretation used by `?=` or only
  report conformance?
- Does unsupported comparison yield false/unknown or fail the query?

**Suggestion:** A. Numeric literals compare nominally using an exact arbitrary-
precision representation, regardless of whether a physical codec used a varint,
fixed integer, or scaled coefficient. Strings use equality only for beta (defer
collation/ordering). JSON nominal equality may be structural while exact literal
identity remains presentation-sensitive. No user-visible cast syntax is needed.
The current evaluator's `f64` conversion and `1e-9` epsilon must be removed.

**Response:**

- Choice: A. `=` is nominal equality, `?=` is possible-value compatibility, and
  neither relation changes literal or proposition identity.
- Supported cross-family semantic relations: Integer and decimal values compare
  across their families using exact arbitrary-precision numeric equality and
  ordering. Certainty compares only with certainty using its exact percentage
  value; it is not an ordinary number. Current numeric, string, certainty, and
  JSON literals denote singleton possible values, so `?=` reduces to nominal
  equality for them. Time values use the D007 interval relations. A future
  literal family may define a non-singleton possible-value set explicitly; no
  family receives an inferred epsilon or tolerance.
- String ordering policy: Strings support equality only in beta, comparing their
  Unicode scalar sequences without normalization. Collation and ordering are
  deferred.
- JSON comparison policy: Nominal equality is structural: object key order and
  insignificant whitespace are ignored, array order is significant, and numbers
  compare by exact numeric value. Duplicate object keys are rejected. Exact
  identity remains presentation-sensitive under D002.
- Constraint/interpretation interaction: A future constraint evaluator may
  report conformance but must not refine nominal or possible-value
  interpretations. The beta defines no general constraint engine.
- Unsupported-comparison behavior: Fail the query with a typed comparison error,
  including when a heterogeneous role produces an unsupported operand pair. Do
  not silently return false or convert to strings.
- Exact literal identity syntax: `===`. The existing `==` spelling is not beta
  syntax; the D012 legacy grammar treats it as nominal `=` with a deprecation
  warning for one beta minor release.
- Reasoning: Preserve precision in proposition identity while allowing useful
  value-level matching. Compatibility is explicit denotational overlap, not
  approximate equality with an arbitrary tolerance. The evaluator must replace
  its current `f64` conversion and epsilon comparison.
- Accepted on: 2026-08-26

### D018: Result Cardinality And Ordering

**Status:** Accepted

**Blocks:** Rust/HTTP/WASM results, streaming, `LIMIT`

**Options:**

- A. Joins have bag semantics. `DISTINCT` removes duplicates. Row order is
  unspecified unless `ORDER BY` is present. `LIMIT` applies after filtering,
  projection/distinctness, and ordering.
- B. All query results are sets with implicit duplicate elimination.
- C. Results have a deterministic implicit identity/time order.
- D. Another rule described in the response.

**Recommendation:** A. It follows ordinary join behavior and avoids turning index
iteration order into an API promise.

**Suggestion:** A. No default ordering; pipeline is filter → projection/DISTINCT
→ ORDER BY → LIMIT. The streaming path already emits a `limited` flag in its end
event — formalize that as the "more rows were available" signal in the versioned
SSE schema rather than inventing a second mechanism.

**Response:**

- Choice: A. Joins have bag semantics and `DISTINCT` is explicit.
- Default ordering, if any: None. Row order is unspecified without `ORDER BY`;
  index or append order is not an API promise.
- LIMIT pipeline position: Filtering, then projection/`DISTINCT`, then `ORDER BY`,
  then `LIMIT`.
- Streaming `more available` policy: The versioned SSE end event uses
  `limited: true` exactly when additional rows existed beyond the applied limit.
- Reasoning: Explicit distinctness and ordering preserve ordinary join behavior
  while keeping internal index choices free to change.
- Accepted on: 2026-08-26

### D019: Role And Parameter Syntax

**Status:** Accepted

**Blocks:** Grammar stability, safe client queries, namespaces

**Options:**

- A. Bare role names remain convenient, with a quoted role form for spaces,
  punctuation, reserved words, and future namespaces. Add typed query parameters
  distinct from variables.
- B. Require every role name to be quoted.
- C. Restrict role names to identifier syntax for beta.
- D. Another rule described in the response.

**Recommendation:** A. Parameters must be bindable through the API without clients
constructing Traqula source strings.

**Suggestion:** A. Bare role names stay identifier-like; add a backtick-quoted
form (`` `role name` ``) for spaces/punctuation/future namespaces — double
quotes already mean strings and single quotes mean times, so backticks avoid
grammar ambiguity. Parameters: `$name` placeholders bound through a JSON object
in the API request, typed like literals and never spliced into source text.

**Response:**

- Choice: A.
- Quoted role syntax: Bare role names remain identifier-like. Backticks quote a
  literal role name containing whitespace, punctuation, or a reserved word, for
  example `` `postal code` ``.
- Parameter syntax: `$name`, bound through a separate API object as a typed
  literal or time value. Parameters are never source text, role names, variable
  names, or syntax fragments.
- Namespace reservation: Unquoted `::` is reserved and rejected in beta for a
  possible future qualified-name syntax. All content inside backticks is forever
  literal, including `::`.
- Reasoning: Common roles stay concise, arbitrary names remain representable, and
  typed parameters let clients issue safe queries without source interpolation.
- Accepted on: 2026-08-26

---

## Append-Only Persistence

### D020: Durable Thing Allocation

**Status:** Accepted

**Blocks:** Record types, `create_thing`, replay identity generator

A Thing may be allocated before any role or posit refers to it.

**Options:**

- A. Allocation itself is durable through an explicit Thing record.
- B. Only Things reachable from durable roles and posits are durable. A standalone
  allocation is ephemeral until referenced by a committed construct.
- C. Remove standalone durable Thing allocation from the beta API.
- D. Another rule described in the response.

**Recommendation:** B or C. Logging every temporary allocation increases format
surface and permits empty identities with no modeled meaning.

**Suggestion:** C for beta: drop standalone durable Thing allocation from the
public promise; identities become durable when a committed role or posit first
references them (B is the fallback if the API must keep `create_thing`). Note
today's behavior differs on both counts: every generated thing is immediately
persisted (`create_thing` → `persist_thing`) and the generator recycles released
identities — both would change under this decision.

**Response:**

- Choice: C. Standalone durable Thing allocation is not part of the public beta
  API. Internally this has B semantics.
- API behavior before first durable reference: An allocated identity becomes
  durable only when a committed role or posit first references it. Failed or
  abandoned commands may leave gaps, but released identities are never recycled.
  The current `create_thing` is internal or explicitly unstable.
- Reasoning: Empty durable identities add format and API surface without modeled
  information. Monotonic non-reuse keeps replay and import safe without requiring
  allocation records.
- Accepted on: 2026-08-26

### D021: Atomicity Unit

**Status:** Accepted

**Blocks:** Commit records, execution rollback, API success semantics

**Options:**

- A. Each complete Traqula script is one atomic batch.
- B. Each semicolon-delimited command is atomic; earlier commands remain committed
  if a later command fails.
- C. Each individual role/posit is atomic unless an explicit transaction is used.
- D. Another rule described in the response.

**Recommendation:** B for beta, with explicit transaction/script atomicity added
later if needed. It supports streaming and bounds rollback while remaining easy
to explain.

**Suggestion:** B. Each semicolon-delimited command is one atomic batch; a
multi-posit `add posit` command commits all of its posits or none. Document
clearly that commands committed before a failing command stay committed.
Script-level transactions are a post-beta addition behind explicit syntax.

**Response:**

- Choice: B. Each semicolon-delimited command is one atomic batch. Commands
  committed before a later failure remain committed.
- Multi-posit `add posit` atomicity: Every role, Thing reference, and posit created
  by one command commits together or none of them do.
- Future transaction policy: Script-level atomicity is deferred to an explicit
  transaction construct after beta; it will not silently change default command
  semantics.
- Reasoning: Command batches provide a clear success boundary, support streaming,
  and bound rollback work without exposing partially applied commands.
- Accepted on: 2026-08-26

### D022: Durability Levels And Acknowledgment

**Status:** Accepted

**Blocks:** Flush behavior, API responses, benchmarks

**Options:**

- A. One durable mode: return success only after the commit record and required
  catalog data have been flushed with `fsync`.
- B. Offer `durable`, `buffered`, and `ephemeral` modes with explicit guarantees.
- C. Flush periodically and let success mean accepted into process memory.
- D. Another rule described in the response.

**Recommendation:** A for the first beta. Additional modes can be introduced once
there are concrete performance requirements and precise names for their risks.

**Suggestion:** A. Success means the commit record and any catalog records it
depends on are fsynced. Worth noting: the SQLite prototype runs
`synchronous=NORMAL` under WAL, which does not guarantee durability of every
acknowledged write — the new format's contract should be strict from day one.
Add `buffered`/`ephemeral` modes only when a real workload and precise risk
names exist for them.

**Response:**

- Choice: A. Beta has one persistent durability level.
- Success guarantee: In a persistent store, success is returned only after the
  command's records, their required catalog records, and the commit frame are
  durably flushed. Persistence-disabled execution makes no disk durability claim.
- Flush policy: Flush the log through the commit frame before acknowledgment.
  Manifest, new-file, and directory-entry changes required to reopen the commit
  are also flushed before acknowledgment.
- Reasoning: One strict meaning of success is testable and avoids making a weak
  default permanent before real workloads justify additional modes.
- Accepted on: 2026-08-26

### D023: Physical File Set And Cross-File Consistency

**Status:** Accepted

**Blocks:** Manifest, role catalog, posit log, backup

**Options:**

- A. A manifest identifies the store UUID and active files. Roles/datatype metadata
  use an append-only catalog; posits use an append-only log. Posit commits record
  the required catalog high-water mark.
- B. Use one interleaved append-only file for all record types.
- C. Use separate files without cross-file sequence references.
- D. Another layout described in the response.

**Recommendation:** A if separate roles remain an explicit goal. B is operationally
simpler and should still be considered before the format is frozen.

**Questions to settle:**

- Is the size/inspection benefit of separate files worth cross-file commit logic?
- Are datatype definitions in the role catalog or a third catalog?
- Does one OS lock cover the complete store directory?

**Suggestion:** Prefer B: one interleaved append-only log plus a small manifest
(store UUID, format version, active file list). Interleaving turns the
"catalog entry durable before dependent posit" rule into a simple record-order
invariant within one file and eliminates cross-file commit logic; datatype
metadata become records in the same log; one OS lock on the manifest covers the
store. The inspection benefits attributed to separate files (option A) are
better served by the logical export/dump tool the roadmap already plans.

**Response:**

- Choice: B.
- File set: A small manifest contains the store UUID, format version, feature
  flags, and active log file list. One interleaved append-only log contains all
  ordered records and commit frames; future rotation may add immutable segments.
- Datatype metadata location: Role and physical-codec metadata are ordinary
  versioned records in the same log before records that depend on them.
- Lock scope: One OS-level store lock covers the manifest and every active or
  sealed log file. Beta permits one writer/owner for the complete store.
- Backup consistency mechanism: Hold the store's read/backup lock while copying
  the manifest and logs through a recorded committed length. Uncommitted tail
  bytes are not part of the backup.
- Reasoning: Interleaving reduces cross-file consistency to record order and one
  commit boundary. Logical export, not physical file separation, provides human
  inspection.
- Accepted on: 2026-08-26

### D024: Record Framing And Integrity

**Status:** Accepted

**Blocks:** Binary format specification and parser

**Options:**

- A. Fixed file header plus versioned, length-delimited records containing sequence,
  type, payload length, payload, and checksum. Commit records delimit batches.
- B. Self-describing serialization such as CBOR with an outer length/checksum frame.
- C. Fixed-size binary structures.
- D. Another format described in the response.

**Recommendation:** A, with an explicitly specified endian-independent payload.
A rolling cryptographic chain is optional and separate from corruption detection.

**Questions to settle:**

- Which checksum detects accidental corruption?
- Is a BLAKE3 chain still required, and what threat does it address?
- What maximum record size is accepted before allocation?

**Suggestion:** A. Payload: hand-specified little-endian encoding with varint
lengths. Checksum: CRC32C per record for accidental corruption (fast, hardware
supported). Keep the BLAKE3 chain only as a separate, optional tamper-evidence
feature — and if kept, hash committed record bytes in sequence order, unlike the
current chain which hashes a textual SQL projection of posit rows. Enforce a
maximum record size (e.g. 16 MiB) validated before allocation.

**Response:**

- Choice: A. Files have fixed headers and versioned, length-delimited records with
  sequence, type, payload length, payload, and checksum. Commit records delimit
  atomic batches.
- Payload encoding: A hand-specified, endian-independent binary format: fixed
  integers are little-endian and lengths use unsigned LEB128. Rust memory layouts
  and textual `Display` output are never persistence encodings.
- Checksum: CRC32C over the complete framed record except the checksum field.
- Rolling hash policy: No rolling cryptographic chain is required by the beta
  format. Reserve a manifest feature flag for a future optional BLAKE3 chain over
  committed framed bytes in sequence order; it must not be described as tamper
  proof without an external trusted anchor.
- Maximum record size: 16 MiB for the complete framed record, validated with
  checked arithmetic before allocation.
- Reasoning: Explicit framing supports safe scanning and recovery; CRC32C handles
  accidental corruption. Deferring an unauthenticated hash chain avoids a second
  integrity mechanism with no stronger beta guarantee.
- Accepted on: 2026-08-26

### D025: Recovery And Corruption Policy

**Status:** Accepted

**Blocks:** Replay, startup behavior, repair tools

**Options:**

- A. Ignore or truncate only an incomplete/uncommitted tail. Any checksum failure,
  unknown mandatory version, dangling reference, or corruption in committed
  history fails startup.
- B. Skip invalid records and continue replay.
- C. Open valid history read-only up to the first corrupt record.
- D. Another rule described in the response.

**Recommendation:** A as normal behavior. An explicit offline salvage tool may
implement C without allowing the server to silently serve partial history.

**Suggestion:** A. Truncate only records after the last commit frame; any damage
to committed history fails startup with a precise error naming the offset and
record. Ship option C later as a separate read-only salvage tool. The current
SQLite restore does the opposite — it silently skips unknown value types and
merely warns on restore failures — so this policy is a behavior change, not just
a format change (see TODO.md).

**Response:**

- Choice: A.
- Tail truncation policy: Writable recovery truncates only bytes after the last
  valid commit frame. An incomplete or checksum-invalid uncommitted tail is
  treated as a torn write. Corruption, unknown mandatory versions/codecs,
  dangling references, or malformed data within committed history fails startup
  with byte offset and record sequence.
- Read-only/salvage policy: Read-only open may ignore an uncommitted tail but may
  not hide committed corruption. A separate offline salvage tool may later expose
  valid history up to the first corrupt record and never overwrites the source.
- Reasoning: Normal startup must not silently turn corruption into missing facts;
  explicit salvage keeps partial recovery visible and auditable.
- Accepted on: 2026-08-26

### D026: Value Codec Registry

**Status:** Accepted

**Blocks:** Lossless value bytes, replay, migration

**Options:**

- A. Beta supports a closed registry of hidden, versioned physical codecs. Every
  codec losslessly reconstructs a logical literal; unknown required codecs fail
  replay. User-facing datatypes and custom type declarations do not exist.
- B. Use one raw UTF-8 literal codec for beta and add compact codecs later.
- C. Allow plugins to register custom physical codecs before beta.
- D. Another rule described in the response.

**Recommendation:** A, with a raw literal fallback. A plugin codec ABI would
significantly increase the compatibility surface and should be deferred.

**Suggestion:** A. Replace the current `DataType::UID` registry with storage-format
codec identifiers. Codec choice is never part of posit equality or user-facing
results and may change during migration or compaction. Preserve the literal's
family and exact presentation independently of codec choice. Unknown required
codec identifiers are a hard replay failure; custom codecs are deferred.

**Response:**

- Choice: A, with a mandatory raw-literal fallback.
- Built-in codec registry location: A closed registry in the storage-format
  specification and private persistence implementation, not the public datatype
  API.
- Codec versioning policy: The `(codec identifier, codec version)` pair has
  immutable decoding semantics. A changed encoding receives a new pair, and
  unknown required pairs fail replay.
- Raw literal fallback policy: Every beta literal family must support the raw
  UTF-8 token codec. Writers use it whenever no compact lossless codec applies.
- Custom codec policy: Plugin and user-defined physical codecs are deferred beyond
  beta.
- Reasoning: Physical codecs remain replaceable storage optimizations and can
  never affect literal identity, proposition identity, or query results.
- Accepted on: 2026-08-26

### D027: Format Evolution And Migration

**Status:** Accepted

**Blocks:** Beta compatibility promise, importer/exporter

**Options:**

- A. Readers support the current format and a documented set of older beta
  versions. Breaking changes use an explicit offline migration into new files.
- B. Only the current format is readable; every upgrade may require migration.
- C. The first beta format is permanently readable by every future release.
- D. Another rule described in the response.

**Recommendation:** A. Commit to logical data preservation and explicit migration,
not to indefinitely carrying every physical decoder inside the engine.

**Suggestion:** A. Minimum window: each beta release reads at least the
immediately preceding beta format. Breaking changes ship with an offline
migrator that writes new files beside the old ones, so rollback is "keep the old
files". The SQLite importer lives until the first stable release, then is
removed together with the `rusqlite` dependency.

**Response:**

- Choice: A.
- Minimum compatibility window: Each engine reads its current format and at least
  the immediately preceding beta format. Published standalone migrators remain
  available so older beta stores have a documented stepwise path forward.
- Migration backup/rollback policy: Breaking migration writes a new store beside
  the source, validates it before activation, and never mutates the old files.
  Rollback selects the retained old store.
- SQLite importer lifetime: Ship and support it through the beta series. Remove it
  and the `rusqlite` dependency at the first stable release, while retaining the
  last beta importer as an archived migration tool.
- Amendment: The SQLite-importer lifetime above is superseded by D031. The
  remaining current/previous beta format guarantees in this decision stay accepted.
- Reasoning: This guarantees a supported logical-data path without requiring the
  main engine to carry every historical physical decoder indefinitely.
- Accepted on: 2026-08-26

---

## Public Beta Boundaries

### D028: Stable Rust API Surface

**Status:** Accepted

**Blocks:** Crate release and semantic versioning

**Options:**

- A. Only high-level database, command, query, lossless literal result, and storage
  configuration APIs are beta-public. Keepers, lookups, parser internals, and
  physical storage types are private or explicitly unstable.
- B. Keep the current internals public and apply semantic versioning to them.
- C. Do not promise any Rust API stability during beta.

**Recommendation:** A. It preserves room to optimize indexes and replay without
breaking users.

**Suggestion:** A. Stable: a narrowed `Database` (construction + command/query
entry points), the Traqula execution API, lossless literal results, storage
configuration, and the error type. Explicitly unstable or private: keepers, lookups,
`ThingGenerator`, parser internals, and `Persistor` — all currently `pub` fields
on `Database`. Fold the `create_apperance` → `create_appearance` rename into the
same narrowing so the deprecation happens once.

**Response:**

- Choice: A.
- Stable modules/types: High-level `Database` construction/open and command/query
  entry points; opaque logical and external identity handles; execution options;
  lossless result and stream-event types; storage configuration; and
  `DatabaseError`.
- Explicitly unstable modules/types: Keepers, indexes and lookups,
  `ThingGenerator`, parser internals, AST/planner internals, `Persistor`, physical
  records and codecs, and all public fields currently exposing those details.
  Rename `create_apperance` to `create_appearance` during this narrowing.
- Reasoning: The beta contract should cover useful database behavior without
  freezing storage, parser, or indexing implementation details that are expected
  to change.
- Accepted on: 2026-08-26

### D029: HTTP Beta Trust Boundary

**Status:** Accepted

**Blocks:** Server documentation and defaults

**Options:**

- A. The beta server is trusted/local only, binds to loopback by default, has no
  authentication claim, and enforces request, runtime, and result limits.
- B. Add authentication and support untrusted network exposure before beta.
- C. Exclude the HTTP server from the beta contract.

**Recommendation:** A. Authentication does not need to block testing the database,
but the server must not imply safe Internet exposure.

**Suggestion:** A. Loopback is already the default bind in main.rs — keep it.
But close the gaps before beta: CORS is currently `allow_origin(Any)` and the
advertised `timeout_ms` request field is ignored (`let _timeout` in server.rs).
Tighten CORS to the local console origin, implement or remove the timeout, and
add request-size, runtime, and result-count limits (see TODO.md).

**Response:**

- Choice: A. The beta HTTP server is a trusted local interface with no
  authentication or safe-Internet-exposure claim.
- Default bind address: `127.0.0.1`. Binding to any non-loopback interface
  requires explicit configuration and does not change the trust claim.
- Request/runtime/result limits: Default request body 1 MiB, configurable up to a
  16 MiB hard maximum; at most 1,000 commands per script; 5-second default runtime
  with `timeout_ms` able to lower it and configuration able to raise it only to a
  30-second hard maximum; and at most 100,000 rows per search across buffered and
  streaming responses. Exceeding a limit returns a structured error or a
  `limited` completion, as appropriate.
- CORS default: Same-origin only. The server may allow an explicit configured list
  of exact loopback origins; wildcard origins are forbidden.
- Reasoning: Local-only scope avoids blocking beta on authentication, but enforced
  resource and origin boundaries are still required. Remove any advertised option
  that cannot be enforced before release.
- Accepted on: 2026-08-26

### D030: Beta Compatibility Promise

**Status:** Accepted

**Blocks:** Release notes, versioning, user expectations

**Options:**

- A. Preserve beta logical data through supported migration. Version Traqula, HTTP,
  SSE, WASM, and storage independently. Deprecate public syntax/API before removal
  where practical, but permit documented beta breaks at minor releases.
- B. Promise full backwards compatibility from the first beta.
- C. Make no compatibility promise until 1.0.
- D. Another policy described in the response.

**Recommendation:** A. It protects testers' data while leaving controlled room to
correct a young language and API.

**Suggestion:** A. Guarantee: logical data (roles, posits, assertions) survives
every beta upgrade through supported migration. Deprecation: one minor release
of warnings for syntax/API removals where practical. Versioning: 0.x SemVer
(minor may break with notes, patch is compatible) with independent version
numbers for the storage format, Traqula grammar, HTTP/SSE schema, and WASM
interface embedded in files and responses.

**Response:**

- Choice: A.
- Logical data guarantee: Roles, referenced Things, posits, assertions, exact
  literal tokens, and times survive every beta upgrade through a supported direct
  or stepwise migration path.
- Language/API deprecation window: Provide one beta minor release of warnings and
  mechanical guidance where practical. Security fixes, corruption fixes, and
  behavior that was never part of a published contract may change immediately
  with release notes.
- Versioning scheme: Use 0.x SemVer: minor releases may contain documented beta
  breaks and patch releases are compatible. Storage, Traqula, HTTP/SSE, and WASM
  have independent versions embedded in their files, requests/responses, or
  interface metadata as applicable.
- Reasoning: Testers get a durable logical-data promise and visible migration path
  while the young language and implementation retain controlled room to improve.
- Accepted on: 2026-08-26

### D031: Pre-Release SQLite Compatibility

**Status:** Accepted

**Supersedes:** Only the SQLite-importer lifetime clause in D027

No Positorium release has used the prototype SQLite backend, and there is no
legacy user data or published SQLite contract to preserve.

**Response:**

- Choice: Remove the SQLite backend, importer scaffolding, feature flag, and
  `rusqlite` dependency before the first beta. Do not build a SQLite migration
  path for an unpublished prototype format.
- Compatibility boundary: The first published append-only beta format begins the
  D027/D030 migration guarantee. Native logical export/import and physical backup
  remain required for that format.
- Reasoning: Carrying and testing an unused legacy path would enlarge the public
  and security surface without protecting released data. Removing it also keeps
  logical values independent of storage UIDs and SQL conversion types.
- Accepted on: 2026-08-26

### D032: Pre-Beta Traqula Compatibility

**Status:** Accepted

**Supersedes:** Only the one-minor legacy-grammar clauses in D012, D014, D017,
and D019

No Positorium release has published the search-side `+x`/bare-variable syntax,
implicit subset matching, or `==` comparison spelling. There is no released
Traqula source contract to migrate.

**Response:**

- Choice: Publish only the D012/D014/D017/D019 grammar in the first beta. Reject
  the unpublished spellings instead of carrying a legacy parser mode.
- Rewrite guidance: Development examples and tests are updated mechanically:
  search variables gain `?`, posit binders gain `=`, subset patterns gain a
  trailing `...`, and nominal `==` becomes `=`.
- Compatibility boundary: Traqula version 1 is the first published language
  contract. D030 deprecation windows begin only after that release.
- Reasoning: As with the unreleased SQLite prototype in D031, a compatibility
  mode would create permanent parser and test surface without protecting any
  released source or user data.
- Accepted on: 2026-08-26

---

## Decision Summary

Update this table as decisions are accepted.

| ID | Decision | Status | Choice |
| --- | --- | --- | --- |
| D001 | Role identity and names | Accepted | Identity; case-sensitive NFC names |
| D002 | Posit proposition equality | Accepted | Proposition tuple; exact UTF-8 token |
| D003 | Thing identity scope and import | Accepted | Local `u64`; external store UUID pair |
| D004 | Identity equivalence and merging | Accepted | Ordinary posits; no destructive merge |
| D005 | Appearance-set cardinality and slots | Accepted | Partial function `Role -> Thing` |
| D006 | Reserved role vocabulary | Accepted | Five fixed roles; classification values remain neutral |
| D007 | Meaning of imprecise time | Accepted | Half-open UTC intervals |
| D008 | Temporal comparison vocabulary | Accepted | Definite ordering plus interval predicates |
| D009 | Snapshot maxima and conflicts | Accepted | Return every maximal applicable posit |
| D010 | Snapshot and value-filter order | Accepted | Snapshot first; explicit `latest` history |
| D011 | Evaluation of `@NOW` | Accepted | Once per script, overrideable |
| D012 | Allocation and query variable syntax | Accepted | `+x` allocates; `?x` queries |
| D013 | Query variable scope | Accepted | Search-local; no beta cross-search state |
| D014 | Exact and open appearance matching | Accepted | Closed exact; ellipsis opens |
| D015 | Query algebra required for beta | Accepted | Core algebra; no optional/aggregation |
| D016 | Negation and open-world semantics | Accepted | Safe correlated `not exists` |
| D017 | Literal interpretation and comparison | Accepted | `===` literal; `=` nominal; `?=` compatible |
| D018 | Result cardinality and ordering | Accepted | Bags; explicit distinct and ordering |
| D019 | Role and parameter syntax | Accepted | Backtick roles; typed `$parameters` |
| D020 | Durable Thing allocation | Accepted | Durable on committed reference; no reuse |
| D021 | Atomicity unit | Accepted | One command per atomic batch |
| D022 | Durability and acknowledgment | Accepted | Acknowledge only after durable flush |
| D023 | Physical file set and consistency | Accepted | Manifest plus interleaved log |
| D024 | Record framing and integrity | Accepted | Binary frames, CRC32C, 16 MiB maximum |
| D025 | Recovery and corruption | Accepted | Truncate tail; fail on committed damage |
| D026 | Value codec registry | Accepted | Closed versioned codecs plus raw fallback |
| D027 | Format evolution and migration | Accepted | Previous-format reader plus offline migration |
| D028 | Stable Rust API surface | Accepted | High-level contracts only |
| D029 | HTTP beta trust boundary | Accepted | Loopback, same-origin, enforced limits |
| D030 | Beta compatibility promise | Accepted | Logical-data migration; versioned surfaces |
| D031 | Pre-release SQLite compatibility | Accepted | Remove unpublished prototype path now |
| D032 | Pre-beta Traqula compatibility | Accepted | Publish only the new grammar |
