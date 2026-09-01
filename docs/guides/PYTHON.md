# Positorium for Python

The `positorium` package embeds the Rust database directly in CPython 3.9 or
newer. It does not start or connect to the HTTP service. Scripts execute against
an in-memory database or an append-only store owned by the Python process.

## Install

The Python package is part of the beta.2 source tree. Until beta.2 appears on
PyPI, install it from a source checkout with Rust available:

```sh
python -m pip install .
```

After beta.2 is published, allow prereleases when installing the newest
release:

```sh
python -m pip install --pre positorium
```

The release workflow builds stable-ABI wheels for Linux, macOS, and Windows.
Installing from a source checkout or source distribution requires a Rust
toolchain.

## In-memory and persistent databases

Use a context manager so persistent store ownership is released
deterministically:

```python
import positorium

with positorium.Database.memory() as database:
    database.execute(
        'add role name; add posit [{(+person, name)}, "Ada", @NOW];'
    )

with positorium.Database.open("evidence.store") as database:
    result = database.execute_one(
        'search [{(?person, name)}, ?name, *] return ?person, ?name;'
    )
```

`Database.memory()` is ephemeral. `Database.open(path)` opens or creates the
same append-only store format used by the native server. Only one process may
own a store. A closed database raises `ClosedError` if used again.

## Results

`Database.execute()` runs one complete Traqula script and returns an
`ExecutionResult`. It is an ordered sequence because one script may contain
several searches. Mutation-only scripts return an empty sequence.

```python
result = database.execute(
    'search [{(?person, name)}, ?name, *] return ?person, ?name;'
)

print(result.traqula_version)
print(result.resolved_now)

result_set = result[0]
for row in result_set:
    print(row["person"].kind, row["person"].text)
    print(row["name"].kind, row["name"].text)
```

`Database.execute_one()` is a convenience that requires exactly one returned
search. A `ResultSet` exposes `columns`, `rows`, `row_count`, `limited`, and the
source `search`. Rows can be indexed by position or projected column name.

Every `Cell` contains a stable `kind` and exact `text`. Positorium deliberately
does not turn `+0010.00` into a Python float or normalize JSON, strings, or time
precision. Use `result_set.to_dicts(text=True)` when only the exact strings are
needed.

Pandas remains optional:

```sh
python -m pip install "positorium[pandas]"
```

```python
frame = result_set.to_pandas()
```

The DataFrame contains exact cell text. No lossy type inference is performed by
the binding.

## Typed parameters

Parameters separate values from Traqula source. Values must be explicit
`Literal` or `Time` instances:

```python
from datetime import date
from decimal import Decimal
import positorium

result = database.execute_one(
    """
    search [{(?person, name)}, $name, *],
           [{(?person, score)}, ?score, *] as of $cutoff
    where ?score = $score
    return ?person, ?score;
    """,
    parameters={
        "name": positorium.literal("Ada"),
        "score": positorium.literal(Decimal("10.00")),
        "cutoff": positorium.time(date(2025, 1, 1)),
    },
)
```

Convenience constructors include:

- `literal("Ada")`, integers, `Decimal`, and mappings;
- `Literal.certainty(80)` for `80%`;
- `time(...)` for ISO strings, dates, and timezone-aware datetimes; and
- `raw_literal("+0010.00")` when exact token spelling is intentional.

Naive datetimes are rejected. A timezone-aware datetime is converted to UTC
before its Traqula token is created. Standalone Python booleans and floats are
also rejected because Traqula has no standalone boolean token and binary floats
cannot promise lexical decimal fidelity.

## Execution controls

```python
result = database.execute(
    script,
    now=positorium.time("2025-01-01"),
    timeout=2.0,
    max_rows=10_000,
)
```

`now` deterministically replaces every `@NOW` in the complete script. `timeout`
is a positive number of seconds, and `max_rows` applies independently to every
search. A lower Traqula `limit` still wins. Long-running native execution
releases the Python interpreter lock; Positorium itself serializes scripts for
one database owner.

## Terrain

`Database.terrain()` returns the authoritative versioned Terrain report as
nested Python dictionaries and lists:

```python
report = database.terrain(
    as_of="2025-01-01",
    timeout=5.0,
    projected_role_limit=8,
    max_relationship_signatures=16,
)
print(report["terrain_version"], report["database"]["posits"])
```

## Exceptions and versions

All database exceptions derive from `PositoriumError`. Useful subclasses include
`ParseError`, `InvalidQueryError`, `PersistenceError`, `DataCorruptionError`,
`QueryTimeoutError`, `ResourceLimitError`, and `ClosedError`.

The package exports `PYTHON_INTERFACE_VERSION`, `TRAQULA_VERSION`, and
`TERRAIN_VERSION`. Python interface 1 is documented in
[Contracts](../reference/CONTRACTS.md). The distribution version follows PEP
440, so Cargo `0.1.4-beta.2` is installed as Python `0.1.4b2`.

## Publishing a release

The Python workflow builds and tests stable-ABI wheels and a source distribution
on ordinary pushes. A version-matching `vX.Y.Z...` tag also publishes the merged
artifacts through PyPI Trusted Publishing. Before the first release, configure a
PyPI trusted publisher for repository `Roenbaeck/positorium`, workflow
`python.yml`, and GitHub environment `pypi`. No long-lived PyPI token is stored
in the repository.
