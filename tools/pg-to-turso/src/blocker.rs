use std::{error::Error, fmt};

/// An error whose message is intentionally safe to copy into the validation
/// manifest. Details such as row IDs can be added as anyhow context for stderr
/// without becoming part of the persisted cutover record.
#[derive(Debug)]
pub(crate) struct ManifestSafeBlocker(String);

impl ManifestSafeBlocker {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub(crate) fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ManifestSafeBlocker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ManifestSafeBlocker {}
