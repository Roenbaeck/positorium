# Positorium Beta Core Model

This document is the normative, storage- and Traqula-independent model contract
for the Positorium beta. The words **must**, **must not**, **should**, and **may**
are used in their usual requirements sense. `DECISIONS.md` records why these
rules were chosen; this document states the resulting behavior.

## Constructs and identity

A **Thing** is an opaque, store-local unsigned identity. Identity `0` is not
allocated. Allocated identities increase monotonically, may contain gaps, and
must never be recycled. A durable external reference is the pair `(store UUID,
local Thing)`; a bare numeric Thing from one store has no meaning in another.

A **Role** is a Thing plus immutable catalog metadata: one canonical name and a
reserved flag. Role equality, hashing, and ordering use only the Role's Thing
identity. The catalog enforces both directions of uniqueness: one canonical
name per Role identity and one Role identity per canonical name. Names are
case-sensitive and NFC-normalized at parser and catalog boundaries. Literal
values are not Unicode-normalized. A role cannot be renamed; a changed name
creates a new Role.

Only `posit` (identity `1`) and `ascertains` (identity `2`) are reserved in the
beta. `thing`, `class`, `classification`, `named`, `subclass`, and `superclass`
have no built-in meaning and remain ordinary user-defined roles.

An **Appearance** is the ordered pair `(Role, Thing)`. Its logical identity,
equality, hashing, and ordering use that pair.

An **AppearanceSet** is a finite partial function from Role to Thing. Its
canonical representation sorts appearances by Role identity and therefore does
not depend on input order. Construction with two appearances for the same Role
must fail with a domain error. It must never silently discard a member or pick
one of the Things.

A **LiteralValue** is the complete UTF-8 token entered for a value, excluding
comments and whitespace outside the token. Leading zeros, explicit signs,
numeric scale, string escape spelling, JSON whitespace, JSON key order, JSON
number spelling, and JSON escape choices are identity-bearing. Physical codec
metadata is not. Literal identity, nominal semantic equality (`=`), and
possible-value compatibility (`?=`) are distinct relations.

A **Time** stores both a temporal value and its expressed precision. Stored-time
equality requires identical value and precision.

A **posit proposition** is exactly:

```text
(AppearanceSet, LiteralValue, Time)
```

A **Posit** is that proposition plus a separately assigned Thing identity. Posit
equality, hashing, ordering, and duplicate detection use the proposition only;
the Posit Thing is excluded. Re-adding the same proposition returns the
canonical Posit and creates no new evidence. Repeated observations, provenance,
and confidence are modeled as ordinary assertion posits.

## Write invariants and query policies

The engine enforces these invariants before acknowledging a write:

- identities are store-local, monotonic, and never recycled;
- Role identity and canonical name mappings are both unique;
- Role names are NFC-normalized and case-sensitive;
- an AppearanceSet has at most one Thing for each Role;
- an identical posit proposition is idempotent;
- persisted references name committed Things and Roles; and
- the built-in Role identities and names are compatible with the store.

These are query policies, not write-time truth constraints:

- which assertion or positor is preferred;
- whether conflicting values can be reconciled;
- whether two Things identify the same external entity;
- which value constraints apply in a particular modeling context; and
- which maximal posit(s) form a snapshot at a cutoff.

Positorium preserves disagreement. Append order, record sequence, and Thing
identity must never become implicit truth, preference, or conflict tie-breakers.

## Value slots and reification

One exact AppearanceSet identifies one value-bearing transition slot over time.
A later posit for the same set does not update or delete an earlier posit; it
adds another immutable proposition to the slot's history.

Because an AppearanceSet is a partial function, repeating a Role is not a way to
model a collection. Reify the collection, membership, or relation instead.
These examples use conceptual tuple notation and ordinary user-chosen roles;
they do not reserve vocabulary.

### Alias or alternate name

Give each alias its own member Thing and timeline:

```text
{(person-42, alias-owner), (alias-1, alias-member)} -> "Ada"
{(person-42, alias-owner), (alias-2, alias-member)} -> "A. Lovelace"
```

### Tags and memberships

Represent each membership as a Thing so membership value, time, evidence, and
later correction have an independent slot:

```text
{(membership-7, membership), (document-9, member), (tag-blue, container)} -> true
{(membership-8, membership), (document-9, member), (tag-urgent, container)} -> true
```

### N-ary or repeated-participant relation

Create a relation Thing and distinct participant-position Things when the same
logical participant role repeats:

```text
{(transfer-3, relation), (account-a, source)} -> true
{(transfer-3, relation), (account-b, destination)} -> true
{(transfer-3, relation), (witness-position-1, participant-position)} -> account-c
{(transfer-3, relation), (witness-position-2, participant-position)} -> account-d
```

The position Things allow any number of participants without placing the same
Role twice in one AppearanceSet.

## Temporal meaning

Finite times denote half-open UTC intervals at their stated precision:

```text
2024                 = [2024-01-01T00:00:00Z, 2025-01-01T00:00:00Z)
2024-05              = [2024-05-01T00:00:00Z, 2024-06-01T00:00:00Z)
2024-05-17           = [2024-05-17T00:00:00Z, 2024-05-18T00:00:00Z)
2024-05-17 12:30:00  = one representable datetime instant
```

Traqula beta has no local-time or offset coercion; clients convert to UTC before
submission. Leap seconds are unsupported. `@BOT` and `@EOT` are unbounded
sentinels below and above every finite interval.

Ordinary `<`, `<=`, `>`, and `>=` are conservative definite relations. `<`
means the left interval ends at or before the right interval starts; `>` is its
converse. `<=` and `>=` additionally accept identical stored times. A definite
comparison that is indeterminate returns false. `=` means identical stored time,
not interval overlap. Explicit relations provide `possibly before`, `possibly
after`, `overlaps`, `contains`, and `within`.

`@NOW` resolves once at the start of a complete script, at full supported UTC
datetime precision. Every occurrence uses that value. Execution options may
override it, and execution metadata reports it.

For an ordinary snapshot at cutoff `T`, a posit is applicable only when its time
satisfies definite `<= T`. For each complete AppearanceSet, the snapshot returns
every maximal applicable posit under the definite partial order. Equal-time
conflicts and incomparable maximal intervals remain separate results.

Ordinary `pattern as of T` selects AppearanceSets by structure, reduces each
complete set to this snapshot, and only then applies value or other posit-field
predicates. `latest pattern as of T` is the separate filter-first operation:
filter history, group by complete AppearanceSet, then retain every partial-order
maximum.

## External identification and equivalence

Identification is modeled with posits and explicit query policy, not a second
mutable key system. The beta forbids destructive identity merging and
storage-layer aliases.

To state that several Things may identify the same entity, create an ordinary
identification Thing, one membership Thing/posit per identified Thing, and
separate assertion posits for evidence and certainty. For example:

```text
{(identification-5, identification), (member-1, membership)} -> customer-17
{(identification-5, identification), (member-2, membership)} -> imported-91
{(assertion-8, evidence), (identification-5, subject)} -> "same tax identifier"
```

Queries opt into that equivalence explicitly and may preserve competing
identifications. No engine operation rewrites historical AppearanceSets.

Logical export represents every identity as `(source store UUID, local Thing)`.
Import always allocates collision-free local identities, builds an explicit
remap table, and rewrites every internal reference. It never retains foreign
numeric identifiers verbatim.
