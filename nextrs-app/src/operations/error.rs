use std::{error::Error, fmt};

#[derive(Debug)]
pub enum OperationError {
    Validation(String),
    ConnectionNotFound(String),
    ReadOnly,
    SchedulerDisabled,
    Coordination(String),
    Storage(anyhow::Error),
}

pub type OperationResult<T> = Result<T, OperationError>;

impl OperationError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    pub fn coordination(message: impl Into<String>) -> Self {
        Self::Coordination(message.into())
    }
}

impl fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => write!(formatter, "validation failed: {message}"),
            Self::ConnectionNotFound(slug) => {
                write!(formatter, "integration connection {slug:?} was not found")
            }
            Self::ReadOnly => formatter.write_str("operations are in read-only mode"),
            Self::SchedulerDisabled => formatter.write_str("the scheduler is disabled"),
            Self::Coordination(message) => {
                write!(formatter, "operation coordination failed: {message}")
            }
            Self::Storage(error) => write!(formatter, "operation storage error: {error:#}"),
        }
    }
}

impl Error for OperationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for OperationError {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<libsql::Error> for OperationError {
    fn from(error: libsql::Error) -> Self {
        Self::Storage(error.into())
    }
}
