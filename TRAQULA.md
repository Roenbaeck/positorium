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
  applies retractions. This dual-cut operator is planned post-version 1 syntax
  and is not accepted by the current parser.

The target pattern can look identical because the operators answer different
questions:

```traqula
/* Raw state: which name posits are latest by appearance time? */
search
  [{(?person, name)}, ?name, ?appeared] as of @NOW
return
  ?person, ?name, ?appeared;

/* Effective evidence: which names do sources currently assert? (planned) */
search
  [{(?person, name)}, ?name, ?appeared] in effect @NOW, @NOW
return
  ?person, ?name, ?appeared;
```

The first query can return a latest name posit even if nobody asserts it. The
second returns one binding per matching effective assertion, so two sources
asserting the same name produce two rows unless `return distinct` is requested.
The first `in effect` operand is assertion time and the second is target
appearance time; the fixed arity makes the comma syntax unambiguous.

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
a planned query operation. Source preference, certainty combination, and
accepted-truth selection remain separate application policies. None alters
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
multiplicity. `union` appends another branch. `not exists` is a safe correlated
anti-join: every variable it references must already be bound positively.

```traqula
search [{(?person, name), ...}, ?name, *]
union [{(?person, alias), ...}, ?name, *]
not exists { [{(?person, retired), ...}, true, *] }
return distinct ?person, ?name
order by ?name, ?person
limit 100;
```

The result pipeline is projection, optional `distinct`, deterministic
`order by`, then `limit`. Without `distinct`, duplicate rows are meaningful and
are retained. `asc` is the default direction; `desc` reverses it.

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
[OPERATIONS.md](OPERATIONS.md). Worked modeling and transfer recipes are in
[COOKBOOK.md](COOKBOOK.md).
