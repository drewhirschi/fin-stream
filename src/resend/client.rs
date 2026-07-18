use std::{fmt, sync::Arc, time::Duration};

use async_trait::async_trait;
use reqwest::{Client, Response, StatusCode, redirect::Policy};
use serde::Deserialize;
use url::Url;
use zeroize::Zeroizing;

const BASE_URL: &str = "https://api.resend.com";
const MAX_EMAIL_RESPONSE_BYTES: usize = 2 * 1_024 * 1_024;
const MAX_ATTACHMENT_LIST_BYTES: usize = 1024 * 1024;
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderEmail {
    pub html: Option<String>,
    pub text: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderAttachment {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub download_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentDownload {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderError {
    Unavailable,
    InvalidResponse,
    ResponseTooLarge,
    InvalidDownloadUrl,
}

impl ProviderError {
    pub const fn is_transient(self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unavailable => "email provider is temporarily unavailable",
            Self::InvalidResponse => "email provider returned an invalid response",
            Self::ResponseTooLarge => "email provider response exceeded the allowed size",
            Self::InvalidDownloadUrl => "email provider returned an invalid attachment location",
        })
    }
}

impl std::error::Error for ProviderError {}

#[async_trait]
pub trait ResendProvider: Send + Sync {
    async fn get_received_email(&self, email_id: &str) -> Result<ProviderEmail, ProviderError>;

    async fn list_attachments(
        &self,
        email_id: &str,
    ) -> Result<Vec<ProviderAttachment>, ProviderError>;

    async fn download_attachment(
        &self,
        download_url: &str,
    ) -> Result<AttachmentDownload, ProviderError>;
}

pub struct ResendClient {
    http: Client,
    api_key: Arc<Zeroizing<String>>,
    base_url: Url,
    allowed_download_host: String,
    allow_http: bool,
}

impl fmt::Debug for ResendClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResendClient")
            .field("api_key", &"[REDACTED]")
            .field("base_url", &self.base_url)
            .field("allowed_download_host", &self.allowed_download_host)
            .finish()
    }
}

impl ResendClient {
    pub fn new(api_key: &str) -> Result<Self, ProviderError> {
        if api_key.trim().is_empty() || api_key.len() > 1_024 {
            return Err(ProviderError::InvalidResponse);
        }
        Self::with_endpoint(
            api_key,
            Url::parse(BASE_URL).map_err(|_| ProviderError::InvalidResponse)?,
            "inbound-cdn.resend.com".to_owned(),
            false,
        )
    }

    fn with_endpoint(
        api_key: &str,
        base_url: Url,
        allowed_download_host: String,
        allow_http: bool,
    ) -> Result<Self, ProviderError> {
        if base_url.host_str().is_none()
            || !base_url.username().is_empty()
            || base_url.password().is_some()
            || base_url.query().is_some()
            || base_url.fragment().is_some()
            || (!allow_http && base_url.scheme() != "https")
            || (allow_http && !matches!(base_url.scheme(), "http" | "https"))
        {
            return Err(ProviderError::InvalidResponse);
        }
        let http = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent("trust-deeds/0.1")
            .build()
            .map_err(|_| ProviderError::InvalidResponse)?;
        Ok(Self {
            http,
            api_key: Arc::new(Zeroizing::new(api_key.trim().to_owned())),
            base_url,
            allowed_download_host,
            allow_http,
        })
    }

    #[cfg(all(test, feature = "local-db"))]
    pub(crate) fn for_test(base_url: Url) -> Result<Self, ProviderError> {
        let host = base_url
            .host_str()
            .ok_or(ProviderError::InvalidResponse)?
            .to_owned();
        Self::with_endpoint("test-api-key", base_url, host, true)
    }

    fn api_url(&self, suffix: &str) -> Result<Url, ProviderError> {
        self.base_url
            .join(suffix)
            .map_err(|_| ProviderError::InvalidResponse)
    }

    fn validate_download_url(&self, raw: &str) -> Result<Url, ProviderError> {
        if raw.len() > 8 * 1_024 {
            return Err(ProviderError::InvalidDownloadUrl);
        }
        let url = Url::parse(raw).map_err(|_| ProviderError::InvalidDownloadUrl)?;
        let host = url
            .host_str()
            .ok_or(ProviderError::InvalidDownloadUrl)?
            .to_ascii_lowercase();
        let allowed_host = self.allowed_download_host.to_ascii_lowercase();
        let host_allowed = host == allowed_host;
        if !host_allowed
            || (!self.allow_http && url.scheme() != "https")
            || (!self.allow_http && url.port_or_known_default() != Some(443))
            || (self.allow_http && !matches!(url.scheme(), "http" | "https"))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(ProviderError::InvalidDownloadUrl);
        }
        Ok(url)
    }
}

#[async_trait]
impl ResendProvider for ResendClient {
    async fn get_received_email(&self, email_id: &str) -> Result<ProviderEmail, ProviderError> {
        validate_provider_id(email_id)?;
        let mut url = self.api_url(&format!("emails/receiving/{email_id}"))?;
        // Keep inline images as bounded attachment references instead of
        // asking Resend to inflate them into data URIs inside the HTML body.
        url.query_pairs_mut().append_pair("html_format", "cid");
        let response = self
            .http
            .get(url)
            .bearer_auth(self.api_key.as_str())
            .send()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        let bytes = successful_body(response, MAX_EMAIL_RESPONSE_BYTES).await?;
        let response: ReceivedEmailResponse =
            serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidResponse)?;
        if response
            .html
            .as_ref()
            .is_some_and(|value| value.len() > MAX_EMAIL_RESPONSE_BYTES)
            || response
                .text
                .as_ref()
                .is_some_and(|value| value.len() > MAX_EMAIL_RESPONSE_BYTES)
        {
            return Err(ProviderError::ResponseTooLarge);
        }
        Ok(ProviderEmail {
            html: response.html,
            text: response.text,
        })
    }

    async fn list_attachments(
        &self,
        email_id: &str,
    ) -> Result<Vec<ProviderAttachment>, ProviderError> {
        validate_provider_id(email_id)?;
        let url = self.api_url(&format!("emails/receiving/{email_id}/attachments"))?;
        let response = self
            .http
            .get(url)
            .bearer_auth(self.api_key.as_str())
            .send()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        let bytes = successful_body(response, MAX_ATTACHMENT_LIST_BYTES).await?;
        let response: AttachmentListResponse =
            serde_json::from_slice(&bytes).map_err(|_| ProviderError::InvalidResponse)?;
        if response.data.len() > 64 {
            return Err(ProviderError::ResponseTooLarge);
        }
        response
            .data
            .into_iter()
            .map(|attachment| {
                validate_provider_id(&attachment.id)?;
                validate_metadata_text(&attachment.filename, 512)?;
                validate_content_type(&attachment.content_type)?;
                if let Some(url) = attachment.download_url.as_deref() {
                    self.validate_download_url(url)?;
                }
                Ok(ProviderAttachment {
                    id: attachment.id,
                    filename: attachment.filename,
                    content_type: attachment.content_type.to_ascii_lowercase(),
                    download_url: attachment.download_url,
                })
            })
            .collect()
    }

    async fn download_attachment(
        &self,
        download_url: &str,
    ) -> Result<AttachmentDownload, ProviderError> {
        let url = self.validate_download_url(download_url)?;
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|_| ProviderError::Unavailable)?;
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .filter(|value| validate_content_type(value).is_ok())
            .unwrap_or("application/octet-stream")
            .to_ascii_lowercase();
        let bytes = successful_body(response, MAX_ATTACHMENT_BYTES).await?;
        Ok(AttachmentDownload {
            bytes,
            content_type,
        })
    }
}

async fn successful_body(
    mut response: Response,
    max_bytes: usize,
) -> Result<Vec<u8>, ProviderError> {
    if !response.status().is_success() {
        return Err(classify_status(response.status()));
    }
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(ProviderError::ResponseTooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ProviderError::Unavailable)?
    {
        if body.len().saturating_add(chunk.len()) > max_bytes {
            return Err(ProviderError::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn classify_status(status: StatusCode) -> ProviderError {
    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        ProviderError::Unavailable
    } else {
        ProviderError::InvalidResponse
    }
}

fn validate_provider_id(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(())
}

fn validate_metadata_text(value: &str, max_bytes: usize) -> Result<(), ProviderError> {
    if value.is_empty() || value.len() > max_bytes || value.chars().any(char::is_control) {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(())
}

fn validate_content_type(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 255
        || value != value.trim()
        || value.chars().any(char::is_control)
        || value.matches('/').count() != 1
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'/' | b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                ))
        })
    {
        return Err(ProviderError::InvalidResponse);
    }
    Ok(())
}

#[derive(Deserialize)]
struct ReceivedEmailResponse {
    html: Option<String>,
    text: Option<String>,
}

#[derive(Deserialize)]
struct AttachmentListResponse {
    data: Vec<AttachmentResponse>,
}

#[derive(Deserialize)]
struct AttachmentResponse {
    id: String,
    filename: String,
    content_type: String,
    download_url: Option<String>,
}
