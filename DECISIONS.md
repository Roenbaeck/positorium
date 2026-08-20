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
- Append-only persistence format: D020-D027
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

Still unresolved: whether lexical fidelity is byte-for-byte for the complete
value token (including leading zeros, explicit signs, JSON whitespace/key order,
and escape choices) or preserves only agreed meaningful presentation metadata.
Whitespace and comments outside the value token are not part of the value.

---

## Core Model

### D001: Role Identity And Names

**Status:** Unresolved  
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

- Choice:
- Name normalization:
- Rename/alias policy:
- Reasoning:
- Accepted on:

### D002: Posit Proposition Equality

**Status:** Unresolved  
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

- Choice:
- Lexical-fidelity boundary:
- JSON literal identity policy:
- Reasoning:
- Accepted on:

### D003: Thing Identity Scope And Import

**Status:** Unresolved  
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

- Choice:
- External identity representation:
- Collision/remapping policy:
- Reasoning:
- Accepted on:

### D004: Identity Equivalence And Merging

**Status:** Unresolved  
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

- Choice:
- Required equivalence roles/patterns, if any:
- Reasoning:
- Accepted on:

### D005: Appearance-Set Cardinality And Value Slots

**Status:** Unresolved  
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

- Choice:
- Intended multivalue modeling pattern:
- Intended repeated-participant pattern:
- Reasoning:
- Accepted on:

### D006: Reserved Role Vocabulary

**Status:** Unresolved  
**Blocks:** Initial role catalog, assertions, classification examples

The implementation and theory currently disagree about names such as `class` and
`classification`.

**Candidate roles:**

- `posit`
- `ascertains`
- `thing`
- `class` or `classification`
- `named`
- `subclass`
- `superclass`

**Options:**

- A. Reserve only the minimal assertion roles for beta: `posit` and
  `ascertains`. Treat classification roles as ordinary user roles until their
  semantics are specified.
- B. Freeze the complete candidate vocabulary now.
- C. Do not reserve names; identify built-ins only by fixed role identities.
- D. Another rule described in the response.

**Recommendation:** A, while still assigning stable catalog identities to those
minimal built-ins. Avoid freezing an unfinished class model.

**Suggestion:** A — reserve only `posit` and `ascertains`, with fixed catalog
identities persisted as compatibility data. The engine today also reserves
`thing` and `classification` in `Database::new` while the theory says `class`;
drop both from the reserved set for beta rather than freezing a name the theory
disagrees with. Reintroduce class vocabulary with the post-beta class layer.

**Response:**

- Choice:
- Exact beta vocabulary:
- Fixed identity policy:
- Reasoning:
- Accepted on:

---

## Time And Snapshots

### D007: Meaning Of Imprecise Time

**Status:** Unresolved  
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

- Choice:
- Interval boundary convention:
- Datetime/timezone policy:
- BOT/EOT policy:
- Reasoning:
- Accepted on:

### D008: Temporal Comparison Vocabulary

**Status:** Unresolved  
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

- Choice:
- Required temporal predicates:
- Equality meaning:
- Reasoning:
- Accepted on:

### D009: Snapshot Maxima And Conflicts

**Status:** Unresolved  
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

- Choice:
- Applicability rule at cutoff:
- Tie/incomparability behavior:
- Reasoning:
- Accepted on:

### D010: Snapshot And Value-Filter Order

**Status:** Unresolved  
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

- Choice:
- Latest-matching-history syntax/operation:
- Reasoning:
- Accepted on:

### D011: Evaluation Of `@NOW`

**Status:** Unresolved  
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

- Choice:
- Precision of generated time:
- Override/parameter policy:
- Reasoning:
- Accepted on:

---

## Traqula

### D012: Allocation And Query Variable Syntax

**Status:** Unresolved  
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

- Choice:
- Preferred syntax:
- Compatibility/deprecation approach:
- Reasoning:
- Accepted on:

### D013: Query Variable Scope

**Status:** Unresolved  
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

- Choice:
- Explicit cross-search mechanism, if required for beta:
- Reasoning:
- Accepted on:

### D014: Exact And Open Appearance-Set Matching

**Status:** Unresolved  
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

- Choice:
- Complete-set binding syntax:
- Role-variable syntax:
- Wildcard/ellipsis meaning:
- Compatibility approach:
- Reasoning:
- Accepted on:

### D015: Query Algebra Required For Beta

**Status:** Unresolved  
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

- Choice:
- Required operations:
- Deferred operations:
- Reasoning:
- Accepted on:

### D016: Negation And Open-World Semantics

**Status:** Unresolved  
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

- Choice:
- Preferred syntax:
- Reasoning:
- Accepted on:

### D017: Literal Interpretation And Comparison

**Status:** Unresolved  
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

- Choice: Partial decision: `=` is nominal equality and `?=` is compatible with.
- Supported cross-family semantic relations:
- String ordering policy:
- JSON comparison policy:
- Constraint/interpretation interaction:
- Unsupported-comparison behavior:
- Exact literal identity syntax:
- Reasoning: Preserve precision in proposition identity while allowing useful
  value-level matching. Compatibility is denotational overlap, not approximate
  equality with an arbitrary tolerance.
- Accepted on:

### D018: Result Cardinality And Ordering

**Status:** Unresolved  
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

- Choice:
- Default ordering, if any:
- LIMIT pipeline position:
- Streaming `more available` policy:
- Reasoning:
- Accepted on:

### D019: Role And Parameter Syntax

**Status:** Unresolved  
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

- Choice:
- Quoted role syntax:
- Parameter syntax:
- Namespace reservation:
- Reasoning:
- Accepted on:

---

## Append-Only Persistence

### D020: Durable Thing Allocation

**Status:** Unresolved  
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

- Choice:
- API behavior before first durable reference:
- Reasoning:
- Accepted on:

### D021: Atomicity Unit

**Status:** Unresolved  
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

- Choice:
- Multi-posit `add posit` atomicity:
- Future transaction policy:
- Reasoning:
- Accepted on:

### D022: Durability Levels And Acknowledgment

**Status:** Unresolved  
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

- Choice:
- Success guarantee:
- Flush policy:
- Reasoning:
- Accepted on:

### D023: Physical File Set And Cross-File Consistency

**Status:** Unresolved  
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

- Choice:
- File set:
- Datatype metadata location:
- Lock scope:
- Backup consistency mechanism:
- Reasoning:
- Accepted on:

### D024: Record Framing And Integrity

**Status:** Unresolved  
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

- Choice:
- Payload encoding:
- Checksum:
- Rolling hash policy:
- Maximum record size:
- Reasoning:
- Accepted on:

### D025: Recovery And Corruption Policy

**Status:** Unresolved  
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

- Choice:
- Tail truncation policy:
- Read-only/salvage policy:
- Reasoning:
- Accepted on:

### D026: Value Codec Registry

**Status:** Unresolved  
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

- Choice:
- Built-in codec registry location:
- Codec versioning policy:
- Raw literal fallback policy:
- Custom codec policy:
- Reasoning:
- Accepted on:

### D027: Format Evolution And Migration

**Status:** Unresolved  
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

- Choice:
- Minimum compatibility window:
- Migration backup/rollback policy:
- SQLite importer lifetime:
- Reasoning:
- Accepted on:

---

## Public Beta Boundaries

### D028: Stable Rust API Surface

**Status:** Unresolved  
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

- Choice:
- Stable modules/types:
- Explicitly unstable modules/types:
- Reasoning:
- Accepted on:

### D029: HTTP Beta Trust Boundary

**Status:** Unresolved  
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

- Choice:
- Default bind address:
- Request/runtime/result limits:
- CORS default:
- Reasoning:
- Accepted on:

### D030: Beta Compatibility Promise

**Status:** Unresolved  
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

- Choice:
- Logical data guarantee:
- Language/API deprecation window:
- Versioning scheme:
- Reasoning:
- Accepted on:

---

## Decision Summary

Update this table as decisions are accepted.

| ID | Decision | Status | Choice |
| --- | --- | --- | --- |
| D001 | Role identity and names | Unresolved | |
| D002 | Posit proposition equality | Unresolved | |
| D003 | Thing identity scope and import | Unresolved | |
| D004 | Identity equivalence and merging | Unresolved | |
| D005 | Appearance-set cardinality and slots | Unresolved | |
| D006 | Reserved role vocabulary | Unresolved | |
| D007 | Meaning of imprecise time | Unresolved | |
| D008 | Temporal comparison vocabulary | Unresolved | |
| D009 | Snapshot maxima and conflicts | Unresolved | |
| D010 | Snapshot and value-filter order | Unresolved | |
| D011 | Evaluation of `@NOW` | Unresolved | |
| D012 | Allocation and query variable syntax | Unresolved | |
| D013 | Query variable scope | Unresolved | |
| D014 | Exact and open appearance matching | Unresolved | |
| D015 | Query algebra required for beta | Unresolved | |
| D016 | Negation and open-world semantics | Unresolved | |
| D017 | Literal interpretation and comparison | Unresolved | `=` nominal; `?=` compatible |
| D018 | Result cardinality and ordering | Unresolved | |
| D019 | Role and parameter syntax | Unresolved | |
| D020 | Durable Thing allocation | Unresolved | |
| D021 | Atomicity unit | Unresolved | |
| D022 | Durability and acknowledgment | Unresolved | |
| D023 | Physical file set and consistency | Unresolved | |
| D024 | Record framing and integrity | Unresolved | |
| D025 | Recovery and corruption | Unresolved | |
| D026 | Value codec registry | Unresolved | |
| D027 | Format evolution and migration | Unresolved | |
| D028 | Stable Rust API surface | Unresolved | |
| D029 | HTTP beta trust boundary | Unresolved | |
| D030 | Beta compatibility promise | Unresolved | |
