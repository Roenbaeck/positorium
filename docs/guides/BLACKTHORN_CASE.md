# The Blackthorn Ruby detective showcase

The Blackthorn Ruby is a complete, executable detective story written in
Traqula 1. It is designed for a live Query Studio demonstration as well as for
reading line by line. The case contains conflicting testimony, independent
device evidence, two explicit retractions, a source-dependent classification,
and an evidence trail that can be solved with ordinary joins.

The source is
[`traqula/blackthorn.traqula`](../../traqula/blackthorn.traqula). It is heavily
commented and intentionally long: the comments are the narration, and each of
its 15 searches becomes a selectable Query Studio result set.

## Premise

At 20:42 on 8 November 2025, Bellwether Museum loses power for ninety seconds.
When the lights return, the Blackthorn Ruby is missing. A low-light camera saw
a figure in clock conservator Elias Reed's red scarf. Head guard Jonah Pike
says he remained at the security desk, and two people initially support the
story.

We play Detective Mara Voss. Instead of replacing a statement when somebody
changes their mind, we preserve the target claim and append a new assertion.
That lets us reconstruct both what was said at 22:00 and what remained in
effect after the second interviews.

The cast is small enough to follow during a demonstration:

| Person | Position | Initial significance |
| --- | --- | --- |
| Mara Voss | Detective | Our explicit evidence policy and final conclusion |
| Jonah Pike | Head of security | Controls the master card and claims he never left his desk |
| Elias Reed | Clock conservator | Owns the red scarf seen by the camera |
| Beatrice Shaw | Assistant archivist | Initially supports her supervisor Jonah's alibi |
| Lydia Marr | Museum patron | Initially says she saw Elias in the gallery |
| Dr Celeste Vale | Curator | Discovers and reports the missing ruby |

## Run it in Query Studio

The quickest zero-install demonstration uses the
[hosted Query Studio](https://roenbaeck.github.io/positorium/):

1. Reload the page if you want a fresh browser database.
2. Open **Studio settings** and enable **Local WASM**.
3. Select **Load detective case** in the editor toolbar.
4. Read the opening comment, then select **Run query**.
5. Use the result-set selector above the table to move through the
   investigation.
6. Open **Terrain** and select **Refresh** after the script finishes.

Loading the example replaces only the editor contents and asks before
overwriting a non-empty workspace. Running remains a separate, explicit action.
The browser database is in memory and is discarded when the page is reloaded.

Against a native server, leave Local WASM disabled and run the same script. The
repository checkout includes `traqula/blackthorn.traqula`, and release archives
built from this revision will include it as well. It can be selected as
`traqula_file_to_run_on_startup` in `positorium.json` when only the populated
database and Terrain are needed. Entity resolution makes the script idempotent,
so rerunning it does not duplicate the case.

Use a dedicated or empty store for the cleanest Terrain counts. The searches
identify the case by its dossier code and still work in a mixed store, but
Terrain intentionally visualizes the complete database snapshot.

## What the script demonstrates

| Traqula or UI feature | Where it appears |
| --- | --- |
| `search ... or add posit` | Every durable case, person, source, place, object, and class identity |
| Atomic `and assert` | Testimony, device observations, evidence, classifications, and the conclusion |
| Assertion provenance | Human, camera, access-control, laboratory, registry, and detective sources |
| 0% retractions | Beatrice withdraws Jonah's alibi; Lydia withdraws her accusation of Elias |
| `in effect T, t` | The 22:00 and 23:20 accounts, current classifications, and effective evidence joins |
| `via` | Source, certainty, and assertion time shown beside the target claim |
| Raw `as of` | Equal-time alternatives remain visible independently of assertion state |
| Natural joins | Master card to cardholder to door; print size to shoe size; fibre to scarf to borrower |
| `union` and `distinct` | One directory of human and institutional sources |
| Safe `not exists` | Candidates lacking both the observed shoe size and an assigned card |
| `class`, `thing`, `subclass` | Five direct classes and an explicit two-edge class hierarchy |
| Multiple result sets | Fifteen narrated investigative steps in one script |
| Terrain | Complete eight-Role projection, twelve relationship signatures, History/Current contrast, and class overlays |

None of these operations silently chooses the truth. Certainty remains a
source's stated certainty. `in effect` resolves effective source-local
assertions, but it does not rank witnesses or combine their percentages. The
final accusation is another posit asserted by Mara at 96%, making the
detective's policy visible and contestable.

## Follow the investigation

The result-set selector is the story's table of contents:

| Set | Rows | Question answered |
| ---: | ---: | --- |
| 1 | 5 | Which opaque class Thing corresponds to each readable class name? |
| 2 | 10 | Who or what can act as an evidence source? |
| 3 | 4 | Who does each current source still call a person of interest? |
| 4 | 5 | What accounts were in effect at 22:00, before the second interviews? |
| 5 | 7 | Which equal-time target alternatives exist in the raw snapshot? |
| 6 | 5 | What remains in effect at 23:20 after the retractions? |
| 7 | 2 | Who changed their account, what did they withdraw, and when? |
| 8 | 1 | Whose assigned master card opened the East Gallery? |
| 9 | 1 | Whose recorded shoe size matches the service-stair print? |
| 10 | 1 | Who carried the scarf whose wool matches the cabinet fibre? |
| 11 | 2 | Which candidates lack both the size-44 and card links? |
| 12 | 1 | Who satisfies the active-class, card, door, and print patterns together? |
| 13 | 2 | Which explicit subclass edges were recorded? |
| 14 | 1 | What conclusion does Mara assert at 90% or greater certainty? |
| 15 | 3 | How did the case status move from open to closed? |

The crucial comparison is result set 4 against result set 6. At 22:00,
Beatrice supports Jonah and Lydia accuses Elias. At 23:20, those two particular
source/target pairs are gone. Jonah's own alibi remains because Beatrice can
retract only her assertion, not his. The raw target snapshot in result set 5
still contains the withdrawn statements because `as of` answers an
appearance-time question, not an assertion-policy question.

The solution does not depend on adding percentages. Three structural paths
converge independently:

```text
Master Card JP-1 ──assigned to──> Jonah Pike
        │
        └──opened──> East Gallery at 20:41:32

Service Stair Print 7 ──size 44──> Jonah Pike's dossier

Cabinet Fibre ──same material──> Red Scarf ──borrowed by──> Jonah Pike
```

Result set 12 intersects the first two paths and returns only Jonah. Result set
10 supplies the attempted framing mechanism. Result set 14 then shows Mara's
explicit conclusion and its provenance.

## Show it in Terrain

In an otherwise empty database, the case produces a complete projection of
exactly eight singleton attribute Roles:

```text
dossier_id · name · occupation · class · description · source type
shoe size · status
```

This creates readable profiles for people, suspects, classes, evidence,
sources, places, and the case itself. The projection is complete rather than
truncated, so absent Role bits really are absent from this dataset.

The relationship selector exposes twelve exact signatures. Particularly useful
ones for a demonstration are:

- `{posit, ascertains}` for the assertion graph;
- `{thing, class}` for direct classifications;
- `{class, subclass}` for the explicit hierarchy;
- `{suspect, case}` for competing accounts;
- `{credential, holder}` and `{credential, access point}` for the card trail;
- `{trace, location}` for the shoeprint; and
- `{garment, borrower}` for the scarf.

Switch between **History** and **Current**. In the isolated fixture, History has
171 posits while Current retains 165 at a cutoff after the story. The difference
comes from transitional values and assertion histories; Current does not erase
History.

### Class overlay demonstration

The class selector displays opaque Thing identities by design. Result set 1
maps those identities to class names, and result set 2 maps source identities
to names.

1. In Terrain's Current scope, select the identity for **Person of interest**.
2. Keep the exact value at `"active"` and certainty at **Positive support**.
3. With **All effective sources**, three people are shaded because the police
   registry still retains all three.
4. Select **Bellwether Police Registry** as the source: three remain.
5. Select **Detective Mara Voss** as the source: only Jonah remains, because
   Mara retracted her active evidence for Elias and Beatrice.

This is the point of the overlay controls: the same stored database can present
different, fully disclosed source policies without pretending that one
classification value is built-in truth. Try the **Physical evidence** and
**Witness** class identities as well to see different Role profiles shaded.

## Suggested exercises

- Add a second laboratory assertion with a different certainty and confirm
  that both sources remain visible.
- Give the footprint a simultaneous, incompatible size and inspect the raw and
  effective views before deciding how a detective policy should respond.
- Retract Jonah's own alibi and confirm that Beatrice's earlier 0% assertion
  could not have done that for him.
- Add a `Charged person` class instead of treating the case status text as a
  classification.
- Add a subclass below `Physical evidence`, then query the explicit hierarchy.
  Notice that the class overlay still performs direct membership only.
- Change a query from `in effect` to `as of` and explain exactly which question
  changed.

For the formal semantics behind the example, continue with the
[Traqula reference](../reference/TRAQULA.md), the
[Terrain reference](../reference/TERRAIN.md), and the assertion and
classification recipes in the [cookbook](COOKBOOK.md).
