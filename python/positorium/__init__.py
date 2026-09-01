"""Embedded Python bindings for the Positorium transitional database."""

from importlib.metadata import PackageNotFoundError, version

from ._api import (
    Cell,
    Database,
    ExecutionResult,
    ExecutionWarning,
    Literal,
    Parameter,
    ResultSet,
    Row,
    Time,
    literal,
    raw_literal,
    time,
)
from ._native import (
    PYTHON_INTERFACE_VERSION,
    RUST_PACKAGE_VERSION,
    TERRAIN_VERSION,
    TRAQULA_VERSION,
    CancelledError,
    ClosedError,
    ConfigurationError,
    DataCorruptionError,
    ExecutionError,
    InternalError,
    InvalidQueryError,
    ParseError,
    PersistenceError,
    PositoriumError,
    QueryTimeoutError,
    ResourceLimitError,
)

try:
    __version__ = version("positorium")
except PackageNotFoundError:  # pragma: no cover - only an unpackaged source tree
    __version__ = RUST_PACKAGE_VERSION

__all__ = [
    "PYTHON_INTERFACE_VERSION",
    "RUST_PACKAGE_VERSION",
    "TERRAIN_VERSION",
    "TRAQULA_VERSION",
    "CancelledError",
    "Cell",
    "ClosedError",
    "ConfigurationError",
    "DataCorruptionError",
    "Database",
    "ExecutionError",
    "ExecutionResult",
    "ExecutionWarning",
    "InternalError",
    "InvalidQueryError",
    "Literal",
    "Parameter",
    "ParseError",
    "PersistenceError",
    "PositoriumError",
    "QueryTimeoutError",
    "ResourceLimitError",
    "ResultSet",
    "Row",
    "Time",
    "__version__",
    "literal",
    "raw_literal",
    "time",
]
