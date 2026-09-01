"""Pythonic value objects and database facade for the native Positorium engine."""

from __future__ import annotations

import json as _json
import os
import re
from collections.abc import Iterator, Mapping, Sequence
from dataclasses import dataclass
from datetime import date, datetime, timezone
from decimal import Decimal
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union, overload

from . import _native

_DECIMAL_TOKEN = re.compile(r"^[+-]?\d+\.\d+$")


@dataclass(frozen=True)
class Literal:
    """An exact Traqula literal token used for a typed query parameter."""

    text: str

    def __post_init__(self) -> None:
        if not isinstance(self.text, str) or not self.text:
            raise ValueError("literal token must be a non-empty string")

    @classmethod
    def string(cls, value: str) -> "Literal":
        """Encode a Python string as a lossless Traqula string token."""
        if not isinstance(value, str):
            raise TypeError("string literal value must be str")
        return cls(_json.dumps(value, ensure_ascii=False))

    @classmethod
    def integer(cls, value: int) -> "Literal":
        """Encode a Python integer as a Traqula integer token."""
        if isinstance(value, bool) or not isinstance(value, int):
            raise TypeError("integer literal value must be int, not bool")
        return cls(str(value))

    @classmethod
    def decimal(cls, value: Union[str, Decimal]) -> "Literal":
        """Encode a fixed-point decimal while preserving its supplied scale."""
        text = str(value)
        if not _DECIMAL_TOKEN.fullmatch(text):
            raise ValueError("decimal literal must use fixed-point notation, such as 10.00")
        return cls(text)

    @classmethod
    def certainty(cls, percent: int) -> "Literal":
        """Encode a signed certainty percentage in the inclusive range -100..100."""
        if isinstance(percent, bool) or not isinstance(percent, int):
            raise TypeError("certainty must be an integer percentage")
        if not -100 <= percent <= 100:
            raise ValueError("certainty must be between -100 and 100 percent")
        return cls(f"{percent}%")

    @classmethod
    def json_object(cls, value: Mapping[str, Any]) -> "Literal":
        """Encode a mapping as a compact Traqula JSON-object token."""
        if not isinstance(value, Mapping):
            raise TypeError("JSON literal value must be a mapping")
        return cls(
            _json.dumps(
                dict(value),
                ensure_ascii=False,
                separators=(",", ":"),
                allow_nan=False,
            )
        )


@dataclass(frozen=True)
class Time:
    """An exact Traqula time token or time constant used as a parameter."""

    text: str

    def __post_init__(self) -> None:
        if not isinstance(self.text, str) or not self.text:
            raise ValueError("time token must be a non-empty string")

    @classmethod
    def at(cls, value: Union[str, date, datetime]) -> "Time":
        """Create a quoted UTC time token from an ISO string, date, or datetime."""
        if isinstance(value, datetime):
            if value.tzinfo is None or value.utcoffset() is None:
                raise ValueError("datetime values must be timezone-aware")
            utc = value.astimezone(timezone.utc).replace(tzinfo=None)
            text = utc.isoformat(sep=" ", timespec="microseconds").rstrip("0").rstrip(".")
            return cls(f"'{text}'")
        if isinstance(value, date):
            return cls(f"'{value.isoformat()}'")
        if not isinstance(value, str) or not value:
            raise TypeError("time value must be a non-empty string, date, or datetime")
        if value.startswith("@") or (value.startswith("'") and value.endswith("'")):
            return cls(value)
        return cls(f"'{value}'")


Parameter = Union[Literal, Time]


def literal(value: Union[str, int, Decimal, Mapping[str, Any]]) -> Literal:
    """Encode a common Python value as a Traqula literal parameter."""
    if isinstance(value, str):
        return Literal.string(value)
    if isinstance(value, bool):
        raise TypeError("Traqula has no standalone boolean literal; use a JSON object or raw_literal")
    if isinstance(value, int):
        return Literal.integer(value)
    if isinstance(value, Decimal):
        return Literal.decimal(value)
    if isinstance(value, Mapping):
        return Literal.json_object(value)
    raise TypeError(f"unsupported Python literal type: {type(value).__name__}")


def raw_literal(text: str) -> Literal:
    """Use an already encoded Traqula literal token without altering its spelling."""
    return Literal(text)


def time(value: Union[str, date, datetime]) -> Time:
    """Encode an ISO value as a Traqula time parameter."""
    return Time.at(value)


@dataclass(frozen=True)
class Cell:
    """One lossless projected value from Traqula."""

    kind: str
    text: str

    def __str__(self) -> str:
        return self.text


class Row(Sequence[Cell]):
    """An ordered result row addressable by position or projected column name."""

    __slots__ = ("_columns", "_cells")

    def __init__(self, columns: Sequence[str], cells: Sequence[Cell]) -> None:
        self._columns = tuple(columns)
        self._cells = tuple(cells)
        if len(self._columns) != len(self._cells):
            raise ValueError("row cell count does not match its result columns")

    @property
    def columns(self) -> Tuple[str, ...]:
        return self._columns

    @overload
    def __getitem__(self, key: int) -> Cell: ...

    @overload
    def __getitem__(self, key: slice) -> Tuple[Cell, ...]: ...

    @overload
    def __getitem__(self, key: str) -> Cell: ...

    def __getitem__(self, key: Union[int, slice, str]) -> Union[Cell, Tuple[Cell, ...]]:
        if isinstance(key, str):
            try:
                index = self._columns.index(key)
            except ValueError as error:
                raise KeyError(key) from error
            return self._cells[index]
        return self._cells[key]

    def __len__(self) -> int:
        return len(self._cells)

    def __iter__(self) -> Iterator[Cell]:
        return iter(self._cells)

    def as_dict(self, *, text: bool = False) -> Dict[str, Union[Cell, str]]:
        """Return a column mapping, optionally containing only exact cell text."""
        if len(set(self._columns)) != len(self._columns):
            raise ValueError("duplicate projected columns cannot be represented as a dictionary")
        if text:
            return {column: cell.text for column, cell in zip(self._columns, self._cells)}
        return dict(zip(self._columns, self._cells))

    def __repr__(self) -> str:
        return f"Row(columns={self._columns!r}, cells={self._cells!r})"


@dataclass(frozen=True)
class ExecutionWarning:
    code: str
    message: str
    rewrite: Optional[str] = None


class ResultSet(Sequence[Row]):
    """One returned Traqula search, including framing and limit metadata."""

    __slots__ = ("columns", "rows", "row_count", "limited", "search")

    def __init__(
        self,
        *,
        columns: Sequence[str],
        rows: Sequence[Row],
        row_count: int,
        limited: bool,
        search: Optional[str],
    ) -> None:
        self.columns = tuple(columns)
        self.rows = tuple(rows)
        self.row_count = row_count
        self.limited = limited
        self.search = search
        if self.row_count != len(self.rows):
            raise ValueError("reported row count does not match returned rows")

    @overload
    def __getitem__(self, key: int) -> Row: ...

    @overload
    def __getitem__(self, key: slice) -> Tuple[Row, ...]: ...

    def __getitem__(self, key: Union[int, slice]) -> Union[Row, Tuple[Row, ...]]:
        return self.rows[key]

    def __len__(self) -> int:
        return len(self.rows)

    def __iter__(self) -> Iterator[Row]:
        return iter(self.rows)

    def to_dicts(self, *, text: bool = False) -> List[Dict[str, Union[Cell, str]]]:
        """Return rows as dictionaries, preserving Cell objects by default."""
        return [row.as_dict(text=text) for row in self.rows]

    def to_pandas(self) -> Any:
        """Create a pandas DataFrame of exact cell text without requiring pandas at install time."""
        try:
            import pandas as pd
        except ImportError as error:
            raise ImportError(
                "pandas is optional; install it with 'pip install positorium[pandas]'"
            ) from error
        return pd.DataFrame(
            [[cell.text for cell in row] for row in self.rows],
            columns=self.columns,
        )

    def __repr__(self) -> str:
        return (
            f"ResultSet(columns={self.columns!r}, row_count={self.row_count}, "
            f"limited={self.limited})"
        )


class ExecutionResult(Sequence[ResultSet]):
    """All returned searches and metadata from one complete Traqula script."""

    __slots__ = (
        "python_interface_version",
        "traqula_version",
        "resolved_now",
        "warnings",
        "result_sets",
    )

    def __init__(
        self,
        *,
        python_interface_version: int,
        traqula_version: int,
        resolved_now: str,
        warnings: Sequence[ExecutionWarning],
        result_sets: Sequence[ResultSet],
    ) -> None:
        self.python_interface_version = python_interface_version
        self.traqula_version = traqula_version
        self.resolved_now = resolved_now
        self.warnings = tuple(warnings)
        self.result_sets = tuple(result_sets)

    @overload
    def __getitem__(self, key: int) -> ResultSet: ...

    @overload
    def __getitem__(self, key: slice) -> Tuple[ResultSet, ...]: ...

    def __getitem__(self, key: Union[int, slice]) -> Union[ResultSet, Tuple[ResultSet, ...]]:
        return self.result_sets[key]

    def __len__(self) -> int:
        return len(self.result_sets)

    def __iter__(self) -> Iterator[ResultSet]:
        return iter(self.result_sets)


def _execution_result(payload: Mapping[str, Any]) -> ExecutionResult:
    result_sets: List[ResultSet] = []
    for result_payload in payload["result_sets"]:
        columns = tuple(result_payload["columns"])
        rows = [
            Row(
                columns,
                [Cell(kind=cell["kind"], text=cell["text"]) for cell in row],
            )
            for row in result_payload["rows"]
        ]
        result_sets.append(
            ResultSet(
                columns=columns,
                rows=rows,
                row_count=result_payload["row_count"],
                limited=result_payload["limited"],
                search=result_payload.get("search"),
            )
        )
    warnings = [
        ExecutionWarning(
            code=warning["code"],
            message=warning["message"],
            rewrite=warning.get("rewrite"),
        )
        for warning in payload["warnings"]
    ]
    return ExecutionResult(
        python_interface_version=payload["python_interface_version"],
        traqula_version=payload["traqula_version"],
        resolved_now=payload["resolved_now"],
        warnings=warnings,
        result_sets=result_sets,
    )


def _time_text(value: Optional[Union[Time, str, date, datetime]]) -> Optional[str]:
    if value is None:
        return None
    if isinstance(value, Time):
        return value.text
    return Time.at(value).text


class Database:
    """An embedded Positorium database owning an in-memory or persistent Rust engine."""

    __slots__ = ("_native", "_path")

    def __init__(self, native: Any, path: Optional[str]) -> None:
        self._native = native
        self._path = path

    @classmethod
    def memory(cls) -> "Database":
        """Create an ephemeral database whose contents disappear when closed."""
        return cls(_native._Database.memory(), None)

    @classmethod
    def open(cls, path: Union[str, bytes, os.PathLike[str], os.PathLike[bytes]]) -> "Database":
        """Open or create an append-only store directory."""
        path_text = os.fsdecode(os.fspath(path))
        return cls(_native._Database.open(path_text), path_text)

    @property
    def closed(self) -> bool:
        return bool(self._native.closed)

    @property
    def path(self) -> Optional[Path]:
        return None if self._path is None else Path(self._path)

    def close(self) -> None:
        """Release the database and its exclusive persistent-store ownership."""
        self._native.close()

    def execute(
        self,
        script: str,
        *,
        parameters: Optional[Mapping[str, Parameter]] = None,
        now: Optional[Union[Time, str, date, datetime]] = None,
        timeout: Optional[float] = None,
        max_rows: Optional[int] = None,
    ) -> ExecutionResult:
        """Execute a complete Traqula script and return every projected search."""
        if not isinstance(script, str):
            raise TypeError("script must be str")
        encoded: Dict[str, Tuple[str, str]] = {}
        for name, parameter in (parameters or {}).items():
            if not isinstance(name, str):
                raise TypeError("parameter names must be strings")
            if isinstance(parameter, Literal):
                encoded[name] = ("literal", parameter.text)
            elif isinstance(parameter, Time):
                encoded[name] = ("time", parameter.text)
            else:
                raise TypeError(
                    f"parameter '{name}' must be Literal or Time, got {type(parameter).__name__}"
                )
        payload = self._native.execute_json(
            script,
            encoded,
            _time_text(now),
            timeout,
            max_rows,
        )
        return _execution_result(_json.loads(payload))

    def execute_one(self, script: str, **options: Any) -> ResultSet:
        """Execute a script that must return exactly one search result set."""
        result = self.execute(script, **options)
        if len(result) != 1:
            raise ValueError(f"expected exactly one result set, received {len(result)}")
        return result[0]

    def terrain(
        self,
        *,
        as_of: Optional[Union[Time, str, date, datetime]] = None,
        timeout: Optional[float] = 5.0,
        projected_role_limit: Optional[int] = None,
        max_relationship_signatures: Optional[int] = None,
    ) -> Dict[str, Any]:
        """Return the authoritative versioned Terrain report as Python data."""
        payload = self._native.terrain_json(
            _time_text(as_of),
            timeout,
            projected_role_limit,
            max_relationship_signatures,
        )
        return _json.loads(payload)

    def __enter__(self) -> "Database":
        if self.closed:
            raise _native.ClosedError("database is closed")
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> None:
        self.close()

    def __repr__(self) -> str:
        mode = "memory" if self._path is None else repr(self._path)
        state = "closed" if self.closed else "open"
        return f"Database({mode}, {state})"
