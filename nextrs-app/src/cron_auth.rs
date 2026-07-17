use std::fmt;

use axum::http::{HeaderMap, header};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Hashed authentication material for the unlinked Vercel Cron endpoint.
///
/// The configured bearer token is reduced to a fixed-size digest as soon as
/// configuration is loaded. Request tokens are hashed before comparison so
/// the equality check has no secret-dependent early return or length branch.
#[derive(Clone)]
pub(crate) struct CronAuthenticator {
    expected_digest: Option<[u8; 32]>,
}

impl CronAuthenticator {
    pub(crate) fn new(secret: Option<&str>) -> Self {
        Self {
            expected_digest: secret.map(digest),
        }
    }

    pub(crate) fn is_configured(&self) -> bool {
        self.expected_digest.is_some()
    }

    pub(crate) fn authorizes(&self, headers: &HeaderMap) -> bool {
        let Some(expected) = self.expected_digest else {
            return false;
        };
        let Some(value) = headers.get(header::AUTHORIZATION) else {
            return false;
        };
        let bytes = value.as_bytes();
        if bytes.len() <= 7 || !bytes[..6].eq_ignore_ascii_case(b"bearer") || bytes[6] != b' ' {
            return false;
        }
        let supplied = digest(&bytes[7..]);
        bool::from(expected.ct_eq(&supplied))
    }
}

impl fmt::Debug for CronAuthenticator {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CronAuthenticator")
            .field("configured", &self.is_configured())
            .finish()
    }
}

fn digest(secret: impl AsRef<[u8]>) -> [u8; 32] {
    Sha256::digest(secret.as_ref()).into()
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};

    use super::CronAuthenticator;

    #[test]
    fn bearer_authentication_is_exact_and_debug_is_redacted() {
        let authenticator = CronAuthenticator::new(Some("sentinel-cron-secret"));
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sentinel-cron-secret"),
        );
        assert!(authenticator.authorizes(&headers));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer sentinel-cron-secret"),
        );
        assert!(authenticator.authorizes(&headers));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer sentinel-cron-secreu"),
        );
        assert!(!authenticator.authorizes(&headers));
        assert!(!format!("{authenticator:?}").contains("sentinel"));
    }

    #[test]
    fn missing_or_disabled_authentication_fails_closed() {
        let headers = HeaderMap::new();
        assert!(!CronAuthenticator::new(Some("secret")).authorizes(&headers));
        assert!(!CronAuthenticator::new(None).authorizes(&headers));
    }
}
