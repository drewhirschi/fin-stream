use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Redirect},
    routing::get,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use libsql::Builder;
use time::OffsetDateTime;
use tower::ServiceExt;

use crate::{
    cron_auth::CronAuthenticator,
    crypto::CredentialCipher,
    db::AppContext,
    media::{HeadObject, MediaBackend, MediaError, MediaResult, MediaService, PresignedUpload},
    operations::{OperationRepository, utc_now_millis},
    app::router_with_store,
    session_store::LibsqlSessionStore,
    workspace_inbox::WorkspaceInboxRepository,
};

use super::resolve_service;
use super::{
    AttachmentDownload, ClaimOutcome, InboundEmailDraft, InboundEmailRepository,
    ProviderAttachment, ProviderEmail, ResendProvider, ResendService, WebhookVerifier,
    client::{ProviderError, ResendClient},
    signature::{SignatureError, sign_for_test},
};

const WEBHOOK_SECRET: &str = "whsec_MDEyMzQ1Njc4OWFiY2RlZg==";
const EMAIL: &str = "admin@example.com";
const PASSWORD: &str = "correct horse battery staple";

#[derive(Default)]
struct FakeProvider {
    email_calls: AtomicUsize,
    attachment_list_calls: AtomicUsize,
    download_calls: AtomicUsize,
    transient_download_failures: AtomicUsize,
    permanent_email_failure: bool,
}

impl FakeProvider {
    fn with_transient_download_failure() -> Self {
        Self {
            transient_download_failures: AtomicUsize::new(1),
            ..Self::default()
        }
    }
}

#[async_trait]
impl ResendProvider for FakeProvider {
    async fn get_received_email(&self, _email_id: &str) -> Result<ProviderEmail, ProviderError> {
        self.email_calls.fetch_add(1, Ordering::SeqCst);
        if self.permanent_email_failure {
            return Err(ProviderError::InvalidResponse);
        }
        Ok(ProviderEmail {
            html: Some("<p>Safe fixture body</p>".to_owned()),
            text: None,
        })
    }

    async fn list_attachments(
        &self,
        _email_id: &str,
    ) -> Result<Vec<ProviderAttachment>, ProviderError> {
        self.attachment_list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![ProviderAttachment {
            id: "attachment_1".to_owned(),
            filename: "statement.pdf".to_owned(),
            content_type: "application/pdf".to_owned(),
            download_url: Some("https://inbound-cdn.resend.com/fixture".to_owned()),
        }])
    }

    async fn download_attachment(
        &self,
        _download_url: &str,
    ) -> Result<AttachmentDownload, ProviderError> {
        self.download_calls.fetch_add(1, Ordering::SeqCst);
        if self
            .transient_download_failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(ProviderError::Unavailable);
        }
        Ok(AttachmentDownload {
            bytes: b"fixture attachment".to_vec(),
            content_type: "application/pdf".to_owned(),
        })
    }
}

#[derive(Default)]
struct BlockingProvider {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    calls: AtomicUsize,
}

#[async_trait]
impl ResendProvider for BlockingProvider {
    async fn get_received_email(&self, _email_id: &str) -> Result<ProviderEmail, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(ProviderEmail {
            html: None,
            text: Some("Concurrent fixture".to_owned()),
        })
    }

    async fn list_attachments(
        &self,
        _email_id: &str,
    ) -> Result<Vec<ProviderAttachment>, ProviderError> {
        Ok(Vec::new())
    }

    async fn download_attachment(
        &self,
        _download_url: &str,
    ) -> Result<AttachmentDownload, ProviderError> {
        Err(ProviderError::InvalidResponse)
    }
}

type StoredTestObject = (Vec<u8>, String, String);

#[derive(Default)]
struct MemoryMediaBackend {
    objects: Mutex<BTreeMap<String, StoredTestObject>>,
}

#[async_trait]
impl MediaBackend for MemoryMediaBackend {
    fn presign_put(
        &self,
        _object_key: &str,
        _content_type: &str,
        _size_bytes: u64,
        _upload_marker: &str,
        _now: OffsetDateTime,
    ) -> MediaResult<PresignedUpload> {
        Err(MediaError::Disabled)
    }

    fn presign_get(&self, object_key: &str, _now: OffsetDateTime) -> MediaResult<String> {
        Ok(format!("https://objects.example.test/{object_key}"))
    }

    async fn head(
        &self,
        object_key: &str,
        _now: OffsetDateTime,
    ) -> MediaResult<Option<HeadObject>> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .get(object_key)
            .map(|(body, content_type, sha256)| HeadObject {
                size_bytes: body.len() as u64,
                content_type: Some(content_type.clone()),
                upload_marker: None,
                sha256: Some(sha256.clone()),
            }))
    }

    async fn put_if_absent(
        &self,
        object_key: &str,
        body: Vec<u8>,
        content_type: &str,
        sha256: &str,
        _now: OffsetDateTime,
    ) -> MediaResult<HeadObject> {
        let mut objects = self.objects.lock().unwrap();
        match objects.get(object_key) {
            Some((existing_body, existing_type, existing_sha))
                if existing_body == &body
                    && existing_type == content_type
                    && existing_sha == sha256 => {}
            Some(_) => return Err(MediaError::ObjectMismatch),
            None => {
                objects.insert(
                    object_key.to_owned(),
                    (body.clone(), content_type.to_owned(), sha256.to_owned()),
                );
            }
        }
        Ok(HeadObject {
            size_bytes: body.len() as u64,
            content_type: Some(content_type.to_owned()),
            upload_marker: None,
            sha256: Some(sha256.to_owned()),
        })
    }
}

async fn context() -> AppContext {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    AppContext::from_database(database).await.unwrap()
}

async fn app(
    context: AppContext,
    provider: Arc<dyn ResendProvider>,
    media_backend: Arc<MemoryMediaBackend>,
) -> Router {
    let store = LibsqlSessionStore::new(context.clone());
    router_with_store(
        context,
        store,
        false,
        Arc::new(CredentialCipher::new("resend-test-encryption-key").unwrap()),
        CronAuthenticator::new(Some("test-cron-secret")),
        Arc::new(MediaService::for_test(
            media_backend,
            b"resend-test-media-key",
        )),
        Arc::new(ResendService::for_test(WEBHOOK_SECRET, provider)),
    )
}

async fn enable_writes(context: &AppContext) {
    let connection = context.connection().await.unwrap();
    OperationRepository::new(&connection)
        .enable_writes(&utc_now_millis())
        .await
        .unwrap();
}

fn signed_webhook_request(body: impl Into<Body>, signed_body: &[u8]) -> Request<Body> {
    let timestamp = OffsetDateTime::now_utc().unix_timestamp();
    Request::post("/webhooks/resend")
        .header("svix-id", "msg_fixture")
        .header("svix-timestamp", timestamp.to_string())
        .header(
            "svix-signature",
            sign_for_test(WEBHOOK_SECRET, "msg_fixture", timestamp, signed_body),
        )
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.into())
        .unwrap()
}

fn email_event(id: &str, with_attachment: bool) -> Vec<u8> {
    let attachments = if with_attachment {
        serde_json::json!([{
            "id": "attachment_1",
            "filename": "statement.pdf",
            "content_type": "application/pdf"
        }])
    } else {
        serde_json::json!([])
    };
    serde_json::to_vec(&serde_json::json!({
        "type": "email.received",
        "created_at": "2026-07-14T12:00:00Z",
        "data": {
            "email_id": id,
            "from": "sender@example.com",
            "to": ["inbox@example.com"],
            "subject": "Fixture",
            "attachments": attachments
        }
    }))
    .unwrap()
}

fn draft(id: &str) -> InboundEmailDraft {
    InboundEmailDraft {
        resend_email_id: id.to_owned(),
        from_address: "sender@example.com".to_owned(),
        to_addresses: "[\"inbox@example.com\"]".to_owned(),
        subject: Some("Fixture".to_owned()),
        received_at: "2026-07-14T12:00:00Z".to_owned(),
        raw_webhook_payload: "{\"type\":\"email.received\"}".to_owned(),
        attachments: Vec::new(),
    }
}

#[test]
fn production_configuration_requires_both_redacted_secrets() {
    assert!(resolve_service(None, None, true).is_err());
    assert!(resolve_service(Some(WEBHOOK_SECRET.to_owned()), None, true).is_err());
    assert!(!resolve_service(None, None, false).unwrap().is_enabled());
    let service = resolve_service(
        Some(WEBHOOK_SECRET.to_owned()),
        Some("re_fixture_api_key".to_owned()),
        true,
    )
    .unwrap();
    assert!(service.is_enabled());
    assert!(!format!("{service:?}").contains("re_fixture_api_key"));
}

#[test]
fn signatures_require_fresh_headers_and_compare_any_v1_value() {
    let verifier = WebhookVerifier::new(WEBHOOK_SECRET).unwrap();
    let now = OffsetDateTime::now_utc();
    let body = br#"{"type":"email.received"}"#;
    let signature = sign_for_test(WEBHOOK_SECRET, "msg_1", now.unix_timestamp(), body);
    let mut headers = HeaderMap::new();
    headers.insert("svix-id", "msg_1".parse().unwrap());
    headers.insert(
        "svix-timestamp",
        now.unix_timestamp().to_string().parse().unwrap(),
    );
    headers.insert(
        "svix-signature",
        format!("v1,{} {signature}", STANDARD.encode([0_u8; 32]))
            .parse()
            .unwrap(),
    );
    assert_eq!(verifier.verify(&headers, body, now), Ok(()));
    assert_eq!(
        verifier.verify(&headers, b"tampered", now),
        Err(SignatureError::Invalid)
    );
    headers.remove("svix-id");
    assert_eq!(
        verifier.verify(&headers, body, now),
        Err(SignatureError::Missing)
    );
    headers.insert("svix-id", "msg_1".parse().unwrap());
    assert_eq!(
        verifier.verify(&headers, body, now + time::Duration::minutes(6)),
        Err(SignatureError::Stale)
    );
    assert!(!format!("{verifier:?}").contains("MDEyMz"));
}

#[tokio::test]
async fn lease_claims_dedupe_busy_and_stored_replays() {
    let context = context().await;
    let connection = context.connection().await.unwrap();
    let repository = InboundEmailRepository::new(&connection);
    let first = repository
        .claim_webhook(&draft("email_lease_1"), &"a".repeat(43))
        .await
        .unwrap();
    let ClaimOutcome::Acquired(claim) = first else {
        panic!("first attempt must own the lease");
    };
    assert!(matches!(
        repository
            .claim_webhook(&draft("email_lease_1"), &"b".repeat(43))
            .await
            .unwrap(),
        ClaimOutcome::Busy(_)
    ));
    repository
        .record_body(claim.email.id, &claim.lease_token, None, "text/plain")
        .await
        .unwrap();
    repository
        .complete(claim.email.id, &claim.lease_token)
        .await
        .unwrap();
    assert!(matches!(
        repository
            .claim_webhook(&draft("email_lease_1"), &"c".repeat(43))
            .await
            .unwrap(),
        ClaimOutcome::AlreadyStored(_)
    ));
}

#[tokio::test]
async fn stale_lease_can_be_reclaimed_and_old_owner_cannot_finalize() {
    let context = context().await;
    let connection = context.connection().await.unwrap();
    let repository = InboundEmailRepository::new(&connection);
    let ClaimOutcome::Acquired(first) = repository
        .claim_webhook(&draft("email_stale_1"), &"a".repeat(43))
        .await
        .unwrap()
    else {
        panic!("first attempt must own the lease");
    };
    connection
        .execute(
            "UPDATE intg_received_email_processing_lease \
             SET claimed_at = '2000-01-01T00:00:00.000Z' WHERE email_id = ?1",
            libsql::params![first.email.id],
        )
        .await
        .unwrap();
    let ClaimOutcome::Acquired(second) = repository
        .claim_retry(first.email.id, &"b".repeat(43))
        .await
        .unwrap()
    else {
        panic!("stale lease must be reclaimable");
    };
    assert!(
        repository
            .record_body(first.email.id, &first.lease_token, None, "text/plain")
            .await
            .is_err()
    );
    repository
        .record_body(second.email.id, &second.lease_token, None, "text/plain")
        .await
        .unwrap();
    repository
        .complete(second.email.id, &second.lease_token)
        .await
        .unwrap();
}

#[tokio::test]
async fn signature_precedes_database_gate_and_valid_events_obey_read_only() {
    let provider = Arc::new(FakeProvider::default());
    let broken_gate_context = context().await;
    broken_gate_context
        .connection()
        .await
        .unwrap()
        .execute("DROP TABLE operation_control", ())
        .await
        .unwrap();
    let bad_app = app(
        broken_gate_context,
        provider.clone(),
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;
    let body = email_event("email_gate_1", false);
    let bad_signature = Request::post("/webhooks/resend")
        .header("svix-id", "msg_bad")
        .header("svix-timestamp", OffsetDateTime::now_utc().unix_timestamp())
        .header("svix-signature", "v1,AAAA")
        .body(Body::from(body.clone()))
        .unwrap();
    let response = bad_app.oneshot(bad_signature).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(provider.email_calls.load(Ordering::SeqCst), 0);

    let context = context().await;
    let app = app(
        context.clone(),
        provider.clone(),
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;

    let response = app
        .oneshot(signed_webhook_request(body.clone(), &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response.headers().contains_key(header::RETRY_AFTER));
    let connection = context.connection().await.unwrap();
    assert!(
        WorkspaceInboxRepository::new(&connection)
            .list_emails(true)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn signed_bad_json_irrelevant_events_and_replays_are_honest() {
    let context = context().await;
    enable_writes(&context).await;
    let provider = Arc::new(FakeProvider::default());
    let app = app(
        context,
        provider.clone(),
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;

    let bad = b"not-json".to_vec();
    assert_eq!(
        app.clone()
            .oneshot(signed_webhook_request(bad.clone(), &bad))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let ignored = br#"{"type":"email.sent","data":{}}"#.to_vec();
    assert_eq!(
        app.clone()
            .oneshot(signed_webhook_request(ignored.clone(), &ignored))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let body = email_event("email_replay_1", false);
    assert_eq!(
        app.clone()
            .oneshot(signed_webhook_request(body.clone(), &body))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.oneshot(signed_webhook_request(body.clone(), &body))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(provider.email_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_delivery_has_one_owner_and_one_retryable_response() {
    let context = context().await;
    enable_writes(&context).await;
    let provider = Arc::new(BlockingProvider::default());
    let app = app(
        context,
        provider.clone(),
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;
    let body = email_event("email_concurrent_1", false);
    let first_app = app.clone();
    let first_body = body.clone();
    let first = tokio::spawn(async move {
        first_app
            .oneshot(signed_webhook_request(first_body.clone(), &first_body))
            .await
            .unwrap()
    });
    provider.entered.notified().await;

    let duplicate = app
        .oneshot(signed_webhook_request(body.clone(), &body))
        .await
        .unwrap();
    assert_eq!(duplicate.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(duplicate.headers().contains_key(header::RETRY_AFTER));
    provider.release.notify_one();
    assert_eq!(first.await.unwrap().status(), StatusCode::OK);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn partial_failure_is_durable_and_authenticated_retry_finishes_inline() {
    let context = context().await;
    context
        .bootstrap_admin(Some(EMAIL), Some(PASSWORD))
        .await
        .unwrap();
    enable_writes(&context).await;
    let provider = Arc::new(FakeProvider::with_transient_download_failure());
    let app = app(
        context.clone(),
        provider.clone(),
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;
    let body = email_event("email_retry_1", true);
    let first = app
        .clone()
        .oneshot(signed_webhook_request(body.clone(), &body))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(first.headers().contains_key(header::RETRY_AFTER));

    let connection = context.connection().await.unwrap();
    let email = WorkspaceInboxRepository::new(&connection)
        .list_emails(true)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(email.processing_state, "error");
    assert!(!email.error_message.unwrap().contains("fixture attachment"));
    let attachments = WorkspaceInboxRepository::new(&connection)
        .list_attachments(email.id)
        .await
        .unwrap();
    assert_eq!(attachments[0].processing_state, "error");

    let anonymous = app
        .clone()
        .oneshot(
            Request::post(format!("/inbox/{}/retry", email.id))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::SEE_OTHER);
    assert_eq!(anonymous.headers()[header::LOCATION], "/login");

    let login = app
        .clone()
        .oneshot(
            Request::post("/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    "email=admin%40example.com&password=correct+horse+battery+staple",
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = login
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find(|value| value.starts_with("__td_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let retry = app
        .oneshot(
            Request::post(format!("/inbox/{}/retry", email.id))
                .header(header::COOKIE, cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("return_to=detail"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(retry.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        retry.headers()[header::LOCATION],
        format!("/inbox/{}", email.id)
    );
    let connection = context.connection().await.unwrap();
    let stored = WorkspaceInboxRepository::new(&connection)
        .email(email.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.processing_state, "stored");
    assert_eq!(provider.email_calls.load(Ordering::SeqCst), 1);
    assert_eq!(provider.download_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn oversized_webhooks_and_provider_redirects_are_rejected() {
    let context = context().await;
    let app = app(
        context,
        Arc::new(FakeProvider::default()),
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;
    let body = vec![b'x'; 256 * 1_024 + 1];
    let response = app
        .oneshot(signed_webhook_request(body.clone(), &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);

    let server = Router::new()
        .route(
            "/emails/receiving/redirect",
            get(|| async { Redirect::temporary("/emails/receiving/target") }),
        )
        .route(
            "/emails/receiving/large",
            get(|| async { vec![b'x'; 2 * 1_024 * 1_024 + 1].into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, server).await });
    let client = ResendClient::for_test(format!("http://{address}/").parse().unwrap()).unwrap();
    assert_eq!(
        client.get_received_email("redirect").await,
        Err(ProviderError::InvalidResponse)
    );
    assert_eq!(
        client.get_received_email("large").await,
        Err(ProviderError::ResponseTooLarge)
    );
    assert_eq!(
        client
            .download_attachment("http://untrusted.invalid/file")
            .await,
        Err(ProviderError::InvalidDownloadUrl)
    );
    assert_eq!(
        client
            .download_attachment("https://other.resend.com/file")
            .await,
        Err(ProviderError::InvalidDownloadUrl)
    );
    task.abort();
}

#[tokio::test]
async fn permanent_provider_failures_return_only_sanitized_metadata() {
    let context = context().await;
    enable_writes(&context).await;
    let provider = Arc::new(FakeProvider {
        permanent_email_failure: true,
        ..FakeProvider::default()
    });
    let app = app(
        context.clone(),
        provider,
        Arc::new(MemoryMediaBackend::default()),
    )
    .await;
    let body = email_event("email_permanent_1", false);
    let response = app
        .oneshot(signed_webhook_request(body.clone(), &body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let response_body = to_bytes(response.into_body(), 4 * 1_024).await.unwrap();
    let response_body = String::from_utf8(response_body.to_vec()).unwrap();
    assert!(response_body.contains("invalid_provider_response"));
    assert!(!response_body.contains("Safe fixture body"));
    let connection = context.connection().await.unwrap();
    let email = WorkspaceInboxRepository::new(&connection)
        .list_emails(true)
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(email.processing_state, "error");
    assert_eq!(
        email.error_message.as_deref(),
        Some("Email content could not be stored because the provider response was invalid.")
    );
}
