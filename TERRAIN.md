# Role Terrain prototype

Role Terrain is an isopleth-inspired structural view of a Positorium database.
The first prototype lives in the Terrain tab of `positorium.html` and uses
clearly labelled mock data. It does not inspect the live database yet.

## Role-space semantics

The map places Roles as points. An isopleth encloses a combination of Role
points and is labelled with the number of distinct Things that appear in every
enclosed Role. Its area does not encode population size.

For a selected scope and displayed Role universe `U`, let the projected profile
of Thing `t` be:

```text
profile_U(t) = { r in U | t appears in Role r }
```

For an enclosed Role set `C`, its extent and support are:

```text
extent_U(C)  = { t | C is a subset of profile_U(t) }
support_U(C) = |extent_U(C)|
```

Adding an enclosed Role cannot increase support. Geometry therefore grows as
the Role combination grows while the labelled support stays equal or decreases.
The mock example renders these deductions:

```text
support({name, hair color}) = 5800
support({name, hair color, height, SSN}) = 5000
support({name, hair color, height, SSN, beard color}) = 1450
support({name, hair color, RFID}) = 750
```

Equal individual Role counts are insufficient to share an isopleth: the Things
must be the same population. The backend supplies semantic Role combinations
and support; the browser owns Role positions, paths, label placement, and color.

Client geometry must preserve the set order. If enclosed Role set `C` is a
strict subset of `D`, the complete line for `C` must lie inside the line for `D`
and the two lines must not cross. Lines may cross only for incomparable Role
sets, such as `{name, hair color, RFID}` and
`{name, hair color, height, social security number}`. Layout validation samples
the complete paths, not only the Role points they contain.

Role text must maintain a visible clearance from every isopleth. Support labels
are mounted at fractions of their own SVG paths, using a small canvas-colored
knockout so the number visibly belongs to the interrupted line. Relationship
allocation anchors are also derived from fractions of their target isopleth
paths, giving zero geometric distance between edge and line. Disjoint
non-crossing lines maintain a minimum client-layout gap. The prototype uses a
plain canvas without a grid or Role-label background shapes.

## Exact projected profiles

An isopleth reports an intersection and does not ordinarily prove the absence of
Roles outside it. Absence is deducible in the mock because `projection.complete`
is true and the response supplies mutually exclusive profiles relative to `U`.

Each profile supplies:

- `id`: stable key used by the client layout.
- `present_roles`: Roles present in the projected profile.
- `absent_roles`: Roles proven absent within the projection.
- `things`: distinct Things having that exact projected profile.

For example, the four profiles under the 5800 isopleth partition its support:

```text
50 + 3550 + 1450 + 750 = 5800
```

If the projection is incomplete, or if profiles or isopleths are omitted by a
support threshold, missing geometry means “not shown”, never zero.

## Relationship semantics

A relationship overlay represents an exact multi-role appearance-set signature,
not an overlap between identity populations. Aggregate `role_totals` report:

- `distinct_things`: distinct Things appearing in that Role in matching sets.
- `participations`: appearances in that Role across matching sets.

Each line in the diagram is an `allocation` from one relationship Role to one
exact projected profile and its anchoring `isopleth_id`. Allocations are
disjoint, so their counts may be added. Hiding an isopleth by support threshold
also hides allocations anchored to it.

Relationship signatures render in a reserved side rail rather than inside the
isopleth field. Each allocation is a separate row and edge: the edge begins at a
path-derived anchor on its target isopleth and ends at its own rail port. The
rail has an explicit visual gap from every isopleth and remains horizontally
pannable with the map on narrow screens.
The mock `{owner, pet}` relationship states:

```text
owner -> beard profile:       100 unique, 170 participations
owner -> four-Role profile:   400 unique, 430 participations
pet   -> RFID profile:        600 unique, 600 participations
```

Consequently there are `500` unique owners but `600` owner participations, while
all `600` pets participate once. The signature reports `600` distinct appearance
sets. Recorded posits remain a separate history measure.

## Mock contract

`TERRAIN_MOCK_DATA` in `positorium-terrain.js` is the proposed semantic response
shape:

```json
{
  "schema_version": 2,
  "source": "mock",
  "database": {
    "things": 12480,
    "roles": 10,
    "appearance_sets": 9730,
    "posits": 38205
  },
  "frames": {
    "history": {
      "label": "All recorded history",
      "projection": {"complete": true, "roles": []},
      "profiles": [],
      "isopleths": [],
      "relationship": {}
    },
    "snapshot": {
      "label": "Maximal values as of now",
      "projection": {"complete": true, "roles": []},
      "profiles": [],
      "isopleths": [],
      "relationship": {}
    }
  }
}
```

The production response should use stable Role identities alongside display
names, allow multiple relationship signatures, and paginate or threshold large
result sets. It must be captured under the database execution owner so counts
come from one coherent state.

## Prototype interactions

- Query/Results and Terrain are alternate workspaces; Terrain uses the complete
  content area rather than a result tab.
- `As of now` is the default frame; History remains available as an explicit
  switch for recorded-history structure.
- Minimum support hides low-support isopleths without implying zero.
- Relationships can be hidden independently.
- Selecting a Role, isopleth, relationship, or allocation opens its measurements.
- Prepare query generates Traqula from semantic Roles rather than stored source.
  It switches back to the Query workspace with the editor focused.

## Backend handoff

A future Rust implementation should provide:

1. Database totals and per-execution mutation effects.
2. Distinct Thing incidence by Role for an explicit temporal/assertion scope.
3. A declared Role universe, exact projected profiles, and support isopleths,
  computed with bounded arity and minimum support.
4. Exact multi-role appearance-set signatures with aggregate Role totals and
  disjoint allocations to projected profiles.
5. A versioned HTTP, SSE, and WASM representation of the same semantic payload.

The frontend should then replace `TERRAIN_MOCK_DATA` with that response. Layout
remains a client concern and may initially use deterministic templates before a
stable automatic contour layout is introduced.
