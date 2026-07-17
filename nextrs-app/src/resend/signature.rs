use std::{fmt, time::Duration};

use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use zeroize::Zeroizing;

type HmacSha256 = Hmac<Sha256>;

const MAX_CLOCK_SKEW: Duration = Duration::from_secs(5 * 60);
const MAX_MESSAGE_ID_BYTES: usize = 256;
const MAX_SIGNATURE_HEADER_BYTES: usize = 4 * 1_024;

#[derive(Clone)]
pub struct WebhookVerifier {
    key: std::sync::Arc<Zeroizing<Vec<u8>>>,
}

impl fmt::Debug for WebhookVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebhookVerifier([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureError {
    Missing,
    Invalid,
    Stale,
}

impl fmt::Display for SignatureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Missing => "required webhook signature headers are missing",
            Self::Invalid => "webhook signature is invalid",
            Self::Stale => "webhook signature timestamp is outside the accepted window",
        })
    }
}

impl std::error::Error for SignatureError {}

impl WebhookVerifier {
    pub fn new(secret: &str) -> Result<Self, SignatureError> {
        let encoded = secret
            .trim()
            .strip_prefix("whsec_")
            .unwrap_or(secret.trim());
        let key = STANDARD
            .decode(encoded)
            .map_err(|_| SignatureError::Invalid)?;
        if key.len() < 16 || key.len() > 256 {
            return Err(SignatureError::Invalid);
        }
        Ok(Self {
            key: std::sync::Arc::new(Zeroizing::new(key)),
        })
    }

    pub fn verify(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        now: OffsetDateTime,
    ) -> Result<(), SignatureError> {
        let message_id = header(headers, "svix-id", MAX_MESSAGE_ID_BYTES)?;
        let timestamp = header(headers, "svix-timestamp", 32)?;
        let signatures = header(headers, "svix-signature", MAX_SIGNATURE_HEADER_BYTES)?;
        let signed_at = timestamp
            .parse::<i64>()
            .map_err(|_| SignatureError::Invalid)?;
        let skew = now.unix_timestamp().abs_diff(signed_at);
        if skew > MAX_CLOCK_SKEW.as_secs() {
            return Err(SignatureError::Stale);
        }

        let mut signer =
            HmacSha256::new_from_slice(self.key.as_slice()).map_err(|_| SignatureError::Invalid)?;
        signer.update(message_id.as_bytes());
        signer.update(b".");
        signer.update(timestamp.as_bytes());
        signer.update(b".");
        signer.update(body);
        let expected = signer.finalize().into_bytes();

        let mut matched = 0_u8;
        let mut saw_v1 = false;
        for candidate in signatures.split_ascii_whitespace() {
            let Some(encoded) = candidate.strip_prefix("v1,") else {
                continue;
            };
            saw_v1 = true;
            if let Ok(decoded) = STANDARD.decode(encoded) {
                matched |= expected.as_slice().ct_eq(decoded.as_slice()).unwrap_u8();
            }
        }
        if saw_v1 && matched == 1 {
            Ok(())
        } else {
            Err(SignatureError::Invalid)
        }
    }
}

fn header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    max_bytes: usize,
) -> Result<&'a str, SignatureError> {
    let value = headers
        .get(name)
        .ok_or(SignatureError::Missing)?
        .to_str()
        .map_err(|_| SignatureError::Invalid)?;
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(SignatureError::Invalid);
    }
    Ok(value)
}

#[cfg(all(test, feature = "local-db"))]
pub(crate) fn sign_for_test(secret: &str, message_id: &str, timestamp: i64, body: &[u8]) -> String {
    let verifier = WebhookVerifier::new(secret).unwrap();
    let mut signer = HmacSha256::new_from_slice(verifier.key.as_slice()).unwrap();
    signer.update(message_id.as_bytes());
    signer.update(b".");
    signer.update(timestamp.to_string().as_bytes());
    signer.update(b".");
    signer.update(body);
    format!("v1,{}", STANDARD.encode(signer.finalize().into_bytes()))
}
