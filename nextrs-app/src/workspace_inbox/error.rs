use std::{error::Error, fmt};

#[derive(Debug)]
pub enum WorkspaceInboxError {
    Validation(String),
    NotFound(String),
    Conflict(String),
    Storage(anyhow::Error),
}

pub type WorkspaceInboxResult<T> = Result<T, WorkspaceInboxError>;

impl WorkspaceInboxError {
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }
}

impl fmt::Display for WorkspaceInboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(message) => write!(formatter, "validation failed: {message}"),
            Self::NotFound(message) => write!(formatter, "not found: {message}"),
            Self::Conflict(message) => write!(formatter, "conflict: {message}"),
            Self::Storage(error) => write!(formatter, "workspace/inbox storage error: {error:#}"),
        }
    }
}

impl Error for WorkspaceInboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for WorkspaceInboxError {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<libsql::Error> for WorkspaceInboxError {
    fn from(error: libsql::Error) -> Self {
        Self::Storage(error.into())
    }
}
