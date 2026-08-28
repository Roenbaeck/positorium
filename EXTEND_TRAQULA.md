# Information in Effect and Neutral Classification Presentation

## Status and sources

This document is an implementation plan for information in effect, five-role
vocabulary bootstrap, neutral classification queries, and a selected-class
Terrain overlay. Constraints are deliberately **not** ready for implementation;
the constraint sections record the current direction and the questions that
must be answered first.

The design draws on the three paper revisions retained in this repository:

- the [oldest paper](theory/oldest_paper.tex), which contains the original
   classification and body-characteristic treatment;
- the [older paper](theory/older_paper.tex), which generalizes bodies of
   information to sets of posits and refines classification and temporal
   resolution; and
- the [recent paper](theory/recent_paper.tex), which gives the current
   assertion-body, information-in-effect, and policy-separation terminology.

The older revisions are informative where they distinguish subclassing from
class membership and represent classification as ordinary temporal, disputable
posits. The model below retains those properties without adopting exhaustive
classification or the assumption that every Thing has one distinguished
class. The recent paper is normative for temporal evidence selection when the
relevant times are comparable and for the boundary between information in
effect and downstream interpretation. Positorium generalizes each grouped
maximum to maximal-element selection under its interval-based partial order,
retaining every incomparable maximum.

For temporal resolution, use the paper's latest separation of concerns. A body
of information is a finite set of posits, an assertion body is a body containing
only assertion posits, and information in effect is a derived subset of an
assertion body at two temporal cuts. It deterministically selects source-local
evidence; it does not fuse sources, rank alternative values by certainty, or
select an accepted truth.

There are no Positorium releases or legacy stores to support. This work should
update the unreleased contracts directly. It must not add a SQLite migration,
an old append-log reader, deprecated syntax, or a compatibility mode for
development data.

## Neutral classification vocabulary

Classification introduces no storage primitive and no database truth semantics.
Class declarations, direct classification statements, and subclass statements
are ordinary posits. Assertions about them are ordinary assertion posits and can
be selected with the same information-in-effect operator as other evidence.

### Reserved roles

Reserve exactly three additional Roles for stable interchange and raw queries:

- `thing`
- `class`
- `subclass`

Together with `posit` and `ascertains`, this produces five reserved Roles:

| Identity | Role |
| ---: | --- |
| 1 | `posit` |
| 2 | `ascertains` |
| 3 | `thing` |
| 4 | `class` |
| 5 | `subclass` |

Do not reserve `named`, `classification`, `superclass`, `policy`,
`posit class`, `lower bound`, or `upper bound`. Names, labels, descriptions, and
external identifiers use ordinary application Roles. Class identity and joins
use the class Thing, never a possibly changing or non-unique display name.

Reservation guarantees only catalog identity and availability. The database
does not decode classifier shapes, interpret their values, infer membership, or
traverse subclasses.

### Structural conventions

Consumers may use three compact structural conventions:

```text
[{(C, class)}, state, t]
[{(x, thing), (C, class)}, state, t]
[{(S, subclass), (C, class)}, state, t]
```

They respectively provide evidence that a Thing is being presented as a class,
that `x` is being classified under `C`, and that `S` is being related to `C` as
a subclass. The separate `subclass` form avoids conflating an instance-of
statement about a class Thing with a subclass statement.

The value `state` is opaque. `"active"`, `"inactive"`, `"included"`, booleans,
and domain-specific values are all ordinary literals. The core does not define
which means membership, exclusion, declaration, retirement, or anything else.
Negative certainty is likewise retained as source-local opposition and is not
converted into an opposite lifecycle value.

A consumer or presentation policy may choose:

- which structural forms it recognizes;
- which values it displays as included;
- which positors and certainty signs it accepts;
- which assertion-time and appearance-time cuts it uses;
- whether several sources are shown separately or fused; and
- whether and how subclass statements are traversed.

Those choices must remain visible to the user. Absence of a matching posit is
absence of evidence under the selected query, not database-defined
non-membership. Ordinary search always returns the recorded structures and
values without applying a classification policy.

## Accepted implementation boundary

The classification extension follows these decisions:

1. **Fixed vocabulary.** Bootstrap the five D006 Roles above. No constraint
   Roles are reserved.
2. **No legacy path.** Update fresh unreleased stores and reject incompatible
   development stores. Do not write a migration.
3. **Value neutrality.** No Rust, Traqula, HTTP, WASM, or Terrain core type
   recognizes built-in classification lifecycle values.
4. **Structural access.** Unary `{class}`, binary `{thing, class}`, and binary
   `{subclass, class}` are ordinary queryable patterns, not write invariants.
   Posits with extra appearances remain equally valid data.
5. **No implicit closure.** Ordinary search and `in effect` do not traverse
   subclass statements or fuse evidence sources.
6. **Presentation policy.** The first interpreted class view belongs to Terrain
   and displays one selected direct class under an explicit client policy.
7. **Identity references.** Future typed parameters may carry current-store
   Thing and Posit identities. External references validate the store UUID.
   Display names never substitute for class identity.

## Proposed Traqula surface

Raw posit spelling remains normative and sufficient. Any future convenience
syntax must be small sugar expanded by the parser/AST into existing `add posit`
operations, not a second mutation path or a hidden value policy.

### Raw representation

```traqula
add role name;

add posit +person_class_decl [
    {(+person_class, class)},
    "declared",
    '2024-03-01'
];

add posit +person_name [
    {(person_class, name)},
    "Person",
    '2024-03-01'
];

add posit +membership [
    {(+archie, thing), (person_class, class)},
    "included",
    '1972-08-20'
];

add posit +mormon_is_christian [
    {(+mormon, subclass), (+christian, class)},
    "included",
    '2024-03-01'
];

add posit [
    {(person_class_decl, posit), (+model, ascertains)},
    100%,
    '2024-03-01'
], [
    {(membership, posit), (model, ascertains)},
    100%,
    '2024-03-01'
], [
    {(mormon_is_christian, posit), (model, ascertains)},
    100%,
    '2024-03-01'
];
```

The classification roles are bootstrapped, so a script does not add them.
User-defined descriptive roles such as `name` remain ordinary catalog entries.
The example values are merely one application's vocabulary.

### No classification mutation sugar initially

Do not add `add class`, `classify`, or `add subclass` commands in the first
implementation. Such syntax would either have to invent a lifecycle value or
hide a policy choice. Raw `add posit` is already concise and makes the stored
value explicit. A later client macro may require the caller to provide the value
and still expand mechanically to `add posit`.

### Information-in-effect query operator

Assertion-backed classification display uses the paper's explicit dual-cut
assertion view. A target-only query should make the common case concise:

```traqula
search
   [{(*, name)}, ?namevalue, ?appeared]
      in effect @NOW, @NOW
return
   ?namevalue, ?appeared;
```

`in effect T, t` always modifies a **target posit pattern**. It never
inspects the pattern's roles to guess whether that pattern is an assertion
envelope. This also keeps assertions about assertions unambiguous: an attached
pattern with the roles `posit` and `ascertains` still denotes a target posit
being ascertained by another effective assertion.

The clause has exactly two operands separated by a comma. The first operand is always the
assertion-time cut `T`; the second is always the target appearance-time cut
`t`. This intentionally parallels `as of t`, which has exactly one operand.
For readability, put a following search pattern on a new line when its pattern
separator would immediately follow the second cut.

Semantically, the engine first computes the information-in-effect slice over
the complete assertion body at `(T, t)`, then dereferences each retained
assertion's target posit, and only then matches the attached target pattern.
Value and target-time restrictions in that pattern must not run before
resolution, because doing so could resurrect an older matching value after a
newer non-matching value has taken effect. Structural target-role restrictions
may be pushed down only as a proven-equivalent optimization.

The modifier:

1. retains assertions at or before assertion cut `T` whose target posit is at
   or before appearance cut `t`;
2. discards zero-certainty assertions and any non-zero assertion retracted by
   the same positor for the same target posit after that assertion and no later
   than `T`;
3. retains assertions whose target appearance time is maximal for each
   `(positor, target appearance set)`; and
4. from those rows, retains assertions whose assertion time is maximal for each
   `(positor, target appearance set, target value)`.

Equal or incomparable maxima remain visible at both reductions. In particular,
alternative values at the same maximal appearance time all remain in effect;
their certainties do not participate in either maximum. Retraction, malformed
assertions, and dangling target identities receive explicit semantics and
diagnostics. Ordinary one-cut `as of` retains its current meaning.

The result is an **effective assertion slice**. Diagnostics such as decisive,
indecisive, or non-contradictory inspect that slice. Fusion and acceptance are
separate operators over it. For example, choosing the highest-certainty value
would be one optional acceptance policy, not part of `in effect`.

The concise form hides, but does not discard, the effective assertion's
identity, positor, certainty, and assertion time. Query cardinality remains one
binding per matching effective assertion. If two positors effectively assert
the same target posit, projecting only target fields therefore returns two
identical rows under Traqula's normal bag semantics; `return distinct` requests
deduplication explicitly.

Queries that need provenance or same-positor correlation can expose the
assertion envelope with a `via` pattern:

```traqula
search
   ?claim = [{(?subject, thing), (?class, class)}, ?state, ?appeared]
      in effect $assertion_cutoff, $appearance_cutoff
      via ?assertion = [
         {(?claim, posit), (?source, ascertains)},
         ?certainty,
         ?asserted
      ]
return
   ?subject, ?class, ?state, ?appeared,
   ?assertion, ?source, ?certainty, ?asserted;
```

`via` is optional surface syntax for bindings already present in the effective
assertion relation; it does not perform a second search or change resolution.
Each `in effect` occurrence without `via` receives fresh hidden evidence
variables. Consequently, multiple shorthand patterns may be supported by
different positors. A query requiring the same positor across patterns must
expose and reuse `?source` through `via`, rather than relying on implicit source
fusion.

### Desugaring target

This separation makes a lower-level expansion precise. Let `A1` be the joined
assertion and target rows passing both cuts. Then the effective assertion
relation is equivalent to four ordinary relational stages:

```text
A1 = temporally eligible assertion rows
A2 = non-zero A1 rows anti-join later retractions of the same
     (positor, target posit)
A3 = A2 with strictly dominated target appearance times removed
     per (positor, target appearance set)
IE = A3 with strictly dominated assertion times removed
     per (positor, target appearance set, target value)
```

Both dominance stages retain equal and incomparable maxima. This is a semantic
desugaring, not permission to implement certainty-based tie-breaking.

The ergonomic target modifier then lowers to a join from `IE` to its target
posit followed by the attached target-pattern match. An explicit `via` pattern
binds columns from `IE`; without it, those columns remain fresh hidden
variables. This lowering fixes target matching after resolution and preserves
source-local bag multiplicity without giving `in effect` a second meaning.

Current Traqula does not yet have enough composable query algebra to spell this
expansion faithfully. Its correlated `not exists` block has no local `where`
predicate or nested/named intermediate relation, while pattern-local `as of`
cannot perform grouped maxima over the joined assertion/target rows. Extending
anti-join blocks with local predicates is useful, but retraction filtering must
also be reusable as the input to both grouped reductions. The implementation
should therefore first define a general lower-level representation for:

- a named or nested relation produced by joins, filters, and anti-joins; and
- nondominated reduction partitioned by explicit grouping variables and
  ordered by a selected time variable.

`in effect` can then lower to the four-stage plan above, and the executor may
optimize that recognized plan with a shared Rust resolver. Until those general
operations exist, implementing only an opaque `in effect` AST node is
acceptable as an executable reference, but it is not yet a true desugaring to
the current language. The expansion must be tested for invariance under query
planning, pattern order, append order, unrelated data, and identity allocation.

The Rust resolver must be implemented once and shared by query execution and
all assertion-aware consumers so their temporal semantics cannot drift.

### Neutral classification queries

Direct classification evidence remains an ordinary target pattern:

```traqula
search
   ?classification = [
      {(?member, thing), (?class, class)},
      ?state,
      ?appeared
   ] in effect $assertion_cutoff, $appearance_cutoff
      via ?assertion = [
         {(?classification, posit), (?source, ascertains)},
         ?certainty,
         ?asserted
      ]
return
   ?classification, ?member, ?class, ?state, ?appeared,
   ?source, ?certainty, ?asserted;
```

This query returns evidence, not a membership verdict. The client selects one
class identity and applies its visible display policy. Subclass evidence uses an
ordinary `{(?child, subclass), (?parent, class)}` target pattern. No implicit
closure or name resolution is added to Traqula.

## Implementation

### 1. Information in effect

Add a reusable semantic module, for example `src/effect.rs`, with opaque public
result types and an internal resolver. It should:

- find exact two-role assertion posits using the `posit` and `ascertains`
  indexes;
- require a certainty literal in `[-100%, 100%]`;
- resolve each referenced target posit;
- apply both temporal cuts and retractions;
- group with structural keys, never formatted text or append order;
- preserve equal and incomparable maxima; and
- check cancellation, timeouts, and scan limits in every loop.

Cache a resolved slice for identical dual cuts during one command. Add counters
before considering durable materialization or persisted indexes.

### 2. Vocabulary bootstrap and neutral access

Bootstrap and verify the five fixed Roles. Do not add `src/classification.rs`, a
classification decoder, lifecycle diagnostics, materialized member indexes, or
subclass inference. Existing exact/open appearance matching and the shared
information-in-effect resolver provide the required data access.

Expose high-level assertion types such as `EffectCut` and
`EffectiveAssertion`; do not expose `ClassificationView`, `MembershipMode`, or
other types that imply a database verdict. Typed Thing/Posit parameters remain
useful for selecting identities but do not change this boundary.

### 3. Terrain selected-class overlay

Terrain 1 remains an authoritative, value-independent structural report. The
browser combines its geometry with a separate neutral classification query.
The first overlay:

- allows exactly one selected class;
- begins with direct `{thing, class}` evidence only;
- shows the selected lifecycle value, positor/source treatment, certainty rule,
  and temporal cuts as presentation settings;
- may default to exact value `"active"`, but only in the UI;
- uses an ordinary descriptive posit for the selector label when available and
  otherwise shows the class Thing identity;
- draws translucent shading behind isopleth strokes, connections, and labels;
- reuses an isopleth interior when it already represents the classified Thing;
- creates padded regions around other visible member geometry;
- merges overlapping regions while retaining disconnected islands; and
- leaves Terrain report counts, identities, layout, and version unchanged.

Subclass expansion is deferred. When added, it remains an explicit client
option parameterized by the same value, evidence, and temporal policy.

HTTP, SSE, and WASM continue returning neutral rows. No transport should add a
classification verdict that Traqula did not return.

## Constraints: research direction only

Do **not** implement constraint roles, policy decoding, cardinality validation,
constraint mutation syntax, or write-time enforcement as part of the
classification extension.

### Body characteristics, diagnostics, and enforcement

Decisiveness, indecisiveness, and non-contradiction describe retained evidence;
they do not alter what `in effect` returns. Their natural default is a query or
audit diagnostic over an explicit assertion body, positor scope, and temporal
cut. Useful results include the characteristic verdict, the affected
`(positor, appearance set)` groups, the effective assertion witnesses, and
summary counts. Calling them statistics is reasonable for the aggregates, but
the underlying result is a formally defined property or diagnostic.

An entire body being decisive or non-contradictory is much stronger than one
selected slice having that property: the paper's body-level definitions
quantify over every information-in-effect slice. Because logical storage is
append-only, a later correction cannot make a historically indecisive slice
disappear. Backdated assertions can also change diagnostics at earlier
appearance-time cuts. Whole-body enforcement is therefore costly and, once
violated, cannot be repaired merely by appending another assertion.

Use three distinct levels:

1. **Representation invariants** are always enforced. Examples include valid
   identities, exact built-in assertion shape where built-in interpretation is
   requested, bounded decoding, and certainty within its admitted range.
2. **Evidence diagnostics** are computed from actual data by default. These
   include decisiveness and the non-contradiction budget for each positor and
   appearance set in an effective slice.
3. **Declared integrity profiles** may later require selected diagnostics for
   a named scope and explicit cuts, with modes such as report, validate, or
   reject. Such a profile is a versioned constraint policy; it never authorizes
   `in effect` to discard alternatives or manufacture a winner.

Source-local exclusivity for the exact same target posit, source, and assertion
time is local enough to be offered as an optional write-time integrity rule.
Decisiveness is usually a poor global default because it deliberately forbids
the competing alternatives transitional representation is designed to retain.
Non-contradiction is often useful as a validation rule, but rejecting violations
should be an explicit database or input-scope profile rather than universal
storage semantics. Applications needing relational-style single-valued state
can opt into a decisive profile; evidence-oriented databases should normally
retain the rows and report the diagnostic.

Do not expose one undifferentiated `comprehensive = true` database switch. The
other paper characteristics require different treatment:

- canonical form can be a normalization rule only for value domains with
   declared complement semantics;
- symmetry and boundedness can be checked only when those complements are
   known, so they belong to a domain profile rather than every `DataType`;
- source-local exclusivity is directly observable and locally enforceable;
- universality is chiefly an identity-alignment and shared-meaning assumption,
   not a property that can be inferred from equal stored identifiers alone; and
- decisiveness and non-contradiction are temporal properties of effective
   slices, with whole-body forms quantified over all cuts.

A future audit may report a named collection of these diagnostics as a
comprehensive profile, but it should retain each verdict, scope, assumptions,
and witnesses separately.

The earlier plan gave cardinality policies too much responsibility. A general
constraint can depend on arbitrary relationships, temporal classifications,
aggregates, absence, external parameters, and competing evidence. Reifying
every possible selector as a fixed family of roles would be cumbersome without
providing general expressiveness.

The more promising model is a versioned constraint program evaluated over an
explicit, immutable set or view of posits:

```text
ConstraintProgram(information snapshot, parameters)
    -> verdict, findings, evidence
```

Cardinality, uniqueness, inclusion, and similar declarations may later be
convenient syntax or library programs compiled to this interface. They are not
the definition of a constraint.

### Constraint definition

A future constraint should be an ordinary posit-backed invocation of:

- an immutable program identity and digest;
- an explicit input-scope definition;
- posit-native parameters and references; and
- a particular program/runtime contract version.

Illustrative roles in the following sketch are **not reserved or accepted
syntax**:

```text
[{(C, constraint), (P, constraint program), (Q, input scope)}, "active", t]
```

Identifiers used by a program should normally arrive as parameters referenced
through appearances, rather than being embedded in source or opaque JSON where
logical import cannot remap them.

The program must be pure and reproducible: no undeclared clock, randomness,
network, filesystem, process state, or mutable global state. Execution must use
canonical input ordering and serialization, a fixed runtime version, and
explicit fuel, memory, cancellation, and output limits.

### Information snapshot and provenance

The input must be frozen at explicit assertion-time and appearance-time cuts.
The evaluation output is appended after the snapshot closes and cannot become
an input to its own execution. Constraints over evaluations require an explicit
later stratum or cut rather than an implicit recursive fixed point.

Remembering only the posit identities inspected by the program is insufficient.
Violations often have positive witnesses, but satisfaction and lower-bound
violations depend on absence. For example, one marriage posit does not prove
monogamy unless the evaluator can also demonstrate that its complete relevant
scope contained no second effective marriage.

A reproducible evaluation therefore needs:

- the exact constraint-definition posit and program version;
- the assertion-time and appearance-time cuts;
- the input-scope/query identity and a digest of its complete result;
- the exact resolved posit identities read by the program;
- a smaller evidence or witness set explaining each finding;
- the evaluator runtime identity and version; and
- the verdict, findings, diagnostics, and resource outcome.

The complete scope digest makes negative evidence and later invalidation
meaningful. The smaller witness set makes results understandable. A safe
runtime can record reads automatically; the program may additionally identify
minimal witnesses but must not be trusted to provide the only dependency
record.

### Evaluation result

The semantic verdict should have at least three states:

- `satisfied`;
- `violated`; and
- `indeterminate`.

Program failure, timeout, cancellation, invalid input, and resource exhaustion
are execution outcomes, not evidence that a constraint is indeterminate or
violated.

One execution may produce multiple findings. Each finding should identify its
subject or context, expected condition, observed condition, explanation, and
witness posit identities. An evaluation summary and its findings can themselves
be stored as ordinary posits and ascertained by an evaluator positor.

An old evaluation remains immutable audit history when data, code, parameters,
or a temporal cut changes. A new input snapshot produces a new evaluation; it
does not rewrite the old result.

### Example requiring conditional policy

Consider the hypothetical rule that Christians are monogamous while Mormons
may be polygamous. A constraint program could:

1. enumerate people from an explicit subject scope;
2. apply its declared value, source, certainty, temporal, and optional subclass
   policy to effective classification evidence;
3. select the applicable limit or exception;
4. resolve effective marriage posits at the same declared cuts;
5. count distinct current spouses; and
6. emit one finding per violation.

For a violation, the witnesses could include the person's classification
posit, subclass posits used by inheritance, and the effective marriage posits.
For satisfaction, those identities are not enough: the evaluation must also
retain the complete scoped-result digest that establishes no additional
marriage was present.

Whether classification policy and exceptions are encoded in program logic,
supplied as policy posits, or represented by a reusable declarative layer
remains an open design question. No specificity, fusion, or override behavior
should be frozen as database classification semantics.

### Questions that block constraint implementation

Write a separate constraint semantics specification before adding code. It must
answer at least:

1. What identifies a constraint: a program, an invocation, a policy Thing, or
   a specific definition posit?
2. How is the complete input universe selected, especially for lower bounds and
   other claims about missing information?
3. Does code receive a materialized set, a read-tracked query API over a frozen
   snapshot, or both?
4. Which language and runtime provide determinism, sandboxing, portability,
   versioning, and useful diagnostics?
5. How are posit identities passed without preventing logical import and
   remapping?
6. How do open-world absence, explicit inactivity, negative certainty,
   disagreement, and incomparable times affect a verdict?
7. At which valid-time cut is a classification used when evaluating a
   historically appearing relationship?
8. How are overlapping policies, exceptions, and incomparable scopes composed?
9. What provenance is sufficient for reproduction, explanation, incremental
   invalidation, and audit?
10. Can constraints evaluate other evaluation posits, and if so, how are strata
    and recursion controlled?
11. Which common declarative forms can be statically analyzed and optimized
    while remaining equivalent to the general program interface?
12. Who ascertains an evaluation, and how can independent evaluators disagree
    without their results being confused with source information?

The semantics should be tested on uniqueness, empty-population lower bounds,
conditional cardinality, overlapping exceptions, disputed membership,
classification changes over time, incomparable temporal maxima, execution
failure, backdated information, and constraints over evaluation results.

## Repository change map

Expected files and responsibilities:

- `DECISIONS.md`, `MODEL.md`, `CONTRACTS.md`, and `TODO.md`: record the fixed
  vocabulary and neutral interpretation boundary while keeping constraints
  research-only.
- `src/construct.rs`: bootstrap and verify the five fixed roles.
- `src/storage.rs` and `src/maintenance.rs`: update fresh-store bootstrap,
  logical export/import validation, and built-in identity checks; no migration.
- `src/effect.rs`: dual-cut assertion resolution.
- `src/traqula.pest`: `in effect`, `via`, and eventual typed identity positions;
  no classification commands.
- `src/traqula.rs` and `src/traqula/query.rs`: AST/desugaring, execution, and
  structured results. New query work belongs in `src/traqula/query.rs`; do not
  extend the retained `search_legacy` evaluator.
- `src/error.rs`: stable assertion-resolution and resource diagnostics.
- `src/interface.rs`, `src/server.rs`, and `src/wasm.rs`: identity parameters,
  result fields, limits, and boundary serialization.
- both `traqula.tmLanguage.json` files: syntax highlighting kept in lockstep.
- `positorium-terrain.js`, `positorium-terrain.css`, `positorium.html`, and
  `tests/terrain_client.test.js`: selected-class controls and shaded overlay.
- `TRAQULA.md`, `TERRAIN.md`, examples, and the cookbook: neutral raw patterns,
  dual-cut semantics, and visible presentation-policy warnings.

No physical posit-record or Terrain report change is required. Classification
data fits the existing appearance-set/value/time representation, and its
shading is browser geometry.

## Delivery sequence

### Phase 0: contracts and golden fixtures

- Record the neutral classification boundary and revise D006.
- Add raw fixtures for unary class, `{thing, class}`, and `{subclass, class}`
  structures using several unrelated literal values.
- Specify exact information-in-effect slices before coding.
- Mark all constraint syntax and implementation work as deferred.

### Phase 1: vocabulary and raw representation

- Bootstrap the five fixed roles in fresh stores.
- Update replay, logical transfer, role-count, and catalog-integrity tests.
- Demonstrate that every structural convention can be inserted and retrieved
  without value interpretation.

### Phase 2: information in effect

- Implement and test the reusable Rust resolver.
- Add the dual-cut assertion-pattern operator.
- Keep existing `as of` behavior unchanged.

### Phase 3: selected-class presentation

- Add a one-class selector and explicit display-policy controls to Terrain.
- Fetch direct classification evidence through neutral Traqula results.
- Render shaded regions without changing structural geometry or report counts.
- Do not implement subclass closure or classification mutation sugar.

### Phase 4: interfaces, documentation, and performance

- Carry `in effect` results and provenance through Rust, HTTP, SSE, and WASM.
- Update the web console and editor grammars for `in effect` and `via`.
- Add benchmarks and optimize only from profiles.
- Update crate and contract version declarations immediately before the first
  Positorium release; do not create legacy shims for unreleased behavior.

### Separate future work: constraint semantics

- Develop the constraint-program, snapshot, provenance, finding, and evaluation
  model in a separate design document.
- Validate it against the blocking questions and examples above.
- Do not schedule parser, storage, evaluator, or UI implementation until that
  model is accepted.

## Test matrix

At minimum, add tests for:

- fixed identities and names for all five reserved Roles;
- unary class statements and a class with no direct classification statements;
- names as ordinary posits, renaming over time, non-unique names, and conflicting
  names without any effect on class identity;
- arbitrary string, boolean, numeric, and JSON values on all three structural
  conventions, returned without classification decoding;
- the literals `"active"` and `"inactive"` receiving no special core behavior;
- multiple `{thing, class}` statements and absence of implicit exclusivity;
- role, class, and posit identities appearing in the `thing` position;
- `{thing, class}` remaining structurally distinct from `{subclass, class}`;
- no implicit subclass traversal, source fusion, or display-name lookup;
- both temporal cuts, reassertion, restatement, retraction, correction, equal
  maxima, incomparable times, and dangling target posits;
- malformed assertion posits while arbitrary non-assertion shapes remain data;
- same-time alternative values remaining in effect regardless of their
   relative certainties, followed by separately tested diagnostic and
   acceptance policies;
- target value and time restrictions being applied after effect resolution so
   an older matching target cannot be resurrected;
- two positors asserting the same target producing two shorthand rows, with
   `return distinct` producing one projected row;
- `via` bindings exposing the same evidence as the shorthand hides, including
   same-positor correlation across multiple effective target patterns;
- semantic equivalence between the reference information-in-effect resolver
   and its eventual lower-level four-stage expansion;
- one selected Terrain class at a time and a visible display policy;
- exact reuse of an isopleth interior, padded non-isopleth regions, merged
  overlaps, preserved disconnected islands, and legible foreground marks;
- unchanged Terrain report identity, counts, and layout with the overlay toggled;
- restart, logical export/import, WASM, HTTP, SSE, cancellation, and limits; and
- invariance under append order, query-plan order, unrelated data, and Thing
  identity ordering.

Add Criterion cases for information-in-effect resolution at 10,000 and 100,000
posits. Measure assertions scanned, retractions examined, temporal comparisons,
effective rows, and peak result size. Use browser performance tests for overlay
geometry. Do not add a materialized class index.

## Definition of done

This extension is complete when:

1. the five reserved Roles have fixed verified identities in fresh stores;
2. classification structures and values round-trip without interpretation;
3. `in effect` returns deterministic source-local evidence with provenance;
4. ordinary history and `as of` queries retain their current semantics;
5. Terrain shades one selected direct class under a visible client policy while
   its structural report and layout remain unchanged;
6. no lifecycle truth table, implicit subclass closure, or source fusion exists
   in the core;
7. Rust, native-store, HTTP/SSE, WASM, editor, and browser tests pass;
8. no constraint implementation has been smuggled into the classification
   presentation; and
9. no SQLite, old-store, or unpublished-syntax migration path has been added.

## Deferred work

Constraints, write-time enforcement, classification verdicts, subclass closure,
closed-world inference, automatic source fusion, global class-name uniqueness,
class equivalence sugar, disjointness, and automatic schema selection remain
deferred. Constraint research must not reserve vocabulary or impose storage
behavior until its semantics are accepted.
