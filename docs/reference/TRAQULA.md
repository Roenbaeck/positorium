<img src="./Traqula.svg" alt="Traqula Language Reference" width="200">
<p/>

# Traqula language version 1

Traqula is Positorium's declarative language for recording and querying
transitional propositions. Version 1 is the first published grammar.

## The data model

A posit is a proposition with three parts:

```text
[AppearanceSet, LiteralValue, Time]
```

- A **Thing** is an opaque identity.
- A **Role** is a named identity such as `name`, `wife`, or `` `postal code` ``.
- An **Appearance** is a `(Thing, Role)` pair.
- An **AppearanceSet** is an unordered, duplicate-free set of appearances.
- A **Posit** is an identified proposition consisting of an appearance set, a
  literal value, and a time.

Posits are appended. A later posit does not overwrite an earlier one.

## Commands at a glance

```traqula
add role name, age, `postal code`;

add posit [{(+person, name)}, "Alice", '2024-01-01'],
          [{(person, age)}, 30, '2024-01-01'];

search [{(?person, name), ...}, ?name, ?time]
where ?time <= @NOW
return ?person, ?name, ?time
order by ?time desc
limit 20;
```

Commands end with `;`. Block comments use `/* ... */`.

## Adding roles and posits

Declare roles before using them:

```traqula
add role name, address, `birth date`;
```

Bare roles are Unicode identifiers. Quote a role with backticks when it contains
spaces, punctuation, a reserved word, or a literal `::` sequence.

Mutation variables are scoped to the script being executed:

- `+person` creates and binds a new Thing.
- `person` recalls that binding in a later posit.
- An optional leading `+p` binds the identity assigned to a newly added posit.

```traqula
add role name;
add posit +p [{(+person, name)}, "Alice", @NOW];
add posit [{(p, posit), (person, ascertains)}, 100%, @NOW];
```

Each `add role` or `add posit` command commits atomically. Earlier successful
commands remain committed if a later command fails.

### Adding and asserting atomically

`and assert` creates the assertion envelope together with its target posit:

```traqula
add posit +claim [
  {(+case, status)}, "open", '2024-03-01'
] and assert +evidence by +registry with 75% at @NOW;
```

This is syntax sugar for an ordinary target posit and an ordinary assertion
posit with the exact `{posit, ascertains}` appearance set. `claim` and
`evidence` bind their canonical posit identities; `registry` binds the source
Thing. The source may instead be recalled with `by registry`. Source,
certainty, and assertion time are mandatory and remain independent of the
target's value and appearance time.

The target and every generated assertion envelope are one command and one
durable batch. If the target proposition already exists, its canonical posit
identity is reused. If the source binding contains several Things, one
assertion posit is generated for every target/source combination.

### Searching or adding

`search ... or add posit ...` resolves durable identities without giving
variable names persistent meaning:

```traqula
search
  [{(?registry, registry_code), ...}, "SE-CIVIL-REGISTRY", *]
or add posit
  [{(+registry, registry_code)}, "SE-CIVIL-REGISTRY", @NOW];

add posit [{(+case, status)}, "open", @NOW]
  and assert by registry with 100% at @NOW;
```

The complete search is evaluated first. With zero matching rows, the add branch
runs once. With one or many rows, it does not run. Identity query binders and
fallback allocation binders having the same name and domain are unified into a
script mutation binding: here `?registry` or `+registry` both establish
`registry` for subsequent additions. Multiple matches remain a set and later
posit construction fans out over that set. Cardinality aggregation and
conditions on aggregates are deliberately deferred.

The search, zero-match decision, and optional fallback mutation execute under
one script owner, so concurrent scripts cannot both observe absence and create
the fallback. An `and assert` suffix on the fallback belongs only to that add
branch and shares its atomic durable batch.

`return` is optional. Omitting it emits no result set; it does not prevent the
compound command from binding identities. When present, every returned
identity must also have an allocation binder in the fallback so either branch
can supply the same result shape. Ordinary searches still keep `?` variables
lexical: only the explicit `search ... or add posit ...` join promotes matching
Thing and Posit binders into mutation scope.

Both branch domains and the complete return shape are validated before the
search reads data or calls a result sink. Invalid compounds therefore emit no
metadata or rows and execute no fallback mutation, whether the current search
would have zero or many matches. `distinct`, `order by`, and `limit` apply to
rows from either branch. A sink that stops at metadata suppresses fallback row
presentation but does not cancel an otherwise valid atomic fallback mutation.

## Search variables and domains

Search variables always start with `?` and are lexical to one `search` command.
They do not carry into a later search. A variable acquires a domain from its
position:

- Thing: `(?person, name)`
- Role: `(?person, ?role)`
- Posit: `?p = [...]`
- AppearanceSet: `?set = {...}`
- LiteralValue: the second posit slot
- Time: the third posit slot or `as of ?cutoff`

Reusing a variable performs an equality join. Reusing it in incompatible
domains is a typed query error.

```traqula
search [{(?person, name), ...}, ?name, *],
       [{(?person, age), ...}, ?age, *]
return ?person, ?name, ?age;
```

`*` matches without binding. `?left|?right` in a Thing slot matches either
already-bound identity.

## Exact and open appearance-set patterns

Appearance-set matching is exact by default:

```traqula
search [{(?wife, wife), (?husband, husband)}, "married", *]
return ?wife, ?husband;
```

That pattern matches an appearance set containing exactly those two members.
Add a trailing `...` to request subset matching:

```traqula
search [{(?person, name), ...}, ?name, *]
return ?person, ?name;
```

Use `*` for any complete appearance set. Bind whole structures explicitly when
needed:

```traqula
search ?p = [?set = {(?thing, ?role), ...}, ?value, ?time]
return ?p, ?set, ?thing, ?role, ?value, ?time;
```

## Literal values and comparisons

Traqula preserves the complete accepted token for every literal:

- integer and decimal: `6`, `+006.00`
- string: `"text"`
- JSON object: `{"key": [true, null]}`
- certainty: `75%`
- time-valued literal: `'2024-05'`

The comparison relations are deliberately different:

```traqula
where ?value === +006.00  /* exact token identity */
where ?value = 6          /* nominal semantic equality */
where ?value ?= 6         /* declared possible-value sets intersect */
```

Integer and decimal comparisons use exact arbitrary-precision arithmetic.
Certainty compares only with certainty and requires `%`. String equality
decodes escapes but does not normalize text; string ordering is not supported.
JSON nominal equality is structural, while `===` preserves presentation such as
key order, spacing, and numeric spelling.

Ordering operators are `<`, `<=`, `>`, and `>=`.

## Time and snapshots

Accepted time precision includes year, month, day, minute, second, and
subsecond forms. A coarse time denotes a half-open interval, so mixed-precision
times can overlap without being ordered. `@BOT`, `@NOW`, and `@EOT` are built-in
constants. One `@NOW` value is resolved for the complete script.

`as of` first matches the pattern structurally, then keeps every maximal
matching posit at or before the cutoff for each appearance set. Equal-time and
incomparable maxima are preserved:

```traqula
search [{(?person, name), ...}, ?name, ?time] as of '2024-06'
return ?person, ?name, ?time;
```

Use `latest` when value matching must happen before reduction:

```traqula
search latest [{(?case, status), ...}, "wanted", ?time] as of @NOW
return ?case, ?time;
```

An `as of ?cutoff` variable may be bound by another pattern in the same search;
the planner resolves the dependency regardless of source order.

An external binding of a pattern's Posit, LiteralValue, or Time variable joins
the temporally reduced result. It must not hide a newer state before reduction.
For `latest`, literal restrictions written inside that pattern are applied
before reduction; external field-variable bindings still join its result.

### History, snapshots, and assertions are separate

A search without `as of` reads recorded history. Ordinary `as of` reduces that
history to the maximal posit proposition or propositions for each complete
appearance set. It does not inspect who asserted a posit, combine certainty,
or choose a preferred source.

Ask which layer the query is intended to read:

- `as of t` asks which recorded posit states are latest at one appearance-time
  cut. It reads posit history directly, independently of whether any source
  currently asserts, previously asserted, or retracted those target posits.
- `in effect T, t` asks which source assertions are effective at an
  assertion-time cut `T` about target states at an appearance-time cut `t`. It
  reads assertion-backed evidence, preserves source-local alternatives, and
  applies retractions. This dual-cut operator is part of the unreleased
  Traqula 1 grammar.

The target pattern can look identical because the operators answer different
questions:

```traqula
/* Raw state: which name posits are latest by appearance time? */
search
  [{(?person, name)}, ?name, ?appeared] as of @NOW
return
  ?person, ?name, ?appeared;

/* Effective evidence: which names do sources currently assert? */
search
  [{(?person, name)}, ?name, ?appeared] in effect @NOW, @NOW
return
  ?person, ?name, ?appeared;
```

The first query can return a latest name posit even if nobody asserts it. The
second returns one binding per matching effective assertion, so two sources
asserting the same name produce two rows unless `return distinct` is requested.
The first `in effect` operand is assertion time and the second is target
appearance time; the fixed arity makes the comma syntax unambiguous. A later
zero-certainty assertion by the same source retracts that exact target posit.
Resolution then retains every equal or incomparable maximum at both temporal
stages. Target value and time predicates run after this reduction, so they
cannot resurrect older matching evidence.

Use `via` when provenance must be visible or correlated with another pattern:

```traqula
search ?claim = [{(?person, name)}, ?name, ?appeared]
  in effect @NOW, @NOW
  via ?assertion = [
    {(?claim, posit), (?source, ascertains)},
    ?certainty,
    ?asserted
  ]
return ?person, ?name, ?source, ?certainty, ?asserted;
```

Without `via`, provenance remains hidden but its source-local bag multiplicity
is preserved. `via` must describe the exact `{posit, ascertains}` assertion
shape. `latest` cannot be combined with `in effect`.

Assertions are ordinary posits joined explicitly through the reserved `posit`
and `ascertains` roles:

```traqula
search ?claim = [{(?case, status), ...}, ?status, ?changed],
       [{(?claim, posit), (?source, ascertains)}, ?certainty, ?asserted]
return ?case, ?status, ?changed, ?source, ?certainty, ?asserted;
```

Five Roles have fixed identities in the accepted version 1 model: `posit`,
`ascertains`, `thing`, `class`, and `subclass`. Only `posit` and `ascertains`
participate in built-in assertion resolution. Information-in-effect selection is
implemented as the neutral `in effect` query operation. Source preference,
certainty combination, and accepted-truth selection remain separate application
policies. None alters
ordinary snapshot semantics or silently discards disagreement.

The three classification Roles are stable vocabulary, not an inference engine.
Traqula treats their values as ordinary literals and does not give `"active"`,
`"inactive"`, or any other value hidden meaning. Direct classification evidence
can be explored with an ordinary pattern:

```traqula
search ?classification = [
  {(?member, thing), (?class, class)},
  ?state,
  ?appeared
]
return ?classification, ?member, ?class, ?state, ?appeared;
```

A consumer may select a class and decide which states, positors, certainties,
and temporal cuts to display. Subclass closure is likewise an explicit consumer
policy over `{(?child, subclass), (?parent, class)}` posits; ordinary search does
not traverse it implicitly.

## Query algebra

Multiple patterns in one branch form a natural join and preserve bag
multiplicity. `union` appends another branch. Returned variables and variables
used by an outer `where`, `order by`, or correlated `not exists` must be bound
in every union branch, with the same domain in each branch. Other positive
bindings remain private to their branches and are not promoted into mutation
scope after the search.

`not exists` is a safe correlated anti-join. Variables already bound positively
are correlated; variables introduced inside the block are local existential
variables and cannot escape it. Separate blocks have separate local scopes.
Absence of recorded evidence does not assert that a proposition is false.

```traqula
search [{(?person, name), ...}, ?name, *]
union [{(?person, alias), ...}, ?name, *]
not exists { [{(?person, retired), ...}, "yes", *] }
return distinct ?person, ?name
order by ?person
limit 100;
```

The result pipeline is projection, optional `distinct`, deterministic
`order by`, then `limit`. Without `distinct`, duplicate rows are meaningful and
are retained. `asc` is the default direction; `desc` reverses it.
Limits must be non-negative integers within the supported count range; negative
or overflowing counts are errors. Ordering literal values requires compatible
numeric families (integer/decimal) or certainty values; certainty cannot be
mixed with ordinary numbers. String ordering is unsupported.

## Execution plans

Prefix a search with `explain` to inspect the chosen plan:

```traqula
explain search [{(?person, name)}, ?name, ?time] as of '2025'
return ?person, ?name
order by ?person
limit 20;
```

`explain` produces one result set with one `plan` column and one JSON literal
cell. It uses the usual Rust, HTTP, SSE, WASM, and Python result framing. It
does not scan result rows, resolve effective evidence, allocate identities, or
execute an `or add` fallback. It validates the search and fallback binding
shape before returning a plan. Other commands in the same script still execute
normally; `explain` applies only to its attached search.

The JSON has an independent `version` (`2`), `resolved_now`, and an operator
tree under `root`. Each node describes its operation, typed `bindings`, and
`inputs`. Plans show chosen pattern order, scopes and dependencies, full scans
or bitmap access, candidate estimates, temporal reduction and field-filter
placement, effective-evidence cuts, projection, ordering, and limits. Candidate
estimates come from catalog/index sizes and are upper bounds, not measured
cardinalities after matching. A changed database may produce a different plan.

The plan's `incremental` flag reports whether scans, nested-loop joins, union,
filters, absence checks, and projection pass rows directly to their consumers.
For these plans, predicates run as soon as their operands are bound, after any
temporal reduction in the producing pattern. An absence block stops at its
first complete match. `distinct` retains the first occurrence of each projected
row. The unbounded result has the same row order and bag multiplicity as full
materialization.

An eligible unsorted `limit` reports `presentation_only: false`: execution stops
after finding one additional projected, distinct-if-requested row, so `limited`
is true only if an omitted row actually exists. A stopped row sink also stops
upstream work. An ordered limit still consumes every input row, validates all
sort keys, and uses `top_k` to retain at most `limit + 1` rows. Numeric sort keys
are interpreted once per row using exact decimal arithmetic. Equal sort keys
preserve input order. An unlimited sort retains the full result. A limit of
the largest supported integer uses the full sort to avoid lookahead overflow.

`materialization_reason` is null for incremental plans. Otherwise it identifies
the full-evaluation requirement: `literal_comparisons_require_complete_validation`,
`effective_evidence_requires_complete_validation`, or
`search_or_add_requires_complete_identity_bindings`. Nominal literal pattern
matches and non-exact literal predicates can fail on later values; effective
evidence can also fail during resolution. Such searches retain the original
evaluation barriers, even with a small limit or stopped sink. `search ... or add`
always validates and collects all matching identities before presentation or
fallback mutation. Static query errors and sort comparison errors still occur
before metadata or rows are emitted.

Incremental execution does not imply a constant-memory query: temporal patterns
still reduce complete candidate histories, bitmap scans retain their candidate
postings, full scans sort candidate identities, and `distinct` retains its seen
rows. Cancellation or operational errors may terminate a stream after earlier
rows were delivered. The explained query's limit does not truncate its
explanation. `EXPLAIN ANALYZE` is not implemented.

Rust callers can use `Engine::explain` or `Engine::explain_with_options` to get a
typed `QueryPlan` directly. These methods accept exactly one search (optionally
prefixed by `explain`) and reject scripts containing other commands before any
mutation. Typed parameters and a supplied clock resolve once per invocation,
as for normal execution. The returned plan describes that invocation; it is
not a reusable prepared-query handle.

`explain` is contextual and does not reserve an existing mutation-variable name.

## Execution budgets

`ExecutionOptions::query_limits` accepts `QueryLimits { max_work_units,
max_memory_bytes }`. Both fields are optional unsigned 64-bit integers. Rust,
Python, and WASM default to unlimited query budgets; zero is a valid cap. HTTP
applies server caps even when the request omits these fields.

```rust
use positorium::{ExecutionOptions, QueryLimits};

let options = ExecutionOptions {
    query_limits: QueryLimits {
        max_work_units: Some(1_000_000),
        max_memory_bytes: Some(64 * 1024 * 1024),
    },
    ..ExecutionOptions::default()
};
// Pass options to Engine::execute_collect_with_options or a streaming method.
```

Work units accumulate across all searches in one script. They charge candidate
visits, structural matching attempts, join and filter steps, temporal reduction,
projection, and batched index/sort work. Bitmap operations are charged from
their input cardinalities before running; comparison sorts prepay
`rows * ceil(log2(rows)) * key_count`. These are logical units, not elapsed time
or exact CPU comparisons. Counts depend on the plan and implementation and may
change between releases. Use a deadline as well when wall-clock time matters.

The memory cap bounds **simultaneously reserved query workspace**. Accounting
includes owned bindings and literal tokens, candidate postings and full-scan
identities, materialized relations, distinct seen-state, ordering buffers,
temporal scratch, fallback identity sets, and retained effective-cut cache
entries. Reservations are released as operators finish, so memory can be reused
by the next search. Vector capacity and owned payload are charged explicitly;
trees, hash sets, bitmaps, and temporal scratch use conservative per-entry
allowances. This is an accounting model, not a measurement or hard limit on
process RSS or every allocator request.

Shared database objects, parsing/planning, mutation staging, and caller-owned
output buffers are outside these query budgets. Built-in collecting APIs also
retain their output outside the workspace cap. Combine budgets with result row
limits or streaming when output could be large. `EXPLAIN` does not execute its
query and does not consume query work or workspace budgets.

Exhaustion returns `DatabaseError::ResourceLimit`, identifying `query work units`
or `query memory bytes` and the configured cap. It aborts the script: a failed
search never triggers `or add` or subsequent mutations. Earlier successful
mutation commands remain committed. A stream may already have emitted metadata
or rows before a budget fails; those rows are a partial result, not successful
completion. A new execution starts with fresh budgets.

HTTP requests use a `query_limits` object with `max_work_units` and
`max_memory_bytes`; clients may only lower the configured server caps. See
[operational limits](../guides/OPERATIONS.md#trusted-interface-and-resource-limits).
WASM callers use `engine.execute_with_query_options(script, { query_limits:
{ max_work_units: 1000000, max_memory_bytes: 67108864 } })`; the same options
object also accepts `parameters` and `max_rows_per_search`. Existing WASM
execution methods retain their defaults. Python exposes `max_work_units` and
`max_memory_bytes` as execution keywords; see
[execution controls](../guides/PYTHON.md#execution-controls).

## Typed parameters

`$name` denotes a value supplied separately by the Rust, HTTP, or WASM API.
Parameters are typed as either literal or time values. They never substitute
source text, roles, variables, or grammar.

```traqula
search [{(?item, amount), ...}, ?amount, *] as of $cutoff
where ?amount = $target
return ?item, ?amount;
```

HTTP request:

```json
{
  "traqula_version": 1,
  "script": "search [{(?item, amount), ...}, ?amount, *] as of $cutoff where ?amount = $target return ?item, ?amount;",
  "parameters": {
    "cutoff": {"kind": "time", "text": "2024-12-31"},
    "target": {"kind": "literal", "text": "6.00"}
  },
  "stream": false
}
```

Missing parameters, invalid parameter tokens, and use in the wrong domain are
typed errors.

## Results and errors

Result cells carry both a lossless text representation and a kind: Thing, Role,
Posit, AppearanceSet, Literal, or Time. Native, HTTP, NDJSON, and WASM surfaces
use the same structured result contract.

Parse errors, unknown variables, inconsistent variable domains, unsafe
negation, invalid recall, unsupported comparisons, bad parameters, timeouts,
and cancellation are reported as errors. They do not panic and do not silently
reinterpret source.

For complete API and compatibility boundaries, see [CONTRACTS.md](CONTRACTS.md).
For startup, backup, validation, and recovery procedures, see
[OPERATIONS.md](../guides/OPERATIONS.md). Worked modeling and transfer recipes are in
[COOKBOOK.md](../guides/COOKBOOK.md).
