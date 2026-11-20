use super::{
    CollectedResultSet, Engine, ExecutionOptions, ExecutionParameter, ResultCell, ResultCellKind,
};
use crate::{Database, DatabaseError, LiteralValue, Result};

impl Database {
    /// Run a Traqula script. Each command with a projection produces a separate
    /// result set, in script order. Mutation-only scripts return an empty vector.
    pub fn execute(&self, script: &str) -> Result<Vec<CollectedResultSet>> {
        Engine::new(self).execute_collect_multi(script)
    }

    /// Run a script with bound parameters, a chosen clock, cancellation or limits.
    pub fn execute_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<Vec<CollectedResultSet>> {
        Engine::new(self).execute_collect_multi_with_options(script, options)
    }

    /// Run a script that returns exactly one result set. The set may contain zero
    /// or more rows. An incorrect number of projections is rejected before any writes.
    pub fn execute_one(&self, script: &str) -> Result<CollectedResultSet> {
        self.execute_one_with_options(script, ExecutionOptions::default())
    }

    /// Run a single-result script with the same options as [`Database::execute_with_options`].
    pub fn execute_one_with_options(
        &self,
        script: &str,
        options: ExecutionOptions,
    ) -> Result<CollectedResultSet> {
        Engine::new(self)
            .collect_result_sets(script, options, true)?
            .pop()
            .ok_or_else(|| DatabaseError::Invariant("missing projected result set".into()))
    }
}

impl CollectedResultSet {
    /// Look up a cell by zero-based row and projected name (without the leading `?`).
    pub fn cell(&self, row: usize, column: &str) -> Option<&ResultCell> {
        let index = self.columns.iter().position(|name| name == column)?;
        self.rows.get(row)?.get(index)
    }

    /// Take the store-qualified `policy` or `revision` from a constraint result.
    /// Pass this parameter to another script on the originating store.
    pub fn reference(&self, row: usize, column: &str) -> Result<ExecutionParameter> {
        let cell = self.cell(row, column).ok_or_else(|| {
            DatabaseError::Parameter(format!("missing identity at row {row}, column '{column}'"))
        })?;
        let store = self
            .cell(row, "store")
            .filter(|cell| cell.kind == ResultCellKind::Literal)
            .and_then(|cell| serde_json::from_str::<String>(cell.as_str()).ok())
            .ok_or_else(|| {
                DatabaseError::Parameter("expected a store column with a quoted store UUID".into())
            })?;
        let text = format!("{store}/{}", cell.text);
        match cell.kind {
            ResultCellKind::Thing => Ok(ExecutionParameter::Thing { text }),
            ResultCellKind::Posit => Ok(ExecutionParameter::Posit { text }),
            _ => Err(DatabaseError::Parameter(format!(
                "'{column}' must contain a Thing or Posit"
            ))),
        }
    }
}

impl ExecutionOptions {
    /// Bind a `$name` parameter. Supply the name without its dollar sign.
    /// Binding a name again replaces its previous value.
    #[must_use]
    pub fn bind(mut self, name: impl Into<String>, value: impl Into<ExecutionParameter>) -> Self {
        self.parameters.insert(name.into(), value.into());
        self
    }
}

impl From<String> for ExecutionParameter {
    /// Encode application text as a quoted Traqula string.
    fn from(value: String) -> Self {
        Self::Literal {
            text: serde_json::Value::String(value).to_string(),
        }
    }
}

impl From<&str> for ExecutionParameter {
    fn from(value: &str) -> Self {
        Self::from(value.to_owned())
    }
}

impl From<i64> for ExecutionParameter {
    fn from(value: i64) -> Self {
        Self::Literal {
            text: value.to_string(),
        }
    }
}

impl From<LiteralValue> for ExecutionParameter {
    /// Carry the complete spelling of a validated literal into a script.
    fn from(value: LiteralValue) -> Self {
        Self::Literal {
            text: value.token().to_owned(),
        }
    }
}
