use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaError {
    Configuration,
    Disabled,
    InvalidInput,
    InvalidKey,
    InvalidIntent,
    ExpiredIntent,
    ObjectMissing,
    ObjectMismatch,
    StorageUnavailable,
}

pub type MediaResult<T> = Result<T, MediaError>;

impl fmt::Display for MediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Configuration => "object storage configuration is invalid",
            Self::Disabled => "object storage is not configured",
            Self::InvalidInput => "media request is invalid",
            Self::InvalidKey => "object key is invalid",
            Self::InvalidIntent => "upload intent is invalid",
            Self::ExpiredIntent => "upload intent has expired",
            Self::ObjectMissing => "uploaded object was not found",
            Self::ObjectMismatch => "uploaded object does not match its intent",
            Self::StorageUnavailable => "object storage is unavailable",
        })
    }
}

impl Error for MediaError {}
