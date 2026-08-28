# Positorium beta cookbook

These patterns use Traqula version 1. They model evidence without overwriting
history or assigning hidden truth semantics. Query variables begin with `?` and
are scoped to one `search`; add-side allocation variables begin with `+` and
remain available to later mutation commands in the same script.

## Corrections and simultaneous disagreement

A correction is another posit for the same exact appearance set. Do not delete
the earlier value:

```traqula
add role name;
add posit [{(+person, name)}, "Alicia", '2024-01-01'];
add posit [{(person, name)}, "Alice", '2024-02-01'];
add posit [{(person, name)}, "Alix", '2024-02-01'];

/* Complete history. */
search [{(?person, name), ...}, ?name, ?time]
return ?person, ?name, ?time
order by ?time;

/* Snapshot: both equal-time maximal disagreements remain visible. */
search [{(?person, name), ...}, ?name, ?time] as of @NOW
return ?person, ?name, ?time;
```

An ordinary snapshot does not decide which of `"Alice"` or `"Alix"` is true.
That decision belongs to an explicit application or assertion-resolution policy.

## Assertions, sources, and certainty

`posit`, `ascertains`, `thing`, `class`, and `subclass` have fixed reserved Role
identities. Only `posit` and `ascertains` define the built-in assertion shape. An
assertion is an ordinary posit that refers to another posit's identity:

```traqula
add role status, source;
add posit +claim [{(+case, status)}, "open", '2024-03-01'];
add posit [{(+registry, source)}, "Registry A", @NOW];
add posit [{(claim, posit), (registry, ascertains)}, 75%, @NOW];

search ?claim = [{(?case, status), ...}, ?status, ?changed],
       [{(?claim, posit), (?source, ascertains)}, ?certainty, ?asserted]
return ?case, ?status, ?changed, ?source, ?certainty, ?asserted;
```

This is a manual assertion join. Ordinary `as of` never follows assertion
posits implicitly, chooses a preferred source, combines certainty, or treats a
negative certainty as deletion. `in effect T, t` selects source-local effective
assertions explicitly and still does not choose truth:

```traqula
add role status, source;
add posit +claim [{(+case, status)}, "open", '2024-03-01'];
add posit [{(+registry, source)}, "Registry A", @NOW];
add posit [{(claim, posit), (registry, ascertains)}, 75%, @NOW];

search ?claim = [{(?case, status)}, ?status, ?changed]
  in effect @NOW, @NOW
  via ?assertion = [
    {(?claim, posit), (?source, ascertains)}, ?certainty, ?asserted
  ]
return ?case, ?status, ?source, ?certainty, ?asserted;
```

The first cutoff applies to assertion time and the second to the target posit's
appearance time. A later zero-certainty assertion by the same source retracts
that exact target posit. Omit `via` when provenance columns are not needed; bag
multiplicity by effective source is still preserved.

## Classification without hidden lifecycle semantics

The reserved classification Roles make raw patterns interoperable without
turning Positorium into an ontology engine:

```traqula
add posit [{(+person_class, class)}, "declared", @NOW];
add posit [{(+ada, thing), (person_class, class)}, "included", @NOW];

search ?classification = [
  {(?member, thing), (?class, class)},
  ?state,
  ?time
]
return ?classification, ?member, ?class, ?state, ?time;
```

`"declared"` and `"included"` are application values, not reserved lifecycle
states. A UI may instead choose `"active"` as its display convention. It must
also choose source, certainty, temporal, and optional subclass-traversal policy;
the database never infers those choices from the literal text.

## External identification without identity merging

Store-local Things are never destructively merged. Reify a proposed
identification and each membership so competing proposals can coexist:

```traqula
add role customer, identification, membership, group, member;
add posit [{(+left, customer)}, "CRM-17", @NOW],
          [{(+right, customer)}, "IMPORT-91", @NOW],
          [{(+same_entity, identification)}, "proposed match", @NOW];
add posit [{(+left_membership, membership), (same_entity, group), (left, member)}, "member", @NOW],
          [{(+right_membership, membership), (same_entity, group), (right, member)}, "member", @NOW];
```

Queries opt into the `same_entity` proposal. Storage and ordinary queries keep
`left` and `right` distinct.

## Multi-valued attributes and tags

An appearance set permits at most one Thing per Role, so do not repeat a `tag`
role in one set. Give each membership its own identity:

```traqula
add role document, tag, membership, container, member;
add posit [{(+doc, document)}, "report.pdf", @NOW],
          [{(+blue, tag)}, "blue", @NOW],
          [{(+urgent, tag)}, "urgent", @NOW];
add posit [{(+blue_link, membership), (doc, container), (blue, member)}, "member", @NOW],
          [{(+urgent_link, membership), (doc, container), (urgent, member)}, "member", @NOW];
```

Each link now has its own value, time, history, and potential assertions.

## Relations with repeated participant roles

When a relation contains several participants of the same logical kind, reify
participant positions instead of repeating one Role:

```traqula
add role transfer, account, participant, relation, position, member;
add posit [{(+wire, transfer)}, "wire-42", @NOW],
          [{(+alice, account)}, "Alice", @NOW],
          [{(+bob, account)}, "Bob", @NOW];
add posit [{(+position_1, participant), (wire, relation), (alice, member)}, "sender", @NOW],
          [{(+position_2, participant), (wire, relation), (bob, member)}, "recipient", @NOW];
```

The two position Things allow any number of participants while every individual
appearance set remains a finite partial function from Role to Thing.

## Constraints are conformance policy

Literal values remain recorded exactly as entered. If one context requires a
unit, range, scale, or allowed vocabulary, record or configure that constraint
outside the value's physical codec and evaluate conformance explicitly. A
nonconforming posit remains part of history; it is not silently coerced or
discarded. The general constraint model is still research: the current direction
is a deterministic versioned program evaluated over an immutable effective
snapshot with explicit findings and provenance, not built-in cardinality roles.

## Backup, restore, and native transfer

Stop the writer before offline maintenance. A physical backup preserves the
store UUID and copies the manifest plus the committed log prefix as one unit:

```text
positorium-store inspect positorium.store
positorium-store backup positorium.store backup.store
```

Restore by configuring Positorium to open `backup.store` while no other process
owns it. For transfer into a new identity domain, dump and import logically:

```text
positorium-store dump positorium.store export.jsonl
positorium-store import export.jsonl imported.store identity-remap.json
```

Import creates a new store UUID, remaps every non-built-in identity, rewrites
all references, and emits the complete mapping. Keep the remap artifact with
the transfer record. There is no SQLite import path because no release used the
prototype SQLite store.
