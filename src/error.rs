use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("Config error: {0}")]
    Config(String),
    #[error("Persistence error: {0}")]
    Persistence(String),
    #[error("Data corruption: {message}")]
    DataCorruption { message: String },
    #[error("Parse error: {message}")]
    Parse {
        message: String,
        line: Option<usize>,
        col: Option<usize>,
    },
    #[error("Execution error: {0}")]
    Execution(String),
    #[error("Execution cancelled")]
    Cancelled,
    #[error("Execution timed out")]
    Timeout,
    #[error("Resource limit exceeded for {resource}: maximum {limit}")]
    ResourceLimit { resource: &'static str, limit: u64 },
    #[error("Unknown role: {0}")]
    UnknownRole(String),
    #[error("Invalid appearance set: {0}")]
    InvalidAppearanceSet(String),
    #[error("Unknown variable: {0}")]
    UnknownVariable(String),
    #[error("Query variable '{name}' has incompatible domains: {first} and {second}")]
    VariableDomain {
        name: String,
        first: &'static str,
        second: &'static str,
    },
    #[error("Invalid query recall: {0}")]
    InvalidRecall(String),
    #[error("Unsupported comparison: {0}")]
    Comparison(String),
    #[error("Invalid parameter: {0}")]
    Parameter(String),
    #[error("Internal invariant violated: {0}")]
    Invariant(String),
    #[error("Lock poisoned: {0}")]
    Lock(String),
}

pub type Result<T> = std::result::Result<T, DatabaseError>;

#[cfg(feature = "persistence")]
impl DatabaseError {
    pub(crate) fn from_io(error: std::io::Error) -> Self {
        Self::Persistence(error.to_string())
    }
}
