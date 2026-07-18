//! Narrow copy of the legacy/provider credential envelope. The exporter never
//! returns plaintext to callers, logs, argv, or its manifest. Encryption is
//! used only to rewrap a credential proven to require an explicitly supplied
//! legacy key under the current production key during cutover.

use std::{error::Error, fmt};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const CREDENTIAL_KEY_VERSION: i64 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CredentialCryptoError {
    EmptyKey,
    UnsupportedKeyVersion,
    InvalidEncoding,
    InvalidNonce,
    DecryptionFailed,
    EncryptionFailed,
    InvalidUtf8,
    EmptyPlaintext,
}

impl fmt::Display for CredentialCryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyKey => "credential encryption key is empty",
            Self::UnsupportedKeyVersion => "credential key version is unsupported",
            Self::InvalidEncoding => "credential encoding is invalid",
            Self::InvalidNonce => "credential nonce is invalid",
            Self::DecryptionFailed => "credential decryption failed",
            Self::EncryptionFailed => "credential encryption failed",
            Self::InvalidUtf8 => "decrypted credential is not valid UTF-8",
            Self::EmptyPlaintext => "decrypted credential is empty",
        })
    }
}

impl Error for CredentialCryptoError {}

pub(crate) struct CredentialCipher {
    derived_key: [u8; 32],
}

impl fmt::Debug for CredentialCipher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialCipher")
            .field("derived_key", &"[redacted]")
            .finish()
    }
}

impl Drop for CredentialCipher {
    fn drop(&mut self) {
        self.derived_key.zeroize();
    }
}

impl CredentialCipher {
    pub(crate) fn new(app_encryption_key: &str) -> Result<Self, CredentialCryptoError> {
        if app_encryption_key.is_empty() {
            return Err(CredentialCryptoError::EmptyKey);
        }
        let mut digest = Sha256::digest(app_encryption_key.as_bytes());
        let mut derived_key = [0_u8; 32];
        derived_key.copy_from_slice(&digest);
        digest.as_mut_slice().zeroize();
        Ok(Self { derived_key })
    }

    pub(crate) fn decrypt_canary(
        &self,
        ciphertext_b64: &str,
        nonce_b64: &str,
        key_version: i64,
    ) -> Result<(), CredentialCryptoError> {
        let plaintext = self.decrypt_parts(ciphertext_b64, nonce_b64, key_version)?;
        if plaintext.is_empty() {
            return Err(CredentialCryptoError::EmptyPlaintext);
        }
        drop(plaintext);
        Ok(())
    }

    pub(crate) fn key_fingerprint(&self) -> String {
        self.derived_key
            .iter()
            .take(4)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    pub(crate) fn decrypt_parts(
        &self,
        ciphertext_b64: &str,
        nonce_b64: &str,
        key_version: i64,
    ) -> Result<Zeroizing<String>, CredentialCryptoError> {
        if key_version != CREDENTIAL_KEY_VERSION {
            return Err(CredentialCryptoError::UnsupportedKeyVersion);
        }
        let ciphertext = BASE64
            .decode(ciphertext_b64)
            .map_err(|_| CredentialCryptoError::InvalidEncoding)?;
        let mut nonce_bytes = BASE64
            .decode(nonce_b64)
            .map_err(|_| CredentialCryptoError::InvalidEncoding)?;
        if nonce_bytes.len() != 12 {
            nonce_bytes.zeroize();
            return Err(CredentialCryptoError::InvalidNonce);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.derived_key)
            .map_err(|_| CredentialCryptoError::DecryptionFailed)?;
        let mut plaintext = cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|_| CredentialCryptoError::DecryptionFailed)?;
        nonce_bytes.zeroize();
        let plaintext = String::from_utf8(std::mem::take(&mut plaintext)).map_err(|error| {
            let mut invalid = error.into_bytes();
            invalid.zeroize();
            CredentialCryptoError::InvalidUtf8
        })?;
        Ok(Zeroizing::new(plaintext))
    }

    pub(crate) fn encrypt_parts(
        &self,
        plaintext: &str,
    ) -> Result<(String, String), CredentialCryptoError> {
        if plaintext.is_empty() {
            return Err(CredentialCryptoError::EmptyPlaintext);
        }
        let cipher = Aes256Gcm::new_from_slice(&self.derived_key)
            .map_err(|_| CredentialCryptoError::EncryptionFailed)?;
        let mut nonce_bytes = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| CredentialCryptoError::EncryptionFailed)?;
        let encoded = (BASE64.encode(ciphertext), BASE64.encode(nonce_bytes));
        nonce_bytes.zeroize();
        Ok(encoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "legacy-fixture-key";
    const CIPHERTEXT: &str = "QsRRZq1wtthAKK37nWsnen/0q535QJ3h1SkkOW8EOLerrg==";
    const NONCE: &str = "AAECAwQFBgcICQoL";

    #[test]
    fn decrypts_exact_legacy_fixture_without_exposing_plaintext() {
        let cipher = CredentialCipher::new(KEY).unwrap();
        cipher
            .decrypt_canary(CIPHERTEXT, NONCE, CREDENTIAL_KEY_VERSION)
            .unwrap();
        assert_eq!(cipher.key_fingerprint(), "63581ea5");
        let rendered = format!("{cipher:?}");
        assert!(!rendered.contains(KEY));
        assert!(!rendered.contains(CIPHERTEXT));
        assert!(!rendered.contains("fixture-secret-123"));
    }

    #[test]
    fn wrong_key_nonce_version_and_encoding_fail_closed() {
        assert_eq!(
            CredentialCipher::new("wrong")
                .unwrap()
                .decrypt_canary(CIPHERTEXT, NONCE, 1),
            Err(CredentialCryptoError::DecryptionFailed)
        );
        let cipher = CredentialCipher::new(KEY).unwrap();
        assert_eq!(
            cipher.decrypt_canary(CIPHERTEXT, "AA==", 1),
            Err(CredentialCryptoError::InvalidNonce)
        );
        assert_eq!(
            cipher.decrypt_canary(CIPHERTEXT, NONCE, 2),
            Err(CredentialCryptoError::UnsupportedKeyVersion)
        );
        assert_eq!(
            cipher.decrypt_canary("not base64", NONCE, 1),
            Err(CredentialCryptoError::InvalidEncoding)
        );
    }

    #[test]
    fn rewrap_round_trip_uses_the_new_key_only() {
        let old = CredentialCipher::new(KEY).unwrap();
        let plaintext = old
            .decrypt_parts(CIPHERTEXT, NONCE, CREDENTIAL_KEY_VERSION)
            .unwrap();
        let current = CredentialCipher::new("current-production-key").unwrap();
        let (ciphertext, nonce) = current.encrypt_parts(&plaintext).unwrap();
        drop(plaintext);
        current
            .decrypt_canary(&ciphertext, &nonce, CREDENTIAL_KEY_VERSION)
            .unwrap();
        assert_eq!(
            old.decrypt_canary(&ciphertext, &nonce, CREDENTIAL_KEY_VERSION),
            Err(CredentialCryptoError::DecryptionFailed)
        );
    }
}
