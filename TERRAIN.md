# Role Terrain

Role Terrain is an isopleth-inspired structural view of actual Positorium query
results. The Terrain tab in `positorium.html` does not contain a seeded dataset:
it recognizes a history/current pair of incidence result sets and derives the
map, statistics, profiles, support contours, and relationship allocations from
their typed cells.

## Try the supplied terrain

1. Open Query Studio with **Local WASM** enabled and start with a fresh page load.
2. Paste the complete contents of [`traqula/terrain.traqula`](traqula/terrain.traqula)
   into the Query editor.
3. Run the script, including both searches at the end.
4. Open **Terrain**.

The source badge should read **Query data**. The fixture is intentionally small
enough to audit by eye, but includes repeated values and relationship history.
The expected measurements are:

| Measurement | History | As of now |
| --- | ---: | ---: |
| Result rows | 30 | 27 |
| Things | 6 | 6 |
| Roles | 8 | 8 |
| Appearance sets | 24 | 24 |
| Posits | 26 | 24 |

The four projected attribute profiles produce support labels of `6`, `3`, `2`,
and `1`:

- all six Things have `name` and `hair color`;
- Ada, Ben, and Cass also have `height` and `social security number`;
- Mochi and Pixel instead have `RFID`;
- Cass alone also has `beard color`.

The `{owner, pet}` relationship has three appearance sets. It has four recorded
posits in History because Cass and Mochi change from `fostered` to `adopted`, and
three posits As of now. Its endpoint allocations are:

- Cass as `owner`: 1 distinct Thing, 2 relationship participations;
- Ada as `owner`: 1 distinct Thing, 1 participation;
- Mochi and Pixel as `pet`: 2 distinct Things, 3 participations.

## Result-set contract

Terrain requires two compatible result sets from the same complete script
execution. One is the recorded-history search and one contains an `as of` clause.
Each result set must project these six logical fields:

```text
posit, appearance set, member Thing, member Role, value, time
```

The fixture uses the non-keyword variable names below:

```traqula
search ?posit_id = [?appearance_set = {(?thing, ?role_name), ...}, ?value, ?time]
return ?posit_id, ?appearance_set, ?thing, ?role_name, ?value, ?time;
```

The client also accepts the compact aliases `p`, `aset`, `r`, `v`, and `t`.
Column order is irrelevant. Typed cell text is used losslessly; display-table
formatting is not scraped.

The history and current searches must both be returned together. This gives the
browser two coherent frames produced under the engine's normal script-execution
boundary. Running an unrelated search does not silently replace an already
loaded Terrain because its column contract does not match.

## Role-space semantics

The map places attribute Roles as points. A Role is treated as an attribute Role
when it occurs in a single-appearance set. Multi-appearance sets are treated as
relationship candidates.

For displayed Role universe `U`, the projected profile of Thing `t` is:

```text
profile_U(t) = { r in U | t appears in Role r }
```

For an enclosed Role set `C`, its extent and support are:

```text
extent_U(C)  = { t | C is a subset of profile_U(t) }
support_U(C) = |extent_U(C)|
```

Each distinct exact profile supplies one isopleth. Its support includes every
profile that is a superset, so adding an enclosed Role cannot increase support.
The map area does not encode population size; the numeric label does.

Terrain currently projects at most eight attribute Roles, ordered by distinct
Thing support and then Role name. The footer states whether this projection is
complete. A missing line means “not shown”, never zero, including when the
minimum-support control hides it.

## Relationship semantics

Terrain groups multi-appearance sets by their exact sorted Role signature and
displays the signature with the most appearance sets. For that signature:

- `appearance_sets` counts matching sets;
- `posits` counts their distinct posit identities in the selected time frame;
- `distinct_things` counts unique endpoint Things for each Role;
- `participations` counts endpoint appearances across matching sets.

Allocations group each relationship endpoint by its exact projected attribute
profile. Their lines therefore connect real endpoint cohorts to the support
isopleth representing that profile. Repeated historical posits for one unchanged
appearance set increase the posit count but do not invent extra participations.

## Client layout and interactions

Semantic counts come from query results; the browser owns deterministic Role
positions, contour paths, labels, and colors. Query/Results and Terrain remain
alternate workspaces.

- **As of now** is the default frame; **History** shows recorded-history structure.
- Minimum support hides low-support isopleths.
- Relationships can be hidden independently.
- Selecting a Role, isopleth, relationship, or allocation opens its measurements.
- **Prepare query** generates Traqula from the selected, query-derived Roles and
  returns to the Query workspace.

The Rust fixture test validates the result counts and typed cells. The Node test
validates the client aggregation independently of SVG layout.
