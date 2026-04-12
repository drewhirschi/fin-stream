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

pub fn app_encryption_key() -> String {
    env::var("APP_ENCRYPTION_KEY").unwrap_or_else(|_| {
        tracing::warn!("APP_ENCRYPTION_KEY not set; using development fallback key");
        "trust-deeds-dev-only-encryption-key".into()
    })
}

pub fn loan_image_storage_dir() -> PathBuf {
    env::var("LOAN_IMAGE_STORAGE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("static/loan-images"))
}

pub fn loan_image_base_url() -> String {
    env::var("LOAN_IMAGE_BASE_URL").unwrap_or_else(|_| "/static/loan-images".into())
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
