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
    #[error("Unknown role: {0}")]
    UnknownRole(String),
    #[error("Invalid appearance set: {0}")]
    InvalidAppearanceSet(String),
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
