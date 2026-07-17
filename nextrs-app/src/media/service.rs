use std::{collections::BTreeMap, env, fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use url::{Host, Url};
use zeroize::Zeroizing;

use super::{
    MediaError, MediaResult,
    key::{aws_encode_path, aws_encode_query, canonical_key, photo_route_url},
};

type HmacSha256 = Hmac<Sha256>;

const UPLOAD_EXPIRY_SECONDS: i64 = 5 * 60;
const DOWNLOAD_EXPIRY_SECONDS: u64 = 60;
const MAX_UPLOAD_BYTES: u64 = 25 * 1_024 * 1_024;
const MAX_SERVER_PUT_BYTES: usize = 25 * 1_024 * 1_024;
const MAX_INTENT_BYTES: usize = 8 * 1_024;
const UPLOAD_MARKER_HEADER: &str = "x-amz-meta-trust-deeds-upload";
const SHA256_HEADER: &str = "x-amz-meta-trust-deeds-sha256";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadObject {
    pub size_bytes: u64,
    pub content_type: Option<String>,
    pub upload_marker: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UploadIntentDraft {
    pub connection_id: i64,
    pub loan_account: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PresignedUpload {
    pub method: &'static str,
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct UploadIntent {
    pub token: String,
    pub object_key: String,
    pub image_url: String,
    pub upload: PresignedUpload,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedUpload {
    pub connection_id: i64,
    pub loan_account: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub object_key: String,
    pub image_url: String,
}

#[async_trait]
pub(crate) trait MediaBackend: Send + Sync {
    fn presign_put(
        &self,
        object_key: &str,
        content_type: &str,
        size_bytes: u64,
        upload_marker: &str,
        now: OffsetDateTime,
    ) -> MediaResult<PresignedUpload>;

    fn presign_get(&self, object_key: &str, now: OffsetDateTime) -> MediaResult<String>;

    async fn head(&self, object_key: &str, now: OffsetDateTime) -> MediaResult<Option<HeadObject>>;

    async fn put_if_absent(
        &self,
        object_key: &str,
        body: Vec<u8>,
        content_type: &str,
        sha256: &str,
        now: OffsetDateTime,
    ) -> MediaResult<HeadObject>;
}

#[derive(Clone)]
pub struct MediaService {
    backend: Option<Arc<dyn MediaBackend>>,
    intent_key: Option<Arc<Zeroizing<[u8; 32]>>>,
    content_security_origin: Option<Arc<str>>,
}

impl fmt::Debug for MediaService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MediaService")
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

impl MediaService {
    /// Resolve the legacy-compatible S3/R2 environment contract. Production
    /// and remote builds require all values; explicit local development may
    /// leave all of them unset, in which case media mutations/read redirects
    /// report an unavailable feature instead of writing to ephemeral disk.
    pub fn from_env() -> MediaResult<Self> {
        let required = cfg!(all(feature = "remote-db", not(feature = "local-db")))
            || env::var("VERCEL_ENV").is_ok_and(|value| value.eq_ignore_ascii_case("production"))
            || env::var("APP_ENV").is_ok_and(|value| value.eq_ignore_ascii_case("production"));
        let Some(config) = S3Config::from_env(required)? else {
            return Ok(Self::disabled());
        };
        Self::from_config(config)
    }

    pub fn disabled() -> Self {
        Self {
            backend: None,
            intent_key: None,
            content_security_origin: None,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.backend.is_some()
    }

    pub fn content_security_origin(&self) -> Option<&str> {
        self.content_security_origin.as_deref()
    }

    pub fn create_upload_intent(&self, draft: UploadIntentDraft) -> MediaResult<UploadIntent> {
        self.create_upload_intent_at(draft, OffsetDateTime::now_utc())
    }

    pub async fn verify_uploaded_intent(
        &self,
        token: &str,
        expected_connection_id: i64,
        expected_loan_account: &str,
    ) -> MediaResult<VerifiedUpload> {
        self.verify_uploaded_intent_at(
            token,
            expected_connection_id,
            expected_loan_account,
            OffsetDateTime::now_utc(),
        )
        .await
    }

    pub fn presign_download(&self, object_key: &str) -> MediaResult<String> {
        self.presign_download_at(object_key, OffsetDateTime::now_utc())
    }

    /// Store provider-owned bytes under a deterministic canonical key without
    /// ever replacing different content at that key. A concurrent identical
    /// writer is accepted only after size, content type, and SHA-256 metadata
    /// are verified with a separate HEAD request.
    pub async fn put_canonical_if_absent(
        &self,
        object_key: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> MediaResult<HeadObject> {
        canonical_key(object_key)?;
        if body.len() > MAX_SERVER_PUT_BYTES
            || content_type.trim().is_empty()
            || content_type.len() > 255
            || content_type.chars().any(char::is_control)
        {
            return Err(MediaError::InvalidInput);
        }
        let sha256 = hex_digest(&body);
        self.backend
            .as_ref()
            .ok_or(MediaError::Disabled)?
            .put_if_absent(
                object_key,
                body,
                content_type,
                &sha256,
                OffsetDateTime::now_utc(),
            )
            .await
    }

    fn create_upload_intent_at(
        &self,
        draft: UploadIntentDraft,
        now: OffsetDateTime,
    ) -> MediaResult<UploadIntent> {
        let backend = self.backend.as_ref().ok_or(MediaError::Disabled)?;
        let key = self.intent_key.as_ref().ok_or(MediaError::Disabled)?;
        validate_upload_draft(&draft)?;

        let issued_at = now.unix_timestamp();
        let expires_at = issued_at + UPLOAD_EXPIRY_SECONDS;
        let upload_marker = random_token(18);
        let object_key = generated_photo_key(&draft, issued_at, &upload_marker)?;
        let image_url = photo_route_url(&object_key)?;
        let claims = UploadClaims {
            version: 1,
            connection_id: draft.connection_id,
            loan_account: draft.loan_account,
            file_name: draft.file_name,
            content_type: draft.content_type,
            size_bytes: draft.size_bytes,
            object_key: object_key.clone(),
            upload_marker: upload_marker.clone(),
            issued_at,
            expires_at,
        };
        let token = sign_claims(&claims, key.as_ref())?;
        let upload = backend.presign_put(
            &object_key,
            &claims.content_type,
            claims.size_bytes,
            &upload_marker,
            now,
        )?;
        Ok(UploadIntent {
            token,
            object_key,
            image_url,
            upload,
            expires_at,
        })
    }

    async fn verify_uploaded_intent_at(
        &self,
        token: &str,
        expected_connection_id: i64,
        expected_loan_account: &str,
        now: OffsetDateTime,
    ) -> MediaResult<VerifiedUpload> {
        let backend = self.backend.as_ref().ok_or(MediaError::Disabled)?;
        let key = self.intent_key.as_ref().ok_or(MediaError::Disabled)?;
        let claims = verify_claims(token, key.as_ref(), now.unix_timestamp())?;
        if claims.connection_id != expected_connection_id
            || claims.loan_account != expected_loan_account
        {
            return Err(MediaError::InvalidIntent);
        }
        canonical_key(&claims.object_key)?;
        let metadata = backend
            .head(&claims.object_key, now)
            .await?
            .ok_or(MediaError::ObjectMissing)?;
        if metadata.size_bytes != claims.size_bytes
            || metadata
                .content_type
                .as_deref()
                .and_then(normalize_content_type)
                != Some(claims.content_type.as_str())
            || metadata.upload_marker.as_deref() != Some(claims.upload_marker.as_str())
        {
            return Err(MediaError::ObjectMismatch);
        }
        let image_url = photo_route_url(&claims.object_key)?;
        Ok(VerifiedUpload {
            connection_id: claims.connection_id,
            loan_account: claims.loan_account,
            file_name: claims.file_name,
            content_type: claims.content_type,
            size_bytes: claims.size_bytes,
            object_key: claims.object_key,
            image_url,
        })
    }

    fn presign_download_at(&self, object_key: &str, now: OffsetDateTime) -> MediaResult<String> {
        canonical_key(object_key)?;
        self.backend
            .as_ref()
            .ok_or(MediaError::Disabled)?
            .presign_get(object_key, now)
    }

    #[cfg(all(test, feature = "local-db"))]
    pub(crate) fn for_test(backend: Arc<dyn MediaBackend>, intent_secret: &[u8]) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"trust-deeds-media-intent-v1\0");
        digest.update(intent_secret);
        Self {
            backend: Some(backend),
            intent_key: Some(Arc::new(Zeroizing::new(digest.finalize().into()))),
            content_security_origin: Some(Arc::from("https://objects.example.invalid")),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadClaims {
    version: u8,
    connection_id: i64,
    loan_account: String,
    file_name: String,
    content_type: String,
    size_bytes: u64,
    object_key: String,
    upload_marker: String,
    issued_at: i64,
    expires_at: i64,
}

struct S3Config {
    endpoint: Url,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: Zeroizing<String>,
    key_prefix: Option<String>,
}

impl fmt::Debug for S3Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Config")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key", &"[REDACTED]")
            .field("secret_key", &"[REDACTED]")
            .field("key_prefix", &self.key_prefix)
            .finish()
    }
}

impl S3Config {
    fn from_env(required: bool) -> MediaResult<Option<Self>> {
        let endpoint = optional_env("S3_ENDPOINT");
        let access_key = optional_env("S3_ACCESS_KEY");
        let secret_key = optional_env("S3_SECRET_KEY");
        let bucket = optional_env("S3_BUCKET");
        let any =
            endpoint.is_some() || access_key.is_some() || secret_key.is_some() || bucket.is_some();
        if !any && !required {
            return Ok(None);
        }
        let allow_http = !required
            && optional_env("S3_ALLOW_HTTP")
                .as_deref()
                .is_some_and(parse_true);
        Self::new(
            endpoint.ok_or(MediaError::Configuration)?,
            optional_env("S3_REGION").unwrap_or_else(|| "auto".to_owned()),
            bucket.ok_or(MediaError::Configuration)?,
            access_key.ok_or(MediaError::Configuration)?,
            secret_key.ok_or(MediaError::Configuration)?,
            optional_env("S3_KEY_PREFIX").or_else(|| Some("loan-images".to_owned())),
            allow_http,
        )
        .map(Some)
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        endpoint: String,
        region: String,
        bucket: String,
        access_key: String,
        secret_key: String,
        key_prefix: Option<String>,
        allow_http: bool,
    ) -> MediaResult<Self> {
        let endpoint = Url::parse(endpoint.trim()).map_err(|_| MediaError::Configuration)?;
        if (!allow_http && endpoint.scheme() != "https")
            || (allow_http && !matches!(endpoint.scheme(), "http" | "https"))
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || !matches!(endpoint.path(), "" | "/")
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(MediaError::Configuration);
        }
        if region.trim().is_empty()
            || region.len() > 64
            || bucket.trim().is_empty()
            || bucket.len() > 255
            || bucket.contains('/')
            || bucket.chars().any(char::is_control)
            || access_key.trim().is_empty()
            || access_key.len() > 256
            || secret_key.trim().is_empty()
            || secret_key.len() > 1_024
        {
            return Err(MediaError::Configuration);
        }
        let key_prefix = key_prefix
            .map(|value| value.trim_matches('/').to_owned())
            .filter(|value| !value.is_empty())
            .map(|value| canonical_key(&value))
            .transpose()?;
        Ok(Self {
            endpoint,
            region: region.trim().to_owned(),
            bucket: bucket.trim().to_owned(),
            access_key: access_key.trim().to_owned(),
            secret_key: Zeroizing::new(secret_key.trim().to_owned()),
            key_prefix,
        })
    }
}

struct S3Backend {
    client: Client,
    endpoint: String,
    host: String,
    region: String,
    bucket: String,
    access_key: String,
    secret_key: Zeroizing<String>,
    key_prefix: Option<String>,
}

impl fmt::Debug for S3Backend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Backend")
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("bucket", &self.bucket)
            .field("access_key", &"[REDACTED]")
            .field("secret_key", &"[REDACTED]")
            .field("key_prefix", &self.key_prefix)
            .finish()
    }
}

impl MediaService {
    fn from_config(config: S3Config) -> MediaResult<Self> {
        let host = endpoint_host(&config.endpoint)?;
        let endpoint = format!("{}://{host}", config.endpoint.scheme());
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .build()
            .map_err(|_| MediaError::Configuration)?;
        let mut digest = Sha256::new();
        digest.update(b"trust-deeds-media-intent-v1\0");
        digest.update(config.secret_key.as_bytes());
        let intent_key = digest.finalize().into();
        let backend = S3Backend {
            client,
            endpoint: endpoint.clone(),
            host,
            region: config.region,
            bucket: config.bucket,
            access_key: config.access_key,
            secret_key: config.secret_key,
            key_prefix: config.key_prefix,
        };
        Ok(Self {
            backend: Some(Arc::new(backend)),
            intent_key: Some(Arc::new(Zeroizing::new(intent_key))),
            content_security_origin: Some(Arc::from(endpoint)),
        })
    }
}

#[async_trait]
impl MediaBackend for S3Backend {
    fn presign_put(
        &self,
        object_key: &str,
        content_type: &str,
        size_bytes: u64,
        upload_marker: &str,
        now: OffsetDateTime,
    ) -> MediaResult<PresignedUpload> {
        let mut headers = BTreeMap::new();
        headers.insert("content-length".to_owned(), size_bytes.to_string());
        headers.insert("content-type".to_owned(), content_type.to_owned());
        headers.insert(UPLOAD_MARKER_HEADER.to_owned(), upload_marker.to_owned());
        let url = self.presign(
            "PUT",
            object_key,
            &headers,
            UPLOAD_EXPIRY_SECONDS as u64,
            now,
        )?;

        // Browsers control Content-Length themselves. It remains in the signed
        // header set, while the caller supplies only the two permitted custom
        // headers and the browser-generated byte length must match the intent.
        headers.remove("content-length");
        Ok(PresignedUpload {
            method: "PUT",
            url,
            headers,
        })
    }

    fn presign_get(&self, object_key: &str, now: OffsetDateTime) -> MediaResult<String> {
        self.presign(
            "GET",
            object_key,
            &BTreeMap::new(),
            DOWNLOAD_EXPIRY_SECONDS,
            now,
        )
    }

    async fn head(&self, object_key: &str, now: OffsetDateTime) -> MediaResult<Option<HeadObject>> {
        let url = self.presign("HEAD", object_key, &BTreeMap::new(), 30, now)?;
        let response = self
            .client
            .head(url)
            .send()
            .await
            .map_err(|_| MediaError::StorageUnavailable)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(MediaError::StorageUnavailable);
        }
        let size_bytes = response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
            .ok_or(MediaError::StorageUnavailable)?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let upload_marker = response
            .headers()
            .get(UPLOAD_MARKER_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let sha256 = response
            .headers()
            .get(SHA256_HEADER)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        Ok(Some(HeadObject {
            size_bytes,
            content_type,
            upload_marker,
            sha256,
        }))
    }

    async fn put_if_absent(
        &self,
        object_key: &str,
        body: Vec<u8>,
        content_type: &str,
        sha256: &str,
        now: OffsetDateTime,
    ) -> MediaResult<HeadObject> {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".to_owned(), content_type.to_owned());
        headers.insert("if-none-match".to_owned(), "*".to_owned());
        headers.insert(SHA256_HEADER.to_owned(), sha256.to_owned());
        let url = self.presign("PUT", object_key, &headers, 30, now)?;
        let size_bytes = u64::try_from(body.len()).map_err(|_| MediaError::InvalidInput)?;
        let response = self
            .client
            .put(url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .header(reqwest::header::IF_NONE_MATCH, "*")
            .header(SHA256_HEADER, sha256)
            .body(body)
            .send()
            .await
            .map_err(|_| MediaError::StorageUnavailable)?;
        if !response.status().is_success()
            && !matches!(
                response.status(),
                StatusCode::PRECONDITION_FAILED | StatusCode::CONFLICT
            )
        {
            return Err(MediaError::StorageUnavailable);
        }

        let existing = self
            .head(object_key, OffsetDateTime::now_utc())
            .await?
            .ok_or(MediaError::ObjectMissing)?;
        let existing_type = existing
            .content_type
            .as_deref()
            .and_then(|value| value.split(';').next())
            .map(str::trim);
        if existing.size_bytes != size_bytes
            || existing_type != Some(content_type)
            || existing.sha256.as_deref() != Some(sha256)
        {
            return Err(MediaError::ObjectMismatch);
        }
        Ok(existing)
    }
}

impl S3Backend {
    fn presign(
        &self,
        method: &str,
        object_key: &str,
        headers: &BTreeMap<String, String>,
        expires_seconds: u64,
        now: OffsetDateTime,
    ) -> MediaResult<String> {
        let object_key = canonical_key(object_key)?;
        let stored_key = match self.key_prefix.as_deref() {
            Some(prefix) => format!("{prefix}/{object_key}"),
            None => object_key,
        };
        let canonical_uri = format!(
            "/{}/{}",
            aws_encode_path(&self.bucket),
            aws_encode_path(&stored_key)
        );
        let short_date = format!(
            "{:04}{:02}{:02}",
            now.year(),
            u8::from(now.month()),
            now.day()
        );
        let timestamp = format!(
            "{short_date}T{:02}{:02}{:02}Z",
            now.hour(),
            now.minute(),
            now.second()
        );
        let scope = format!("{short_date}/{}/s3/aws4_request", self.region);

        let mut canonical_headers = headers.clone();
        canonical_headers.insert("host".to_owned(), self.host.clone());
        let signed_headers = canonical_headers
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(";");
        let canonical_headers = canonical_headers
            .iter()
            .map(|(name, value)| format!("{name}:{}\n", canonical_header_value(value)))
            .collect::<String>();

        let mut query = [
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256".to_owned()),
            ("X-Amz-Credential", format!("{}/{scope}", self.access_key)),
            ("X-Amz-Date", timestamp.clone()),
            ("X-Amz-Expires", expires_seconds.to_string()),
            ("X-Amz-SignedHeaders", signed_headers.clone()),
        ];
        query.sort_by(|left, right| left.0.cmp(right.0));
        let canonical_query = query
            .iter()
            .map(|(name, value)| format!("{}={}", aws_encode_query(name), aws_encode_query(value)))
            .collect::<Vec<_>>()
            .join("&");
        let canonical_request = format!(
            "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\nUNSIGNED-PAYLOAD"
        );
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{timestamp}\n{scope}\n{}",
            hex_digest(canonical_request.as_bytes())
        );
        let signature = signing_key(
            self.secret_key.as_bytes(),
            &short_date,
            &self.region,
            string_to_sign.as_bytes(),
        )?;
        Ok(format!(
            "{}{canonical_uri}?{canonical_query}&X-Amz-Signature={}",
            self.endpoint,
            hex_bytes(&signature)
        ))
    }
}

fn validate_upload_draft(draft: &UploadIntentDraft) -> MediaResult<()> {
    if draft.connection_id <= 0
        || draft.loan_account.trim().is_empty()
        || draft.loan_account.len() > 128
        || draft.file_name.trim().is_empty()
        || draft.file_name.len() > 255
        || draft.file_name.chars().any(char::is_control)
        || draft.size_bytes == 0
        || draft.size_bytes > MAX_UPLOAD_BYTES
        || normalize_content_type(&draft.content_type) != Some(draft.content_type.as_str())
    {
        return Err(MediaError::InvalidInput);
    }
    Ok(())
}

fn normalize_content_type(raw: &str) -> Option<&str> {
    let value = raw.split(';').next()?.trim();
    matches!(value, "image/jpeg" | "image/png" | "image/webp").then_some(value)
}

fn generated_photo_key(
    draft: &UploadIntentDraft,
    issued_at: i64,
    marker: &str,
) -> MediaResult<String> {
    let loan = draft
        .loan_account
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let extension = match draft.content_type.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        _ => return Err(MediaError::InvalidInput),
    };
    canonical_key(&format!(
        "loan-workspace/{loan}/manual-{issued_at}-{marker}.{extension}"
    ))
}

fn sign_claims(claims: &UploadClaims, key: &[u8; 32]) -> MediaResult<String> {
    let payload = serde_json::to_vec(claims).map_err(|_| MediaError::InvalidIntent)?;
    let encoded = URL_SAFE_NO_PAD.encode(payload);
    let signature = hmac(key, encoded.as_bytes())?;
    Ok(format!("{encoded}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

fn verify_claims(token: &str, key: &[u8; 32], now: i64) -> MediaResult<UploadClaims> {
    if token.is_empty() || token.len() > MAX_INTENT_BYTES {
        return Err(MediaError::InvalidIntent);
    }
    let (payload, signature) = token.split_once('.').ok_or(MediaError::InvalidIntent)?;
    if signature.contains('.') {
        return Err(MediaError::InvalidIntent);
    }
    let signature = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| MediaError::InvalidIntent)?;
    let mut verifier = HmacSha256::new_from_slice(key).map_err(|_| MediaError::InvalidIntent)?;
    verifier.update(payload.as_bytes());
    verifier
        .verify_slice(&signature)
        .map_err(|_| MediaError::InvalidIntent)?;
    let payload = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| MediaError::InvalidIntent)?;
    let claims: UploadClaims =
        serde_json::from_slice(&payload).map_err(|_| MediaError::InvalidIntent)?;
    if claims.version != 1
        || claims.issued_at > now + 60
        || claims.expires_at != claims.issued_at + UPLOAD_EXPIRY_SECONDS
        || claims.expires_at < now
    {
        return Err(if claims.expires_at < now {
            MediaError::ExpiredIntent
        } else {
            MediaError::InvalidIntent
        });
    }
    validate_upload_draft(&UploadIntentDraft {
        connection_id: claims.connection_id,
        loan_account: claims.loan_account.clone(),
        file_name: claims.file_name.clone(),
        content_type: claims.content_type.clone(),
        size_bytes: claims.size_bytes,
    })?;
    Ok(claims)
}

fn signing_key(
    secret: &[u8],
    date: &str,
    region: &str,
    string_to_sign: &[u8],
) -> MediaResult<[u8; 32]> {
    let mut first = b"AWS4".to_vec();
    first.extend_from_slice(secret);
    let date_key = hmac(&first, date.as_bytes())?;
    let region_key = hmac(&date_key, region.as_bytes())?;
    let service_key = hmac(&region_key, b"s3")?;
    let signing_key = hmac(&service_key, b"aws4_request")?;
    hmac(&signing_key, string_to_sign)
}

fn hmac(key: &[u8], value: &[u8]) -> MediaResult<[u8; 32]> {
    let mut signer = HmacSha256::new_from_slice(key).map_err(|_| MediaError::Configuration)?;
    signer.update(value);
    Ok(signer.finalize().into_bytes().into())
}

fn hex_digest(value: &[u8]) -> String {
    hex_bytes(&Sha256::digest(value))
}

fn hex_bytes(value: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn canonical_header_value(value: &str) -> String {
    value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn endpoint_host(endpoint: &Url) -> MediaResult<String> {
    let host = match endpoint.host().ok_or(MediaError::Configuration)? {
        Host::Domain(value) => value.to_owned(),
        Host::Ipv4(value) => value.to_string(),
        Host::Ipv6(value) => format!("[{value}]"),
    };
    Ok(endpoint
        .port()
        .map_or(host.clone(), |port| format!("{host}:{port}")))
}

fn optional_env(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_true(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn random_token(bytes: usize) -> String {
    let mut value = vec![0_u8; bytes];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

#[cfg(test)]
mod signer_tests {
    use super::*;

    fn config(endpoint: &str, allow_http: bool) -> S3Config {
        S3Config::new(
            endpoint.to_owned(),
            "auto".to_owned(),
            "private-media".to_owned(),
            "sentinel-access-key".to_owned(),
            "sentinel-secret-key".to_owned(),
            Some("trust-deeds/prod".to_owned()),
            allow_http,
        )
        .unwrap()
    }

    #[test]
    fn storage_configuration_is_narrow_and_secrets_are_redacted() {
        let config = config("https://objects.example.invalid", false);
        let debug = format!("{config:?}");
        assert!(!debug.contains("sentinel-access-key"));
        assert!(!debug.contains("sentinel-secret-key"));
        assert!(debug.contains("[REDACTED]"));

        assert!(
            S3Config::new(
                "http://objects.example.invalid".to_owned(),
                "auto".to_owned(),
                "bucket".to_owned(),
                "access".to_owned(),
                "secret".to_owned(),
                None,
                false,
            )
            .is_err()
        );
        assert!(
            S3Config::new(
                "https://user:password@objects.example.invalid/path".to_owned(),
                "auto".to_owned(),
                "bucket".to_owned(),
                "access".to_owned(),
                "secret".to_owned(),
                None,
                false,
            )
            .is_err()
        );
        assert!(
            S3Config::new(
                "https://objects.example.invalid".to_owned(),
                "auto".to_owned(),
                "bucket".to_owned(),
                "access".to_owned(),
                "secret".to_owned(),
                Some("../other-tenant".to_owned()),
                false,
            )
            .is_err()
        );
    }

    #[test]
    fn signed_urls_are_short_lived_scoped_and_never_contain_the_secret() {
        let service =
            MediaService::from_config(config("https://objects.example.invalid:8443", false))
                .unwrap();
        assert_eq!(
            service.content_security_origin(),
            Some("https://objects.example.invalid:8443")
        );
        let url = service
            .presign_download_at(
                "loan-workspace/LN 1/front + side.jpg",
                OffsetDateTime::from_unix_timestamp(1_720_953_600).unwrap(),
            )
            .unwrap();
        assert!(url.starts_with(
            "https://objects.example.invalid:8443/private-media/trust-deeds/prod/loan-workspace/LN%201/front%20%2B%20side.jpg?"
        ));
        assert!(url.contains("X-Amz-Expires=60"));
        assert!(url.contains("X-Amz-SignedHeaders=host"));
        assert!(url.contains("sentinel-access-key"));
        assert!(!url.contains("sentinel-secret-key"));
        let signature = url.split("X-Amz-Signature=").nth(1).unwrap();
        assert_eq!(signature.len(), 64);
        assert!(signature.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
