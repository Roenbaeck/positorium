# Extending Traqula with Classification

## Status and sources

This document is an implementation plan for classification and a research note
for constraints. Classification is sufficiently defined to proceed through
contracts, fixtures, and implementation. Constraints are deliberately **not**
ready for implementation; the constraint sections record the current direction
and the questions that must be answered first.

The design draws on both versions of the transitional representation paper:

- *Transitional Representation: A Formalism for Conflicting and Evolving
  Information*, release
  [`v0.9.0`](https://github.com/Roenbaeck/transitional/releases/tag/v0.9.0),
  commit `12942e2c29fc3e3a79708b691a733f19a6878ef9`; and
- the earlier representation in commit
  `4c5b7e89cda8f5db81494c09c268b04b55754ee3`.

The older paper is preferable where it distinguishes subclassing from class
membership. The newer paper is preferable where classification is represented
as ordinary temporal, disputable posits. The model below combines those
properties without adopting the older paper's exhaustive classification or its
assumption that every Thing has one distinguished class.

There are no Positorium releases or legacy stores to support. This work should
update the unreleased contracts directly. It must not add a SQLite migration,
an old append-log reader, deprecated syntax, or a compatibility mode for
development data.

## Classification model

Classification introduces no new storage primitive. Class declarations,
memberships, and subclass relationships are ordinary posits. Beliefs about
them are ordinary assertion posits and are resolved through the same
information-in-effect semantics as other information.

### Reserved roles

Reserve exactly three additional roles for classification:

- `thing`
- `class`
- `subclass`

Together with the existing `posit` and `ascertains` roles, this produces five
reserved roles in total. The proposed fixed identities are:

| Identity | Role |
| ---: | --- |
| 1 | `posit` |
| 2 | `ascertains` |
| 3 | `thing` |
| 4 | `class` |
| 5 | `subclass` |

Do not reserve `named`, `classification`, `superclass`, `policy`,
`posit class`, `lower bound`, or `upper bound`.

The role `class` consistently marks a participant as a class. In a membership
posit it is the target class; in a subclass posit it is the parent class; and
in the unary form it explicitly declares a class. A separate `superclass` role
would repeat information already conveyed by the `class` role.

Names, labels, descriptions, and external identifiers are application data.
They can use ordinary user-defined roles such as `name`, but none has built-in
classification semantics. Class identity and joins always use the class Thing,
never a possibly non-unique or changing name.

### Class declaration

An active unary classifier declares a class even when it has no members and no
parent class:

```text
[{(C, class)}, "active", t]
```

This form is useful for discovery, documentation, and empty classes. A Thing
appearing in the `class` or `subclass` position of either binary form is also
known structurally to be a class, so a declaration need not precede first use.
The unary declaration does not give the class a globally preferred name.

### Direct class membership

Direct membership is represented by:

```text
[{(x, thing), (C, class)}, lifecycle, t]
```

Initially, `lifecycle` has two recognized exact string values:

- `"active"` states that `x` is a direct member of `C`;
- `"inactive"` states that the direct membership is inactive.

Other values remain valid stored information but make the built-in membership
interpretation indeterminate. A negative or conflicting assertion about a
classifier posit is not silently converted into the opposite lifecycle state.

The Thing `x` may be any Thing, including a role identity, class identity, or
posit identity. In particular:

```text
[{(C, thing), (M, class)}, "active", t]
```

means that class `C` is an instance of metaclass `M`. It does **not** mean that
`C` is a subclass of `M`.

Membership is deliberately non-exhaustive and non-functional. A Thing may
belong to zero, one, or many classes, and absence of a membership posit is not
evidence of non-membership.

### Subclass relationships

Subset semantics is represented separately:

```text
[{(S, subclass), (C, class)}, lifecycle, t]
```

An active posit means that every member of `S` is also an inherited member of
`C` at the selected information cut. An inactive posit removes that direct
subclass edge; it does not retract other paths from `S` to `C`.

This form has several advantages over treating a class as an ordinary member
of another class:

- it states actual subset semantics;
- it distinguishes subclassing from metaclass membership;
- it supports multiple inheritance without assigning one privileged class;
- it makes direct edges separately queryable from inherited membership; and
- it keeps the entire relationship temporal, disputable, and posit-native.

Subclass traversal is transitive and cycle-safe. A cycle denotes mutual subset
reachability and therefore extensionally equivalent membership for the classes
in that cycle; it should also be exposed as a diagnostic because it may be an
unintended taxonomy error. Traversal must never choose a winner by Thing
identity or append order.

### Subjective and temporal interpretation

Classification is evaluated for one positor over one information-in-effect
slice. It must not fuse assertions from multiple positors into a global class
graph unless an explicit query policy requests such fusion.

At a selected assertion-time and appearance-time cut, an interpreted
classification can be:

- direct membership;
- inherited membership, with its subclass path as provenance;
- explicit inactive membership;
- unknown because no applicable classifier is present; or
- indeterminate because effective classifier states, assertions, or temporal
  maxima conflict.

Raw posit search always remains available and returns the actual classifier
and subclass posits. Inheritance is an explicit classification-query option,
not hidden behavior added to ordinary `search`.

## Decisions required before classification implementation

Revise D006 in `DECISIONS.md` and the corresponding normative model and
contract text before changing code. Record at least these decisions:

1. **Reserved vocabulary.** Retain `posit` and `ascertains` at identities `1`
   and `2`; reserve `thing`, `class`, and `subclass` at identities `3` through
   `5`. No constraint roles are reserved.
2. **No legacy path.** Bootstrap fresh stores with the five-role vocabulary and
   reject incompatible development stores. Do not write a migration.
3. **Classifier shapes.** Interpret only the exact unary `{class}`, binary
   `{thing, class}`, and binary `{subclass, class}` role sets as built-in
   classification forms. A posit with additional appearances is ordinary
   application data and is not silently decoded as a classifier.
4. **Lifecycle profile.** Initially recognize exact string values `"active"`
   and `"inactive"`. Preserve all other values but report their built-in
   interpretation as indeterminate.
5. **Open-world membership.** Missing membership is `unknown`, not
   non-membership. Multiple direct classes are valid.
6. **Direct versus inherited membership.** Ordinary patterns and direct queries
   return recorded classifiers. Transitive subclass traversal is explicit,
   cycle-safe, and provenance-preserving.
7. **Per-positor views.** Resolve classification independently for each positor
   unless a separate, explicit source-fusion policy is supplied.
8. **Temporal comparability.** Retain equal or incomparable maxima. If they
   change the interpretation, return `indeterminate` rather than imposing a
   total order.
9. **Identity parameters.** Typed parameters may carry current-store Thing and
   Posit identities. Validate the store UUID at external boundaries. Never
   resolve a class reference from its name.

## Proposed Traqula surface

Raw posit spelling remains normative and sufficient. Convenience syntax should
be small sugar expanded by the parser/AST into existing `add posit` operations,
not a second mutation path.

### Raw representation

```traqula
add role name;

add posit +person_class_decl [
    {(+person_class, class)},
    "active",
    '2024-03-01'
];

add posit +person_name [
    {(person_class, name)},
    "Person",
    '2024-03-01'
];

add posit +membership [
    {(+archie, thing), (person_class, class)},
    "active",
    '1972-08-20'
];

add posit +mormon_is_christian [
    {(+mormon, subclass), (+christian, class)},
    "active",
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

### Classification mutation sugar

Possible convenience forms are:

```traqula
add class +person_class at '2024-03-01';
classify +archie as person_class active at '1972-08-20';
add subclass +mormon of +christian active at '2024-03-01';
```

They expand respectively to:

```traqula
add posit [{(+person_class, class)}, "active", '2024-03-01'];
add posit [{(+archie, thing), (person_class, class)}, "active", '1972-08-20'];
add posit [{(+mormon, subclass), (+christian, class)}, "active", '2024-03-01'];
```

These forms are provisional until the raw representation, typed identities,
and classification queries are working. Naming remains an ordinary posit and
does not need special class syntax.

### Information-in-effect query operator

Classification requires the paper's explicit dual-cut assertion view. A
possible raw query form is:

```traqula
search ?assertion = [
           {(?claim, posit), (?source, ascertains)},
           ?certainty,
           ?asserted
       ] in effect ($assertion_cutoff, $appearance_cutoff),
       ?claim = [{(?subject, thing), (?class, class)}, ?state, ?appeared]
return ?subject, ?class, ?state, ?appeared, ?source, ?certainty, ?asserted;
```

`in effect (T, t)` applies to an assertion pattern and:

1. retains assertions at or before assertion cut `T` whose target posit is at
   or before appearance cut `t`;
2. resolves target states per positor and target appearance set at the
   appearance cut; and
3. resolves assertion versions per positor, target posit, and assertion cut.

Equal or incomparable maxima remain visible. Retraction, malformed assertions,
and dangling target identities receive explicit semantics and diagnostics.
Ordinary one-cut `as of` retains its current meaning.

The Rust resolver must be implemented once and shared by query execution and
classification so their temporal semantics cannot drift.

### Classification queries

Direct classification remains an ordinary pattern. Optional sugar can expose
the interpreted view:

```traqula
search classifications of $archie
       in effect ($assertion_cutoff, $appearance_cutoff)
       include subclasses
return ?class, ?mode, ?path, ?source, ?certainty, ?since;
```

`?mode` distinguishes direct and inherited membership. `?path` carries the
subclass posit identities used for an inherited result. Names are joined only
when explicitly requested and all effective matching names remain visible.

## Classification implementation

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

### 2. Effective classification view

Add `src/classification.rs` over an information-in-effect slice. Build ephemeral
indexes keyed by positor and cut:

```text
(positor, class) -> direct member Things and classifier posit identities
(positor, thing) -> direct class Things and classifier posit identities
(positor, child class) -> direct parent classes and subclass posit identities
```

Keep direct and inherited membership separate. Transitive traversal must be
iterative, cycle-safe, retain every supporting path required for provenance,
and never cross positor boundaries by default.

Class declarations and classifier values that are malformed, conflicting, or
temporally incomparable remain visible and produce typed diagnostics. They are
never collapsed into an arbitrary graph.

### 3. Public and client boundaries

Expose high-level types such as `EffectCut`, `EffectiveAssertion`,
`ClassificationView`, `MembershipMode`, and `ClassificationDiagnostic`. Keep
keepers and posting lists private. Extend execution parameters and result cells
with opaque current-store identity handles.

Once Traqula returns these rows through the existing result contract, HTTP,
SSE, and WASM should inherit the same semantics. Add boundary tests and update
the web console examples and autocomplete in `positorium.html`.

## Constraints: research direction only

Do **not** implement constraint roles, policy decoding, cardinality validation,
constraint mutation syntax, or write-time enforcement as part of the
classification extension.

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
2. resolve their effective classification, including subclass provenance;
3. select the applicable limit or exception;
4. resolve effective marriage posits at the same declared cuts;
5. count distinct current spouses; and
6. emit one finding per violation.

For a violation, the witnesses could include the person's classification
posit, subclass posits used by inheritance, and the effective marriage posits.
For satisfaction, those identities are not enough: the evaluation must also
retain the complete scoped-result digest that establishes no additional
marriage was present.

Whether exceptions are encoded in program logic, supplied as policy posits, or
represented by a reusable declarative layer remains an open design question.
No specificity or override behavior should be frozen until overlapping and
disputed classifications have precise semantics.

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

## Repository change map for classification

Expected files and responsibilities:

- `DECISIONS.md`, `MODEL.md`, `CONTRACTS.md`, and `TODO.md`: accept and schedule
  the classification semantics, while marking constraints as research-only.
- `src/construct.rs`: bootstrap and verify the five fixed roles.
- `src/storage.rs` and `src/maintenance.rs`: update fresh-store bootstrap,
  logical export/import validation, and built-in identity checks; no migration.
- `src/effect.rs`: dual-cut assertion resolution.
- `src/classification.rs`: direct and inherited effective class views.
- `src/traqula.pest`: typed identity positions, `in effect`, and eventual small
  classification convenience commands.
- `src/traqula.rs` and `src/traqula/query.rs`: AST/desugaring, execution, and
  structured results. New query work belongs in `src/traqula/query.rs`; do not
  extend the retained `search_legacy` evaluator.
- `src/error.rs`: stable classification and indeterminate diagnostic codes.
- `src/interface.rs`, `src/server.rs`, and `src/wasm.rs`: identity parameters,
  result fields, limits, and boundary serialization.
- both `traqula.tmLanguage.json` files: syntax highlighting kept in lockstep.
- `TRAQULA.md`, examples, and `positorium.html`: raw and sugar examples,
  dual-cut semantics, and open-world warnings.

No physical posit-record change is required. Classification data fits the
existing appearance-set/value/time representation.

## Delivery sequence

### Phase 0: contracts and golden fixtures

- Record the classification decisions above and revise D006.
- Add raw golden fixtures for class declaration, membership, metaclass
  membership, subclassing, multiple inheritance, and disputed classifiers.
- Specify exact information-in-effect slices and classification query results
  before coding.
- Mark all constraint syntax and implementation work as deferred.

### Phase 1: vocabulary and raw representation

- Bootstrap the five fixed roles in fresh stores.
- Update replay, logical transfer, role-count, and catalog-integrity tests.
- Demonstrate that every classification example can be inserted and retrieved
  using ordinary posits and existing search patterns.

### Phase 2: information in effect

- Implement and test the reusable Rust resolver.
- Add the dual-cut assertion-pattern operator.
- Keep existing `as of` behavior unchanged.

### Phase 3: classification

- Implement effective class declarations, direct membership, and subclass
  edges.
- Add typed identity parameters.
- Implement explicit inherited membership with cycle and provenance reporting.
- Add classification sugar only after raw behavior and query results are stable.

### Phase 4: interfaces, documentation, and performance

- Carry classification results through Rust, HTTP, SSE, and WASM.
- Update the web console and editor grammars.
- Add benchmarks and optimize only from profiles.
- Update crate and contract version declarations immediately before the first
  Positorium release; do not create legacy shims for unreleased behavior.

### Separate future work: constraint semantics

- Develop the constraint-program, snapshot, provenance, finding, and evaluation
  model in a separate design document.
- Validate it against the blocking questions and examples above.
- Do not schedule parser, storage, evaluator, or UI implementation until that
  model is accepted.

## Classification test matrix

At minimum, add tests for:

- unary class declaration and a declared class with no members;
- names as ordinary posits, renaming over time, non-unique names, and conflicting
  names without any effect on class identity;
- active/inactive membership and positive, negative, zero, and competing
  classifier assertions;
- unknown versus explicit inactive versus indeterminate membership;
- multiple direct classes and a Thing with no known class;
- classifying role, class, and posit identities;
- metaclass membership remaining distinct from subclassing;
- active/inactive subclass transitions and alternative inheritance paths;
- deep chains, diamonds, cycles, and multiple inheritance;
- direct versus inherited membership with complete path provenance;
- independent class frameworks from two positors;
- both temporal cuts, reassertion, restatement, retraction, correction, equal
  maxima, incomparable times, and dangling target posits;
- malformed assertion and classifier posits;
- raw posit spelling versus sugar expansion equivalence;
- restart, logical export/import, WASM, HTTP, SSE, cancellation, and limits; and
- invariance under append order, query-plan order, unrelated data, and Thing
  identity ordering.

Add Criterion cases for information-in-effect resolution, direct class lookup,
and subclass traversal at 10,000 and 100,000 posits. Measure assertions scanned,
classifier posits decoded, graph edges traversed, paths retained, and peak result
size. Avoid a persistent materialized class index until measurements show it is
needed.

## Classification definition of done

The classification extension is complete when:

1. declarations, memberships, metaclass memberships, and subclass relationships
   are reproducible as raw Traqula golden fixtures;
2. raw posits and every accepted convenience form produce identical stored
   propositions;
3. direct and inherited membership remain distinguishable with provenance;
4. disagreement remains visible per positor and temporal cut;
5. unknown and indeterminate cases are not silently converted to false;
6. ordinary history and `as of` queries retain their current semantics;
7. Rust, native-store, HTTP/SSE, WASM, editor, and browser tests pass;
8. no constraint implementation has been smuggled into the classification
   layer; and
9. no SQLite, old-store, or unpublished-syntax migration path has been added.

## Deferred work

Constraints, write-time enforcement, closed-world inference, automatic source
fusion, global class-name uniqueness, class equivalence sugar, disjointness,
and automatic schema selection remain deferred. Constraint research must not
reserve vocabulary or impose storage behavior until its semantics are accepted.
