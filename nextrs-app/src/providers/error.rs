use std::{error::Error, fmt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderName {
    Tmo,
    Monarch,
}

impl fmt::Display for ProviderName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Tmo => "TMO",
            Self::Monarch => "Monarch",
        })
    }
}

/// A deliberately redacted provider-boundary error.
///
/// This type never stores response bodies, credentials, provider-supplied
/// messages, or request URLs. It is therefore safe to persist in an operation
/// record or include in structured logs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderError {
    InvalidConfiguration {
        provider: ProviderName,
    },
    Timeout {
        provider: ProviderName,
    },
    Transport {
        provider: ProviderName,
    },
    HttpStatus {
        provider: ProviderName,
        status: u16,
    },
    ResponseTooLarge {
        provider: ProviderName,
        limit_bytes: usize,
    },
    InvalidResponse {
        provider: ProviderName,
    },
    AuthenticationRejected {
        provider: ProviderName,
    },
    RequestRejected {
        provider: ProviderName,
    },
    MissingData {
        provider: ProviderName,
    },
}

pub type ProviderResult<T> = Result<T, ProviderError>;

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration { provider } => {
                write!(formatter, "{provider} provider configuration is invalid")
            }
            Self::Timeout { provider } => write!(formatter, "{provider} request timed out"),
            Self::Transport { provider } => {
                write!(formatter, "{provider} request could not be completed")
            }
            Self::HttpStatus { provider, status } => {
                write!(formatter, "{provider} returned HTTP status {status}")
            }
            Self::ResponseTooLarge {
                provider,
                limit_bytes,
            } => write!(
                formatter,
                "{provider} response exceeded the {limit_bytes}-byte limit"
            ),
            Self::InvalidResponse { provider } => {
                write!(formatter, "{provider} returned an invalid response")
            }
            Self::AuthenticationRejected { provider } => {
                write!(formatter, "{provider} rejected authentication")
            }
            Self::RequestRejected { provider } => {
                write!(formatter, "{provider} rejected the request")
            }
            Self::MissingData { provider } => {
                write!(
                    formatter,
                    "{provider} response did not contain required data"
                )
            }
        }
    }
}

impl Error for ProviderError {}
