//! Private PyO3 extension surface used by the public `positorium` Python package.
//!
//! Traqula and the versioned report/result contracts remain the language boundary.
//! The pure-Python facade owns Python conveniences and converts these JSON
//! envelopes into immutable Python value objects.

use crate::construct::{Database, PersistenceMode};
use crate::datatype::Time;
use crate::error::DatabaseError;
use crate::terrain::{
    DEFAULT_MAX_RELATIONSHIP_SIGNATURES, DEFAULT_PROJECTED_ROLE_LIMIT, TERRAIN_VERSION,
    TerrainOptions,
};
use crate::traqula::{
    CollectedResultSet, Engine, ExecutionOptions, ExecutionParameter, ExecutionWarning,
    TRAQULA_VERSION, parse_time_with_now,
};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

pub const PYTHON_INTERFACE_VERSION: u16 = 1;

create_exception!(_native, PositoriumError, PyException);
create_exception!(_native, ConfigurationError, PositoriumError);
create_exception!(_native, PersistenceError, PositoriumError);
create_exception!(_native, DataCorruptionError, PositoriumError);
create_exception!(_native, ParseError, PositoriumError);
create_exception!(_native, ExecutionError, PositoriumError);
create_exception!(_native, CancelledError, PositoriumError);
create_exception!(_native, QueryTimeoutError, PositoriumError);
create_exception!(_native, ResourceLimitError, PositoriumError);
create_exception!(_native, InvalidQueryError, PositoriumError);
create_exception!(_native, InternalError, PositoriumError);
create_exception!(_native, ClosedError, PositoriumError);

fn python_error(error: DatabaseError) -> PyErr {
    let display = error.to_string();
    match error {
        DatabaseError::Config(_) => ConfigurationError::new_err(display),
        DatabaseError::Persistence(_) => PersistenceError::new_err(display),
        DatabaseError::DataCorruption { .. } => DataCorruptionError::new_err(display),
        DatabaseError::Parse { message, line, col } => {
            let location = match (line, col) {
                (Some(line), Some(col)) => format!(" at line {line}, column {col}"),
                (Some(line), None) => format!(" at line {line}"),
                _ => String::new(),
            };
            ParseError::new_err(format!("{message}{location}"))
        }
        DatabaseError::Execution(_) => ExecutionError::new_err(display),
        DatabaseError::Cancelled => CancelledError::new_err(display),
        DatabaseError::Timeout => QueryTimeoutError::new_err(display),
        DatabaseError::ResourceLimit { .. } => ResourceLimitError::new_err(display),
        DatabaseError::UnknownRole(_)
        | DatabaseError::InvalidAppearanceSet(_)
        | DatabaseError::UnknownVariable(_)
        | DatabaseError::VariableDomain { .. }
        | DatabaseError::InvalidRecall(_)
        | DatabaseError::Comparison(_)
        | DatabaseError::Parameter(_) => InvalidQueryError::new_err(display),
        DatabaseError::Invariant(_) | DatabaseError::Lock(_) => InternalError::new_err(display),
    }
}

fn positive_duration(value: Option<f64>, name: &str) -> PyResult<Option<Duration>> {
    value
        .map(|seconds| {
            if !seconds.is_finite() || seconds <= 0.0 {
                return Err(PyValueError::new_err(format!(
                    "{name} must be a finite positive number of seconds"
                )));
            }
            Duration::try_from_secs_f64(seconds).map_err(|_| {
                PyValueError::new_err(format!("{name} is outside the supported duration range"))
            })
        })
        .transpose()
}

fn resolve_time_token(token: Option<&str>, default_now: &Time, name: &str) -> PyResult<Time> {
    let token = token.unwrap_or("@NOW");
    parse_time_with_now(token, default_now)
        .ok_or_else(|| PyValueError::new_err(format!("invalid {name} time token '{token}'")))
}

fn execution_parameters(
    parameters: Option<HashMap<String, (String, String)>>,
) -> PyResult<HashMap<String, ExecutionParameter>> {
    parameters
        .unwrap_or_default()
        .into_iter()
        .map(|(name, (kind, text))| {
            let parameter = match kind.as_str() {
                "literal" => ExecutionParameter::Literal { text },
                "time" => ExecutionParameter::Time { text },
                _ => {
                    return Err(PyValueError::new_err(format!(
                        "parameter '{name}' has unsupported kind '{kind}'"
                    )));
                }
            };
            Ok((name, parameter))
        })
        .collect()
}

#[derive(serde::Serialize)]
struct PythonQueryResponse {
    python_interface_version: u16,
    traqula_version: u16,
    resolved_now: String,
    warnings: Vec<ExecutionWarning>,
    result_sets: Vec<CollectedResultSet>,
}

/// Native database owner. Python users receive the facade in `positorium._api`.
#[pyclass(module = "positorium._native", name = "_Database")]
struct PyDatabase {
    database: Option<Arc<Database>>,
}

impl PyDatabase {
    fn database(&self) -> PyResult<Arc<Database>> {
        self.database
            .as_ref()
            .cloned()
            .ok_or_else(|| ClosedError::new_err("database is closed"))
    }
}

#[pymethods]
impl PyDatabase {
    #[staticmethod]
    fn memory() -> PyResult<Self> {
        let database = Database::new(PersistenceMode::InMemory).map_err(python_error)?;
        Ok(Self {
            database: Some(Arc::new(database)),
        })
    }

    #[staticmethod]
    fn open(path: String) -> PyResult<Self> {
        let database = Database::new(PersistenceMode::File(path)).map_err(python_error)?;
        Ok(Self {
            database: Some(Arc::new(database)),
        })
    }

    #[pyo3(signature = (script, parameters=None, now=None, timeout=None, max_rows=None))]
    fn execute_json(
        &self,
        py: Python<'_>,
        script: String,
        parameters: Option<HashMap<String, (String, String)>>,
        now: Option<String>,
        timeout: Option<f64>,
        max_rows: Option<usize>,
    ) -> PyResult<String> {
        if max_rows == Some(0) {
            return Err(PyValueError::new_err("max_rows must be positive"));
        }
        let database = self.database()?;
        let clock = Time::new();
        let resolved_now = resolve_time_token(now.as_deref(), &clock, "now")?;
        let options = ExecutionOptions {
            now: Some(resolved_now.clone()),
            timeout: positive_duration(timeout, "timeout")?,
            max_rows_per_search: max_rows,
            parameters: execution_parameters(parameters)?,
            ..ExecutionOptions::default()
        };

        let result_sets = py
            .detach(move || {
                Engine::new(&database).execute_collect_multi_with_options(&script, options)
            })
            .map_err(python_error)?;
        let warnings = result_sets
            .first()
            .map(|result| result.metadata.warnings.clone())
            .unwrap_or_default();
        serde_json::to_string(&PythonQueryResponse {
            python_interface_version: PYTHON_INTERFACE_VERSION,
            traqula_version: TRAQULA_VERSION,
            resolved_now: resolved_now.to_string(),
            warnings,
            result_sets,
        })
        .map_err(|error| InternalError::new_err(format!("cannot serialize query result: {error}")))
    }

    #[pyo3(signature = (as_of=None, timeout=5.0, projected_role_limit=None, max_relationship_signatures=None))]
    fn terrain_json(
        &self,
        py: Python<'_>,
        as_of: Option<String>,
        timeout: Option<f64>,
        projected_role_limit: Option<usize>,
        max_relationship_signatures: Option<usize>,
    ) -> PyResult<String> {
        let database = self.database()?;
        let clock = Time::new();
        let as_of = resolve_time_token(as_of.as_deref(), &clock, "as_of")?;
        let options = TerrainOptions {
            as_of: Some(as_of),
            timeout: positive_duration(timeout, "timeout")?,
            projected_role_limit: projected_role_limit.unwrap_or(DEFAULT_PROJECTED_ROLE_LIMIT),
            max_relationship_signatures: max_relationship_signatures
                .unwrap_or(DEFAULT_MAX_RELATIONSHIP_SIGNATURES),
            ..TerrainOptions::default()
        };
        let report = py
            .detach(move || database.terrain_with_options(options))
            .map_err(python_error)?;
        serde_json::to_string(&report)
            .map_err(|error| InternalError::new_err(format!("cannot serialize Terrain: {error}")))
    }

    fn close(&mut self) {
        self.database.take();
    }

    #[getter]
    fn closed(&self) -> bool {
        self.database.is_none()
    }

    fn __repr__(&self) -> &'static str {
        if self.database.is_some() {
            "<positorium.Database open>"
        } else {
            "<positorium.Database closed>"
        }
    }
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = module.py();
    module.add_class::<PyDatabase>()?;
    module.add("PositoriumError", py.get_type::<PositoriumError>())?;
    module.add("ConfigurationError", py.get_type::<ConfigurationError>())?;
    module.add("PersistenceError", py.get_type::<PersistenceError>())?;
    module.add("DataCorruptionError", py.get_type::<DataCorruptionError>())?;
    module.add("ParseError", py.get_type::<ParseError>())?;
    module.add("ExecutionError", py.get_type::<ExecutionError>())?;
    module.add("CancelledError", py.get_type::<CancelledError>())?;
    module.add("QueryTimeoutError", py.get_type::<QueryTimeoutError>())?;
    module.add("ResourceLimitError", py.get_type::<ResourceLimitError>())?;
    module.add("InvalidQueryError", py.get_type::<InvalidQueryError>())?;
    module.add("InternalError", py.get_type::<InternalError>())?;
    module.add("ClosedError", py.get_type::<ClosedError>())?;
    module.add("PYTHON_INTERFACE_VERSION", PYTHON_INTERFACE_VERSION)?;
    module.add("TRAQULA_VERSION", TRAQULA_VERSION)?;
    module.add("TERRAIN_VERSION", TERRAIN_VERSION)?;
    module.add("RUST_PACKAGE_VERSION", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
