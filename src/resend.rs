use reqwest::Client;
use serde::Deserialize;

const BASE_URL: &str = "https://api.resend.com";

pub struct ResendClient {
    http: Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct ReceivedEmailResponse {
    pub html: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AttachmentListResponse {
    pub data: Vec<AttachmentMeta>,
}

#[derive(Debug, Deserialize)]
pub struct AttachmentMeta {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    /// Signed CDN URL to download the attachment bytes. Short-lived (~1 hour).
    pub download_url: Option<String>,
}

impl ResendClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            http: Client::new(),
            api_key: api_key.to_string(),
        }
    }

    /// Fetch the full email content (body) for a received email.
    ///
    /// Inbound emails live under `/emails/receiving/{id}` — distinct from the
    /// `/emails/{id}` endpoint which is for outbound (sent) emails only.
    pub async fn get_received_email(
        &self,
        email_id: &str,
    ) -> anyhow::Result<ReceivedEmailResponse> {
        let url = format!("{BASE_URL}/emails/receiving/{email_id}");
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Resend API error {status}: {body}");
        }

        Ok(resp.json().await?)
    }

    /// List attachments for a received email. Each entry includes a signed
    /// `download_url` for fetching the raw bytes.
    pub async fn list_attachments(
        &self,
        email_id: &str,
    ) -> anyhow::Result<Vec<AttachmentMeta>> {
        let url = format!("{BASE_URL}/emails/receiving/{email_id}/attachments");
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Resend attachments API error {status}: {body}");
        }

        let list: AttachmentListResponse = resp.json().await?;
        Ok(list.data)
    }

    /// Download a single attachment via its signed `download_url`. Resend
    /// doesn't expose a direct-bytes API endpoint for inbound attachments —
    /// the list endpoint returns a short-lived CDN URL (~1 hour) that we fetch
    /// without the API key.
    pub async fn download_attachment(
        &self,
        download_url: &str,
    ) -> anyhow::Result<(Vec<u8>, String)> {
        let resp = self.http.get(download_url).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("attachment download error {status}: {body}");
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = resp.bytes().await?.to_vec();
        Ok((bytes, content_type))
    }
}
