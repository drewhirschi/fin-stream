//! Legacy-compatible encryption for provider credentials at rest.
//!
//! Version 1 is intentionally byte-for-byte compatible with the Axum app:
//! `SHA-256(APP_ENCRYPTION_KEY)` is the AES-256-GCM key, the nonce is 12 random
//! bytes, no associated data is used, and both nonce and ciphertext (including
//! the GCM tag) use standard padded base64.

use std::{error::Error, fmt};

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

pub const CREDENTIAL_KEY_VERSION: i64 = 1;

#[derive(Clone, Eq, PartialEq)]
pub struct EncryptedCredential {
    pub ciphertext: String,
    pub nonce: String,
    pub key_version: i64,
}

impl fmt::Debug for EncryptedCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EncryptedCredential")
            .field("ciphertext", &"[REDACTED]")
            .field("nonce", &"[REDACTED]")
            .field("key_version", &self.key_version)
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialDecryptCanary {
    pub key_version: i64,
    /// First eight hexadecimal characters of SHA-256(APP_ENCRYPTION_KEY).
    /// This is non-reversible and safe to compare in operator output.
    pub key_fingerprint: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialCryptoError {
    EmptyKey,
    UnsupportedKeyVersion,
    InvalidEncoding,
    InvalidNonce,
    EncryptionFailed,
    DecryptionFailed,
    InvalidUtf8,
}

impl fmt::Display for CredentialCryptoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::EmptyKey => "credential encryption key is empty",
            Self::UnsupportedKeyVersion => "credential key version is unsupported",
            Self::InvalidEncoding => "credential encoding is invalid",
            Self::InvalidNonce => "credential nonce is invalid",
            Self::EncryptionFailed => "credential encryption failed",
            Self::DecryptionFailed => "credential decryption failed",
            Self::InvalidUtf8 => "decrypted credential is not valid UTF-8",
        })
    }
}

impl Error for CredentialCryptoError {}

pub struct CredentialCipher {
    derived_key: [u8; 32],
}

impl fmt::Debug for CredentialCipher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialCipher")
            .field("derived_key", &"[REDACTED]")
            .finish()
    }
}

impl Drop for CredentialCipher {
    fn drop(&mut self) {
        self.derived_key.zeroize();
    }
}

impl CredentialCipher {
    pub fn new(app_encryption_key: &str) -> Result<Self, CredentialCryptoError> {
        if app_encryption_key.is_empty() {
            return Err(CredentialCryptoError::EmptyKey);
        }
        let digest = Sha256::digest(app_encryption_key.as_bytes());
        let mut derived_key = [0_u8; 32];
        derived_key.copy_from_slice(&digest);
        Ok(Self { derived_key })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedCredential, CredentialCryptoError> {
        let mut nonce = [0_u8; 12];
        OsRng.fill_bytes(&mut nonce);
        self.encrypt_with_nonce(plaintext, nonce)
    }

    /// Decrypt a stored credential into zeroizing memory.
    pub fn decrypt(
        &self,
        encrypted: &EncryptedCredential,
    ) -> Result<Zeroizing<String>, CredentialCryptoError> {
        self.decrypt_parts(
            &encrypted.ciphertext,
            &encrypted.nonce,
            encrypted.key_version,
        )
    }

    /// Adapter-friendly form for the existing TMO/Monarch record columns.
    pub fn decrypt_parts(
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
            let mut invalid_bytes = error.into_bytes();
            invalid_bytes.zeroize();
            CredentialCryptoError::InvalidUtf8
        })?;
        Ok(Zeroizing::new(plaintext))
    }

    /// Verify a migrated credential without returning or logging plaintext.
    pub fn decrypt_canary(
        &self,
        ciphertext_b64: &str,
        nonce_b64: &str,
        key_version: i64,
    ) -> Result<CredentialDecryptCanary, CredentialCryptoError> {
        let plaintext = self.decrypt_parts(ciphertext_b64, nonce_b64, key_version)?;
        drop(plaintext);
        Ok(CredentialDecryptCanary {
            key_version,
            key_fingerprint: self.key_fingerprint(),
        })
    }

    pub fn key_fingerprint(&self) -> String {
        self.derived_key
            .iter()
            .take(4)
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn encrypt_with_nonce(
        &self,
        plaintext: &str,
        nonce_bytes: [u8; 12],
    ) -> Result<EncryptedCredential, CredentialCryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.derived_key)
            .map_err(|_| CredentialCryptoError::EncryptionFailed)?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes())
            .map_err(|_| CredentialCryptoError::EncryptionFailed)?;
        Ok(EncryptedCredential {
            ciphertext: BASE64.encode(ciphertext),
            nonce: BASE64.encode(nonce_bytes),
            key_version: CREDENTIAL_KEY_VERSION,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CREDENTIAL_KEY_VERSION, CredentialCipher, CredentialCryptoError, EncryptedCredential,
    };

    const FIXTURE_KEY: &str = "legacy-fixture-key";
    const FIXTURE_PLAINTEXT: &str = "fixture-secret-123";
    const FIXTURE_CIPHERTEXT: &str = "QsRRZq1wtthAKK37nWsnen/0q535QJ3h1SkkOW8EOLerrg==";
    const FIXTURE_NONCE: &str = "AAECAwQFBgcICQoL";

    fn fixture() -> EncryptedCredential {
        EncryptedCredential {
            ciphertext: FIXTURE_CIPHERTEXT.into(),
            nonce: FIXTURE_NONCE.into(),
            key_version: CREDENTIAL_KEY_VERSION,
        }
    }

    #[test]
    fn decrypts_deterministic_legacy_compatible_fixture() {
        let cipher = CredentialCipher::new(FIXTURE_KEY).unwrap();
        assert_eq!(
            cipher.decrypt(&fixture()).unwrap().as_str(),
            FIXTURE_PLAINTEXT
        );

        let generated = cipher
            .encrypt_with_nonce(
                FIXTURE_PLAINTEXT,
                [
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
                ],
            )
            .unwrap();
        assert_eq!(generated, fixture());
    }

    #[test]
    fn random_round_trip_and_canary_do_not_return_plaintext() {
        let cipher = CredentialCipher::new(FIXTURE_KEY).unwrap();
        let encrypted = cipher.encrypt("a provider secret").unwrap();
        assert_eq!(
            cipher.decrypt(&encrypted).unwrap().as_str(),
            "a provider secret"
        );
        let canary = cipher
            .decrypt_canary(
                &encrypted.ciphertext,
                &encrypted.nonce,
                encrypted.key_version,
            )
            .unwrap();
        assert_eq!(canary.key_version, CREDENTIAL_KEY_VERSION);
        assert_eq!(canary.key_fingerprint.len(), 8);
        assert!(!format!("{canary:?}").contains("provider secret"));
    }

    #[test]
    fn tamper_wrong_key_and_unknown_version_fail_closed() {
        let cipher = CredentialCipher::new(FIXTURE_KEY).unwrap();
        let wrong_cipher = CredentialCipher::new("a-different-key").unwrap();
        assert_eq!(
            wrong_cipher.decrypt(&fixture()).unwrap_err(),
            CredentialCryptoError::DecryptionFailed
        );

        let mut tampered = fixture();
        tampered.ciphertext.replace_range(..1, "R");
        assert_eq!(
            cipher.decrypt(&tampered).unwrap_err(),
            CredentialCryptoError::DecryptionFailed
        );

        let mut unknown = fixture();
        unknown.key_version = 2;
        assert_eq!(
            cipher.decrypt(&unknown).unwrap_err(),
            CredentialCryptoError::UnsupportedKeyVersion
        );
    }

    #[test]
    fn errors_and_debug_output_never_include_key_or_ciphertext() {
        let cipher = CredentialCipher::new(FIXTURE_KEY).unwrap();
        let encrypted = fixture();
        let error = cipher
            .decrypt_parts("not base64!", FIXTURE_NONCE, 1)
            .unwrap_err();
        let rendered = format!("{cipher:?} {encrypted:?} {error:?} {error}");
        assert!(!rendered.contains(FIXTURE_KEY));
        assert!(!rendered.contains(FIXTURE_CIPHERTEXT));
        assert!(!rendered.contains(FIXTURE_PLAINTEXT));
    }
}
