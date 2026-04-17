use std::env;
use std::path::PathBuf;

pub fn get_host() -> String {
    env::var("HOST").unwrap_or_else(|_| "0.0.0.0".into())
}

pub fn get_port() -> u16 {
    env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000)
}

pub fn get_database_url() -> String {
    env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@127.0.0.1:5432/trust_deeds".into())
}

pub fn tmo_company_id() -> String {
    env::var("TMO_COMPANY_ID").unwrap_or_else(|_| "vci".into())
}

pub fn tmo_account() -> String {
    env::var("TMO_ACCOUNT").expect("TMO_ACCOUNT must be set")
}

pub fn tmo_pin() -> String {
    env::var("TMO_PIN").expect("TMO_PIN must be set")
}

pub fn monarch_token() -> String {
    env::var("MONARCH_TOKEN").expect("MONARCH_TOKEN must be set")
}

pub fn monarch_account_id() -> String {
    env::var("MONARCH_ACCOUNT_ID").unwrap_or_else(|_| "217902882668946592".into())
}

/// Returns the app encryption key used to derive the AES-256-GCM key for
/// at-rest secrets (TMO PIN, Monarch token).
///
/// Behavior:
/// - If `APP_ENCRYPTION_KEY` is set to a non-empty value, use it.
/// - Otherwise, in debug builds (or when `APP_ENV=dev`), fall back to a
///   well-known development key so local iteration doesn't require env setup.
/// - In release builds without the env var, panic loudly. Silently switching
///   to a dev key in production causes "failed to decrypt secret" the next
///   time the real key is provided and we try to read rows bootstrapped with
///   the dev key.
pub fn app_encryption_key() -> String {
    if let Ok(value) = env::var("APP_ENCRYPTION_KEY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let allow_dev_fallback = cfg!(debug_assertions)
        || env::var("APP_ENV")
            .ok()
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "dev" || v == "development" || v == "test"
            })
            .unwrap_or(false);

    if allow_dev_fallback {
        tracing::warn!(
            "APP_ENCRYPTION_KEY not set; using development fallback key (debug/dev build only)"
        );
        return "trust-deeds-dev-only-encryption-key".into();
    }

    // Release build with no explicit key: refuse to boot. Exiting here is
    // preferable to silently falling back and corrupting the credential table.
    panic!(
        "APP_ENCRYPTION_KEY must be set in release builds. Refusing to boot with the development \
         fallback key. Set APP_ENCRYPTION_KEY in the deployment environment (e.g. Coolify env \
         vars) and restart. If you intentionally want the dev fallback on this host, set \
         APP_ENV=dev."
    );
}

/// Return a short, non-reversible fingerprint of the current encryption key
/// for log diffing. First 8 hex chars of SHA-256(key). Safe to log.
pub fn app_encryption_key_fingerprint() -> String {
    use sha2::{Digest, Sha256};
    let key = app_encryption_key();
    let digest = Sha256::digest(key.as_bytes());
    let hex: String = digest.iter().take(4).map(|b| format!("{:02x}", b)).collect();
    hex
}

pub fn admin_email() -> Option<String> {
    env::var("ADMIN_EMAIL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn admin_password() -> Option<String> {
    env::var("ADMIN_PASSWORD")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn loan_image_storage_dir() -> PathBuf {
    env::var("LOAN_IMAGE_STORAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("static/loan-images"))
}

pub fn loan_image_base_url() -> String {
    env::var("LOAN_IMAGE_BASE_URL").unwrap_or_else(|_| "/static/loan-images".into())
}

pub fn resend_api_key() -> Option<String> {
    env::var("RESEND_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn resend_webhook_secret() -> Option<String> {
    env::var("RESEND_WEBHOOK_SECRET")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn s3_endpoint() -> Option<String> {
    env::var("S3_ENDPOINT")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn s3_access_key() -> Option<String> {
    env::var("S3_ACCESS_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn s3_secret_key() -> Option<String> {
    env::var("S3_SECRET_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn s3_bucket() -> Option<String> {
    env::var("S3_BUCKET")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

pub fn s3_region() -> String {
    env::var("S3_REGION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "auto".into())
}

pub fn s3_key_prefix() -> String {
    env::var("S3_KEY_PREFIX")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "loan-images".into())
}
