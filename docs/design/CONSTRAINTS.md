# Saved Traqula constraints

Beta.5 introduces saved Traqula constraints and version 1 of the finding
report. [Command reference](../reference/TRAQULA.md#saved-constraints)

## What can a business describe?

A business can record “after 30 days, customer service may authorize a
cancellation under extreme circumstances.” This describes who may decide, what
the decision permits, and which evidence a later check should find. The agent
supplies the judgment about the circumstances. The company can keep that policy
and its decision history in Positorium even when it delegates part of the work
to a person.

| Record | What it establishes |
| --- | --- |
| Policy revision | The wording, ordinary deadline, country exceptions, and who has discretion while this revision applies |
| Exception decision | An identified agent approved or denied a particular request under that revision, at a stated time, with any reason the agent supplied |
| Cancellation | A source reports that the business cancelled the order. A reference to the decision records the claim that the business acted on it |
| Constraint finding | An evaluator checked the recorded action against a specific policy revision and evidence snapshot |

A customer's description of an emergency is evidence for the reviewer. The
reviewer's authorization is a separate act. A valid approval *permits* a
cancellation, and the order can still stand. Likewise, a later finding of
conformance establishes what the checker could verify. The decision reference
on the action is what lets someone trace *why* the business says it acted.
One saved query could list late requests awaiting a decision. Another could
audit completed cancellations. A person or workflow takes the action in response.

## The cancellation rule as a test case

For this example, count UTC calendar days from the order date (day zero).
Use the destination country at placement to select an inclusive two-day or
30-day window. The discretionary exception starts after day 30 in either
country. It requires approval for this request by an agent authorized at the
time of the decision, before or at cancellation. These are choices in the
example policy. A business can specify a different calendar, jurisdiction,
or effective date in its own revision.

| Evidence at the chosen cuts | Expected result |
| --- | --- |
| Cancellation on day 2 in an ordinary country, or day 30 in an extended-window country | `satisfied`, including the last permitted day |
| Cancellation on day 5 | `violated` in the two-day country and `satisfied` in the 30-day country. Disputed country evidence spanning both outcomes gives `indeterminate` |
| Day 31, with effective approval for this request and policy from an authorized agent | `satisfied`. Cite the cancellation, decision, and delegation evidence |
| Request awaiting review, with the order still standing | The request is pending. It lies outside the population of **completed cancellations** |
| Day 31, completed, with no qualifying approval in a register the policy declares complete | `violated`. A reason label, a decision on another request, or an approval issued only after cancellation cannot fulfill this recording obligation |
| Day 31, with incomplete review records or unresolved approval and denial from equally authorized reviewers | `indeterminate` when those alternatives change the answer. Keep the evidence for both |

The order date and the cancellation date are business facts. Assertion time
controls what the checker could know. A decision made on time but recorded
tomorrow can change a later evaluation of yesterday's action. An approval
*made* tomorrow has a different meaning under this example policy.

This Traqula 1 query traces a supported cancellation to the **particular
approval assertion** it cites. Two agents can ascertain the same decision
posit. The assertion reference identifies whose approval the action cites.
Both patterns use the same assertion-time cut. The approval must also be in
effect at the time of cancellation for that request and policy revision.
The full check also evaluates the agent's delegation and conflicting decisions.

```traqula
search ?action = [{(?req, request), (?policy, policy), (?approval, relies_on)}, "cancelled", ?at]
         in effect $known_at, $action_cut
         via ?report = [{(?action, posit), (*, ascertains)}, ?reported, *],
       ?decision = [{(?req, request), (?policy, policy)}, "approved", *]
         in effect $known_at, ?at
         via ?approval = [{(?decision, posit), (?agent, ascertains)}, ?support, *]
where ?reported > 0% and ?support > 0%
return ?req, ?action, ?report, ?decision, ?approval, ?agent;
```

## A saved check

A policy is a Thing with a stable identity and a direct membership in the
`Constraint` class. The policy uses the shared `name` role for its name and
the shared `source` role for its Traqula text. Each source posit identifies
one immutable revision. A definition links its human wording, language and
evaluator versions, evidence scope, and parameter declarations. Keep references
to other Things and posits in appearances so logical import can remap them.
Each check identifies the exact revision it evaluated.

### When to create the Constraint class

Create the class on the first successful `add constraint`. A fresh format 1.0
store has five reserved roles and zero posits. First use gives new and
existing stores the same initialization path. A project that wants
`Constraint` to appear in its class browser before any rules exist can
declare and name the class in its setup script.

Use `in class $class` when the caller supplies a designated class Thing. Otherwise look for distinct Things with both a class
declaration and the name `Constraint`. Reuse a single match. With zero
matches, stage a class declaration and a `name` posit for a new Thing. With
several matches, report the candidate identities so the maintainer can
choose. The rule's membership posit records the chosen Thing identity.

Validate the rule body before writing. Under the writer lock, confirm the
class and named policy lookups. Commit any missing `name` and `source` roles,
the new class posits if needed, and the rule's membership, name and source
as one batch. Concurrent first saves by the same name then share one class
and one policy. Rejecting an invalid first rule leaves no partial class
behind. `check constraint` only reads
the published view. On an unused store it reports the requested rule as
unknown. It can also find a rule that another script saved as ordinary posits.

### Traqula commands

Use `add constraint` to save a definition and `check constraint` to evaluate
one. The body is an ordinary read-only Traqula search. Each returned row
identifies a finding. The inner search can also run on its own.

```text
add constraint "Every order has a recorded date" {
    search ?record = [{(?order, order_record)}, *, *]
    not exists { [{(?order, order_date)}, *, *] }
    return ?order, ?record;
};
```

Here one `order_record` posit identifies each order and `order_date` carries
valid dates. The first pattern puts the order in scope and binds `?order`.
The `not exists` clause looks for a date on that same order. Each row
returns the undated order and the record that brought it into scope.
If the rule applies to orders listed in a delivery manifest, start the
search from the manifest entries. An expected order can then be in scope
even if its order record has yet to arrive.

An empty result means “no undated order found in this view.” An audit that
also needs to distinguish “12 orders checked, all dated” from “0 orders
observed” can run a separate `return distinct ?order` search over order
records on the same view and count its rows. The maintainer declares when
the order and date feeds are complete.

For a new policy, `add constraint` validates the read-only query and expands
to one atomic write of the membership, name and source posits. It returns
the policy Thing and the immutable source Posit. The saved rule survives
closing and reopening the store.
For use within the creating script, `add constraint +revision "…" { … };`
also binds the definition identity to the local name `revision`, just as
`add posit +p` binds `p`. A later command in that script can then say
`check constraint revision;`. The binding is optional.

### Adding a revision to a named constraint

An update appends a source posit for the same policy Thing. The
`add constraint "name" { … };` uses the name as a lookup within the designated
`Constraint` class. Count distinct policy Things before looking at their
source history. With zero matches, create a policy. With one match, reuse
that Thing and append its source revision. With several matches, return
their identities. The caller can target one with
`add constraint of $policy { … };`. Independent policies can share a name.

The optional `at` clause supplies the new source's appearance time. It
defaults to the script's resolved `@NOW`. For example,
`add constraint +next of $policy at $starts { … };` can schedule a change
or record one retroactively. The same policy, exact code token and identical
appearance time identify the same posit on retry. A different appearance
time gives a distinct revision even for identical code. Assertions about an earlier
revision continue to target that particular posit.

### Recalling a constraint from another script

A second script can open the same store and check the saved rule by name.

```text
check constraint "Every order has a recorded date";
check constraint "Every order has a recorded date" as of '2026-11-30 12:00:00';
```

Resolve the recorded name and direct class membership to one policy Thing
on the pinned view. An unknown name gives a lookup error and several matching
Things give an ambiguous name. For that one policy, read its `source` slot
using ordinary Traqula `as of` semantics. The first command uses `@NOW`.
The second selects the rule applicable at the supplied policy time. For a
November cancellation checked in December, this allows the checker to use
the November rule. The saved query's evidence cuts and load readiness are
separate inputs.

Select all maximal revisions whose appearance time is definitely at or
before the policy cutoff. With one revision, validate and run it. A policy
awaiting its source is incomplete. If it has revisions, but none qualifies
at the cutoff, report that there is no applicable revision. Distinct
equal-time or incomparable maxima produce an ambiguity with candidate Posit
references. Arrival order, identity number and certainty supply no default
tie-breaker. A policy that requires endorsement can explicitly select
assertion-backed evidence at an assertion-time cut. [Snapshot and assertion semantics](../reference/TRAQULA.md#history-snapshots-and-assertions-are-separate)

`check constraint $revision;` directly selects that immutable source posit,
including a superseded one. The checker verifies its shape, class membership
and read-only program contract. Record the resolved revision, the policy
cutoff if used and the evidence snapshot in the result. A subsequent
backdated append can change what a fresh historical lookup finds. A caller
can also supply a readiness declaration when checking a completed load.

```text
check constraint $revision with readiness $load_complete;
```

Here the caller binds `$revision` and the optional `$load_complete` to
store-qualified Posit references. Identity parameters in these command
positions carry the store UUID and identity. The checker verifies that the readiness
declaration covers the selected register and that its deliveries are visible
in the pinned view. `check constraint $revision;` can also run during a load.
For this missing-date rule it can report an undated order while conformance
of the finished register is still `indeterminate`. Saving and checking are
independent operations. The maintainer chooses when to request a verdict.

### The definition posit on disk

Make each constraint revision an ordinary posit. Its appearance set identifies
the policy Thing in the `source` role, its value is the Traqula search as a
string, and its time is the revision's appearance time. Assertions and
findings refer to that source posit by its own identity.

The `add constraint` captures the text inside the braces, including
comments, whitespace and literal spelling. It uses one deterministic string
encoding so identical source produces the same quoted token. Positorium
reuses the same posit for the same appearances, exact token and time. Keep the
complete appearance set `{(policy, source)}` stable across revisions so that
`as of` reduces them together. Put references to the revision and its
immutable language, parameter and scope declarations in companion posits.
Treat conflicting interpretation metadata for the same source posit as a
definition conflict. Record the exact declarations used by an audit.
[Posit identity and value slots](../reference/MODEL.md#value-slots-and-reification)

Format v1 already stores this as a Posit record in the append-only `.ptl`
log. The record carries the posit identity, appearances, appearance time and
the string literal. The value payload is a byte count followed by the exact
UTF-8 token, including its quotes and escapes, using the `raw-utf8-token`
codec. The writer flushes the batch and its Commit to the log, then records
the committed length by atomically replacing `manifest.pmf`. Reopening the
store replays the committed records and rebuilds the indexes. The
command commits the definition and its associated metadata together.
[Posit and Commit records](../reference/STORAGE.md#9-posit-record)

Use a string for the first implementation. The evaluator can parse and
type-check it as a read-only query under the recorded language version.
Reading the posit returns its source. Execution happens on `check constraint`,
which applies the same checks to definitions written directly or imported.
A parsed plan can be rebuilt from the source. A separate Traqula literal
family would be useful if ordinary queries need to recognize or manipulate
programs as values across roles. It would need defined literal syntax,
equality semantics, and support in the store and logical transfer formats.

### Storing a search body in beta.4

Traqula 1 already reserves `thing` and `class`. This beta.4 example
illustrates first-use lookup in a store with at most one declared class
named `Constraint`. The caller supplies the query text as the string literal
parameter `$source`.

```traqula
add role name, source;
search [{(?constraint_class, class)}, "declared", *],
       [{(?constraint_class, name)}, "Constraint", *]
or add posit [{(+constraint_class, class)}, "declared", '2026-11-20'],
             [{(constraint_class, name)}, "Constraint", '2026-11-20'];

/* First revision of a new policy. */
add posit [{(+policy, thing), (constraint_class, class)}, "included", '2026-11-20'],
          [{(policy, name)}, "Every order has a recorded date", '2026-11-20'],
          +definition [{(policy, source)}, $source, '2026-11-20 00:00:00'];
```

The beta.4 fallback binds all matching class identities, and the mutating
commands above commit individually. The `add constraint` checks
for a single target and writes the whole definition in one batch.
[Search-or-add semantics](../reference/TRAQULA.md#searching-or-adding)

While `policy` is bound, a second source posit schedules a new revision.
The caller supplies another query as the string parameter `$new_source`.

```traqula
add posit +next [{(policy, source)}, $new_source, '2026-12-01 00:00:00'];
```

A fresh script can inspect the recorded revision applicable at a policy
cutoff. This fixture has one matching class and policy. An unqualified
`source` pattern retrieves the full revision history.

```traqula
search [{(?policy, name)}, "Every order has a recorded date", *],
       [{(?policy, thing), (?kind, class)}, "included", *],
       [{(?kind, class)}, "declared", *],
       [{(?kind, name)}, "Constraint", *],
       ?definition = [{(?policy, source)}, ?body, *] as of '2026-11-30 12:00:00'
return distinct ?kind, ?policy, ?definition, ?body;
```

The example uses recorded direct membership with the value `"included"` for
discovery and reads the exact unary `source` slot. The membership and
declaration values are conventions of this checker.
Effective applicability and support can be inspected separately at the
chosen cuts. [Role naming anti-pattern](../guides/COOKBOOK.md#anti-pattern-of-class-qualified-role-names)

### Asserting and disputing a constraint revision

The definition makes a proposition a source can take a position on. In this
example it says that this query expresses the order-date policy from the
stated appearance time. Operations supports that definition and Customer
Service disputes it. Append these Traqula 1 commands to the script that
bound `definition`.

```traqula
add posit [{(+operations, name)}, "Operations", '2026-11-20'],
          [{(+service, name)}, "Customer Service", '2026-11-20'];
add posit [{(definition, posit), (operations, ascertains)}, 100%, '2026-11-20'],
          [{(definition, posit), (service, ascertains)}, -100%, '2026-11-20'];
```

Both assertions target the same immutable revision. An `in effect` query
with `via` can return each source, its signed certainty and its assertion
time. A later `0%` from Customer Service withdraws its own position on that
revision. A maintainer can still request an informative check of the
revision and see what it would find. [Querying assertion evidence](../reference/TRAQULA.md#history-snapshots-and-assertions-are-separate)

The target of an assertion matters. If both departments agree on the wording
but disagree about applying it in Sweden, record the claim of applicability
to Sweden as its own posit referring to the revision. A saved audit finding
can likewise refer to the exact revision, the checked snapshot and the order.
Someone who accepts the rule can dispute that finding by asserting a
position on the finding's own posit.

### Evaluation contract

The shared Rust evaluator takes a definition Posit, parameter values,
time cuts and an optional readiness declaration. The definition declares
the scope and the output columns. The caller selects the revision to
evaluate and can request an audit record of the result.

1. **Save.** Parse and type-check each finding query and its projected subject
   and witness columns. A definition may give separate queries for definite
   violations and unresolved cases. Associate each with a finding code and
   explanation. Admit read-only projected searches. Reject mutations, including
   a nested `or add posit` fallback, before storing an executable definition.
2. **Check on demand.** Resolve the definition, typed parameters and clock once
   on one published read view. Express restrictions on eligible subjects in
   the finding search. Apply the same evidence scope to positive joins and
   absence checks. The first fixture uses recorded history. A policy using
   effective evidence specifies
   the assertion and appearance cuts and how it treats sources, certainties
   and retractions.
   A missing date violates a rule requiring it in a complete order register.
   The same absence in a partial feed can leave a question about the order
   indeterminate.
3. **Return findings.** Each finding identifies the subject, what was expected,
   what was observed, and the relevant witness posits. For a rule over every
   subject, any definite violation makes the summary `violated`. Otherwise an
   unresolved case makes it `indeterminate`. To conclude `satisfied`, the
   check must finish and meet the rule's declared coverage requirements for
   both the population and its evidence. Report a population count only when
   the audit enumerates that population. An empty order feed can still be
   awaiting delivery. Parse errors, timeouts, exhausted budgets and truncated
   results have operational outcomes separate from the verdicts of completed
   checks. The checker rejects a query `limit` in a finding or evidence search.

For replay, retain the definition identity and source digest, input scope,
evaluator version, parameters, clock, cuts, and store identity. A manifest of
all Posit identities in canonical order identifies the checked store view.
Report version 2 retains its count and digest by default. Request the list
itself with `with full manifest`. The actual read identities and smaller
per-finding witness sets have their own fields. A backdated append can change the result
at identical time cuts.
An optional audit write appends the completed evaluation as ordinary posits
ascertained by the evaluator after the read view closes. A later check can
then use those findings as evidence.

## When is a load ready for validation?

A published snapshot establishes what Positorium has received. The maintainer
knows which deliveries the rule expects. Consider an order header received in
one batch and its date supplied by another. An on-demand search between them
can correctly report “order 42 has no recorded date in this snapshot.” While
the dates batch is outstanding, conformance of the finished register is
`indeterminate`. If the responsible maintainer declares the order and date
feeds complete for that load and the date is still absent, the same observation
supports a `violated` finding.

Some findings need only one witness. A recorded quantity of `-5` already
violates a rule requiring every recorded quantity to be nonnegative, even
while other deliveries are outstanding.

Readiness belongs to a particular scope, such as one source's order export
for a given business day. A maintainer can declare that scope ready when
invoking a check. For recurring loads, a trusted loader can make the same
declaration under an agreed delivery protocol. One way to preserve it is an
ordinary posit identifying the covered deliveries, ascertained by the loader.
The check can run automatically once that declaration **and the deliveries
it covers** are visible locally. This also handles an end-of-load message
arriving ahead of the data. Record the declaration used and the snapshot
checked. A late correction can prompt a new evaluation while preserving the
earlier finding.

## When to check and when to enforce

The default is an informative check on demand. **When a check runs** and
**what its verdict controls** are separate choices. Saving a snippet makes it
available for evaluation. A separate, explicit policy can select a revision,
its evidence scope and the operation or event that invokes it.

| Use | When it runs | What the result does |
| --- | --- | --- |
| On-demand audit | A caller asks about an order or a register at chosen cuts | Returns the verdict and its witnesses |
| Automatic audit | A configured load reaches its declared completion point, relevant evidence changes, or a deadline arrives | Calls the same informative checker. Any finding about missing evidence carries the rule's coverage assumptions |
| Strict action policy | A workflow is about to perform a named action, such as finalizing an order | Permits, refuses, or holds that particular action according to the policy |

Consider a day-31 cancellation. Record the customer's **request** when it
arrives. Before carrying it out, the workflow can require a qualifying
approval. Under that policy, `satisfied` permits cancellation, `violated`
blocks it, and `indeterminate` sends the request to review. A timeout holds
the action for retry and is reported as an execution failure. When a partner
**reports** that it already cancelled the order, record the assertion. An
audit can now expose a cancellation lacking the required authorization.
The store still enforces representation invariants on every write.
[Write invariants and query policies](../reference/MODEL.md#write-invariants-and-query-policies)

Implement the on-demand evaluator first. Opt-in automatic reporting can reuse
it after commit. Strict enforcement needs a further guarantee for an action
committed by Positorium. Evaluate the proposed action with its relevant
evidence and policy revision, and commit against that same state or recheck
if it has changed. This covers an approval withdrawn between preview and
commit.

## Temporal operations for Traqula

The beta already orders appearance-time bindings with definite comparisons.
`?=` on two such bindings tests interval overlap. Dates in the **value** slot
are literal tokens with different comparison semantics. Beta.5 adds typed
interval predicates and calendar arithmetic on appearance times and date-valued literals.
[Traqula time and snapshots](../reference/TRAQULA.md#time-and-snapshots)

| Operation | Semantics and example |
| --- | --- |
| Overlaps and disjoint | Finite half-open intervals overlap when they share an instant. Disjoint is the complement. March contains March 31, while March and April merely touch and are disjoint |
| Contains and within | Every instant of the inner interval lies in the outer one. Within reverses the operands |
| Definitely or possibly before and after | Distinguish a relation holding for every admissible pair of instants from one holding for at least one pair |
| Add or subtract calendar days | Shift both bounds by a signed number of civil days. February 28, 2024 plus one day is February 29. Shifting a whole month may produce an interval spanning parts of two months |
| Elapsed calendar days | Two exact civil dates give a signed day difference. Coarse dates give the range of possible differences. An order placed sometime in March and cancelled April 1 spans 1–31 days |

This pseudocode gives the reference calculation for one interpretation of the evidence.

```text
elapsed   = calendar_day(cancellation_date) − calendar_day(order_date)
ordinary  = 0 ≤ elapsed ≤ country_window
exception = elapsed > 30 and qualifying_approval(request, policy_revision, cancellation_time)
permitted = ordinary or exception
```

Evaluate the admissible dates, countries and decisions. Agreement across the
alternatives gives a definite answer. Mixed answers give an unresolved finding.
When essential evidence is missing, apply the policy's explicit recording
obligations or report that the case is indeterminate.

Use checked arithmetic with typed errors for overflow, incompatible operands,
and arithmetic on `@BOT` or `@EOT`. Elapsed days use the UTC civil date of each endpoint. Calendar shifts preserve the time of day of a datetime instant. [Temporal design decisions](../development/DECISIONS.md#d007-meaning-of-imprecise-time)

## Implementation and verification

1. **Deliver the missing-date check end to end in Rust.** Save the definition
   as ordinary posits, reopen the store, and check it by name and by revision
   reference from a new script. Exercise an empty-store check, an invalid
   first save, concurrent first saves, and reuse of a predeclared class.
   A report with the same name and its own source should appear in a general
   name search. The constraint lookup should select the class member. Verify
   that the exact multiline source and the revision reference survive
   reopening. Save twice by one unambiguous name and verify the same policy
   Thing. Test a scheduled revision, a late backdated append, a cutoff
   before the first revision, equal and incomparable maxima, and two policy
   Things sharing a name. Check a superseded revision by its reference.
   Add opposing assertions about one revision and check it on
   demand. Reuse the Traqula parser, typed planner, correlated anti-join,
   `ReadView`, and shared execution budgets. Test the undated order before
   and after its load is declared complete. Both an empty register and a
   register where every order has a date should yield zero findings. Also
   test a hidden fallback mutation,
   truncated output, and a writer adding the missing date mid-check. A
   subsequent check should see the newly published date.
2. **Expose temporal relations and calendar expressions.** Extend the typed
   predicates in `src/traqula/query.rs` and the grammar in `src/traqula.pest`.
   Reuse the interval operations on `Time` in `src/datatype.rs` for both
   operand sources. Test adjacent intervals, mixed precision, leap days,
   negative differences, overflow, and the day 2, 30 and 31 boundaries.
   Keep computed intervals as query values. Exercise the same predicates
   through the planner and executor.
3. **Check the delegated exception from stored evidence.** Join cancellations,
   the applicable policy, country, reviewer delegation, and decisions in one
   scoped evaluation. Test the table above, withdrawals, and rechecking after
   backdated evidence. Expose the shared Rust result through the client
   interfaces and version its contract when it ships.
4. **Measure before extending the language further.** Compare a set-based
   plan with a reference evaluation across many orders and sparse exceptions.
   Inspect `explain search` plans, work units and time to complete. Share
   the scope manifest across findings and use indexed joins and anti-joins
   across the population. Add features such as distinct counts or finite
   dependency traversal when a worked constraint demonstrates the need. The
   order policies require only finite queries and calendar operations. The
   evaluation contract can accommodate another runtime if a future rule needs it.


The release-mode sparse-approval benchmark compares the saved check against a
Rust scan of the generated case inputs and asserts equality of the complete
finding sets. On the development machine the indexed check found 75 of 1,000
orders in about 4 ms and 300 of 4,000 in about 19 ms. It consumed 54,261 and
232,998 logical work units respectively. The reference scan starts with decoded
days and approvals in memory. The check reads posits, evaluates the anti-join
and builds a report with its witnesses and input manifest. These measurements
cover the recorded-history check in the benchmark. [Reproducible fixture](../../benches/constraints.rs)
