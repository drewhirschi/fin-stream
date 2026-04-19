use std::sync::Arc;

use axum::{
    Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use sqlx::PgPool;

use crate::{AppState, config, db, media_storage::MediaStorage, resend::ResendClient};

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/webhooks/resend", post(resend_webhook))
}

#[derive(Deserialize)]
struct ResendWebhookEvent {
    #[serde(rename = "type")]
    event_type: String,
    data: serde_json::Value,
    created_at: Option<String>,
}

#[derive(Deserialize)]
struct ReceivedEmailData {
    email_id: String,
    from: String,
    to: Vec<String>,
    subject: Option<String>,
    #[serde(default)]
    attachments: Vec<AttachmentRef>,
}

#[derive(Deserialize)]
struct AttachmentRef {
    id: String,
    filename: String,
    content_type: String,
}

async fn resend_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> impl IntoResponse {
    // Validate SVIX signature if secret is configured
    if let Some(secret) = config::resend_webhook_secret() {
        if let Err(e) = verify_svix_signature(&headers, &body, &secret) {
            tracing::warn!("webhook signature verification failed: {e}");
            return StatusCode::UNAUTHORIZED;
        }
    }

    // Parse the event
    let event: ResendWebhookEvent = match serde_json::from_str(&body) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("webhook parse error: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    // Only handle email.received events
    if event.event_type != "email.received" {
        return StatusCode::OK;
    }

    let data: ReceivedEmailData = match serde_json::from_value(event.data) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("webhook data parse error: {e}");
            return StatusCode::BAD_REQUEST;
        }
    };

    let received_at = event.created_at.unwrap_or_default();
    let to_json = serde_json::to_string(&data.to).unwrap_or_else(|_| "[]".into());

    // Insert into DB (idempotent)
    let email_id = match db::emails::insert_received_email(
        &state.db,
        &data.email_id,
        &data.from,
        &to_json,
        data.subject.as_deref(),
        &received_at,
        &body,
    )
    .await
    {
        Ok(Some(id)) => id,
        Ok(None) => {
            // Duplicate webhook — already processed
            tracing::debug!("duplicate webhook for email {}", data.email_id);
            return StatusCode::OK;
        }
        Err(e) => {
            tracing::error!("failed to insert received email: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR;
        }
    };

    // Insert attachment rows
    let mut attachment_ids = Vec::new();
    for att in &data.attachments {
        match db::emails::insert_attachment_row(
            &state.db,
            email_id,
            &att.id,
            &att.filename,
            &att.content_type,
        )
        .await
        {
            Ok(id) => attachment_ids.push((id, att.id.clone(), att.filename.clone())),
            Err(e) => tracing::error!("failed to insert attachment row: {e}"),
        }
    }

    state.page_cache.invalidate("inbox").await;

    // Spawn background task to fetch body + attachments from Resend API
    let pool = state.db.clone();
    let resend_email_id = data.email_id.clone();
    tokio::spawn(async move {
        if let Err(e) =
            fetch_and_store_email(&pool, &resend_email_id, email_id, &attachment_ids).await
        {
            tracing::error!(
                "failed to fetch/store email {resend_email_id}: {e}"
            );
            let _ = db::emails::mark_email_error(&pool, email_id, &e.to_string()).await;
        }
    });

    StatusCode::OK
}

pub(crate) async fn fetch_and_store_email(
    pool: &PgPool,
    resend_email_id: &str,
    email_db_id: i64,
    attachment_ids: &[(i64, String, String)], // (db_id, resend_attachment_id, filename)
) -> anyhow::Result<()> {
    let api_key = config::resend_api_key()
        .ok_or_else(|| anyhow::anyhow!("RESEND_API_KEY not configured"))?;

    let client = ResendClient::new(&api_key);
    let storage = MediaStorage::from_env().await?;

    // Fetch email body
    let email = client.get_received_email(resend_email_id).await?;

    let (body_bytes, ext, content_type) = if let Some(html) = &email.html {
        (html.as_bytes().to_vec(), "html", "text/html")
    } else if let Some(text) = &email.text {
        (text.as_bytes().to_vec(), "txt", "text/plain")
    } else {
        (Vec::new(), "txt", "text/plain")
    };

    if !body_bytes.is_empty() {
        let body_key = format!("emails/{resend_email_id}/body.{ext}");
        storage
            .store(&body_key, body_bytes, Some(content_type))
            .await?;
        db::emails::mark_email_body_stored(pool, email_db_id, &body_key, content_type).await?;
    } else {
        db::emails::mark_email_body_stored(pool, email_db_id, "", "text/plain").await?;
    }

    // Fetch signed download URLs from Resend, then pull bytes from each.
    let att_list = match client.list_attachments(resend_email_id).await {
        Ok(list) => list,
        Err(e) => {
            tracing::error!("failed to list attachments for {resend_email_id}: {e}");
            Vec::new()
        }
    };

    for (db_id, resend_att_id, filename) in attachment_ids {
        let Some(meta) = att_list.iter().find(|m| &m.id == resend_att_id) else {
            tracing::warn!(
                "attachment {resend_att_id} ({filename}) in webhook payload but not in Resend list response"
            );
            continue;
        };
        let Some(download_url) = meta.download_url.as_deref() else {
            tracing::warn!("attachment {resend_att_id} has no download_url in list response");
            continue;
        };

        match client.download_attachment(download_url).await {
            Ok((bytes, _ct)) => {
                let safe_filename = filename.replace(['/', '\\', '\0'], "_");
                let att_key =
                    format!("emails/{resend_email_id}/attachments/{safe_filename}");
                let size = bytes.len() as i32;
                if let Err(e) = storage.store(&att_key, bytes, None).await {
                    tracing::error!("failed to store attachment {filename}: {e}");
                    continue;
                }
                let _ =
                    db::emails::mark_attachment_stored(pool, *db_id, &att_key, size).await;
            }
            Err(e) => {
                tracing::error!("failed to fetch attachment {filename}: {e}");
            }
        }
    }

    tracing::info!("email {resend_email_id} stored successfully");
    Ok(())
}

fn verify_svix_signature(
    headers: &HeaderMap,
    body: &str,
    secret: &str,
) -> anyhow::Result<()> {
    let msg_id = headers
        .get("svix-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing svix-id header"))?;
    let timestamp = headers
        .get("svix-timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing svix-timestamp header"))?;
    let signatures = headers
        .get("svix-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| anyhow::anyhow!("missing svix-signature header"))?;

    // Decode the secret (strip "whsec_" prefix, then base64 decode)
    let secret_bytes = {
        let raw = secret.strip_prefix("whsec_").unwrap_or(secret);
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(raw)?
    };

    // Compute expected signature
    let signed_content = format!("{msg_id}.{timestamp}.{body}");
    let mut mac = Hmac::<Sha256>::new_from_slice(&secret_bytes)?;
    mac.update(signed_content.as_bytes());
    let expected = mac.finalize().into_bytes();

    let expected_b64 = {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(expected)
    };

    // Check against any of the provided signatures (v1,<sig> format)
    for sig in signatures.split(' ') {
        if let Some(sig_b64) = sig.strip_prefix("v1,") {
            if sig_b64 == expected_b64 {
                return Ok(());
            }
        }
    }

    anyhow::bail!("no matching signature found")
}
