use std::{error::Error, fmt};

#[derive(Debug)]
pub enum IntegrationError {
    Configuration(String),
    Validation(String),
    Storage(anyhow::Error),
}

pub type IntegrationResult<T> = Result<T, IntegrationError>;

impl fmt::Display for IntegrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => {
                write!(formatter, "integration is not configured: {message}")
            }
            Self::Validation(message) => {
                write!(formatter, "integration validation failed: {message}")
            }
            Self::Storage(error) => write!(formatter, "integration storage error: {error:#}"),
        }
    }
}

impl Error for IntegrationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Configuration(_) | Self::Validation(_) => None,
            Self::Storage(error) => Some(error.as_ref()),
        }
    }
}

impl IntegrationError {
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration(message.into())
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

impl From<anyhow::Error> for IntegrationError {
    fn from(error: anyhow::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<libsql::Error> for IntegrationError {
    fn from(error: libsql::Error) -> Self {
        Self::Storage(error.into())
    }
}
