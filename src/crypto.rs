use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

fn cipher() -> Aes256Gcm {
    let digest = Sha256::digest(crate::config::app_encryption_key().as_bytes());
    Aes256Gcm::new_from_slice(&digest).expect("32-byte key")
}

pub fn encrypt_string(plaintext: &str) -> anyhow::Result<(String, String)> {
    let cipher = cipher();
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("failed to encrypt secret: {e}"))?;

    Ok((BASE64.encode(ciphertext), BASE64.encode(nonce_bytes)))
}

pub fn decrypt_string(ciphertext_b64: &str, nonce_b64: &str) -> anyhow::Result<String> {
    let cipher = cipher();
    let ciphertext = BASE64
        .decode(ciphertext_b64)
        .map_err(|e| anyhow::anyhow!("invalid ciphertext encoding: {e}"))?;
    let nonce_bytes = BASE64
        .decode(nonce_b64)
        .map_err(|e| anyhow::anyhow!("invalid nonce encoding: {e}"))?;

    if nonce_bytes.len() != 12 {
        anyhow::bail!("invalid nonce length");
    }

    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
        .map_err(|e| anyhow::anyhow!("failed to decrypt secret: {e}"))?;

    String::from_utf8(plaintext).map_err(|e| anyhow::anyhow!("invalid utf-8 secret: {e}"))
}
