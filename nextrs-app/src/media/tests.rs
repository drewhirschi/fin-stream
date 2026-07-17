use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::{
    extract::{Extension, Path},
    http::{StatusCode, header},
};
use libsql::Builder;
use time::OffsetDateTime;

use super::{
    HeadObject, MediaError, MediaResult, MediaService, PhotoLocation, PresignedUpload,
    UploadIntentDraft, WorkspaceFormValues, WorkspacePhotoView, classify_photo, photo_route_url,
    safe_external_url, service::MediaBackend,
};
use crate::workspace_inbox::{LoanWorkspace, LoanWorkspacePhoto};
use crate::{db::AppContext, media::http};

#[derive(Default)]
struct FakeBackend {
    head: Mutex<Option<HeadObject>>,
}

#[async_trait]
impl MediaBackend for FakeBackend {
    fn presign_put(
        &self,
        _object_key: &str,
        content_type: &str,
        _size_bytes: u64,
        upload_marker: &str,
        _now: OffsetDateTime,
    ) -> MediaResult<PresignedUpload> {
        Ok(PresignedUpload {
            method: "PUT",
            url: "https://objects.example.invalid/upload?signature=redacted".to_owned(),
            headers: [
                ("content-type".to_owned(), content_type.to_owned()),
                (
                    "x-amz-meta-trust-deeds-upload".to_owned(),
                    upload_marker.to_owned(),
                ),
            ]
            .into_iter()
            .collect(),
        })
    }

    fn presign_get(&self, object_key: &str, _now: OffsetDateTime) -> MediaResult<String> {
        Ok(format!(
            "https://objects.example.invalid/{object_key}?signature=redacted"
        ))
    }

    async fn head(
        &self,
        _object_key: &str,
        _now: OffsetDateTime,
    ) -> MediaResult<Option<HeadObject>> {
        Ok(self.head.lock().unwrap().clone())
    }

    async fn put_if_absent(
        &self,
        _object_key: &str,
        body: Vec<u8>,
        content_type: &str,
        sha256: &str,
        _now: OffsetDateTime,
    ) -> MediaResult<HeadObject> {
        Ok(HeadObject {
            size_bytes: body.len() as u64,
            content_type: Some(content_type.to_owned()),
            upload_marker: None,
            sha256: Some(sha256.to_owned()),
        })
    }
}

fn upload_draft() -> UploadIntentDraft {
    UploadIntentDraft {
        connection_id: 7,
        loan_account: "LN 1/2".to_owned(),
        file_name: "Front elevation.jpg".to_owned(),
        content_type: "image/jpeg".to_owned(),
        size_bytes: 12_345,
    }
}

#[test]
fn legacy_photo_locations_share_one_canonical_namespace() {
    for location in [
        "/media/loan-workspace/LN-1/front.jpg",
        "/static/loan-images/LN-1/front.jpg",
        "https://legacy.example/media/loan-workspace/LN-1/front.jpg",
    ] {
        assert_eq!(
            classify_photo(location),
            Ok(PhotoLocation::Stored(
                "loan-workspace/LN-1/front.jpg".to_owned()
            ))
        );
    }
    assert_eq!(
        photo_route_url("loan-workspace/loan 1/front + side.jpg").unwrap(),
        "/media/loan-workspace/loan%201/front%20%2B%20side.jpg"
    );

    for location in [
        "https://bucket.s3.us-west-2.amazonaws.com/loan-workspace/LN-1/front.jpg",
        "https://s3.us-west-2.amazonaws.com/bucket/loan-workspace/LN-1/front.jpg",
        "https://account.r2.cloudflarestorage.com/bucket/loan-workspace/LN-1/front.jpg",
        "s3://bucket/loan-workspace/LN-1/front.jpg",
    ] {
        assert_eq!(
            classify_photo(location),
            Ok(PhotoLocation::Stored(
                "loan-workspace/LN-1/front.jpg".to_owned()
            )),
            "failed to classify {location}"
        );
    }
}

#[test]
fn unsafe_and_ambiguous_object_locations_are_rejected() {
    for location in [
        "/media/loan-workspace/../secret",
        "/media/loan-workspace/%2e%2e/secret",
        "/media/loan-workspace/%2Fetc/passwd",
        "/media/loan-workspace/a%252Fb",
        "/media/loan-workspace//front.jpg",
        "/media/loan-workspace/front.jpg?token=secret",
        "javascript:alert(1)",
    ] {
        assert!(classify_photo(location).is_err(), "accepted {location}");
    }
}

#[test]
fn external_links_require_http_without_embedded_credentials() {
    assert!(safe_external_url("https://www.redfin.com/example").is_some());
    assert!(safe_external_url("javascript:alert(1)").is_none());
    assert!(safe_external_url("https://user:pass@example.com/private").is_none());
}

#[tokio::test]
async fn signed_intent_is_scoped_and_finalizes_only_matching_object_metadata() {
    let backend = Arc::new(FakeBackend::default());
    let media = MediaService::for_test(backend.clone(), b"test-intent-secret");
    let intent = media.create_upload_intent(upload_draft()).unwrap();
    assert!(
        intent
            .object_key
            .starts_with("loan-workspace/LN-1-2/manual-")
    );
    assert!(
        intent
            .image_url
            .starts_with("/media/loan-workspace/LN-1-2/")
    );
    assert!(!intent.upload.url.contains("test-intent-secret"));
    assert_eq!(intent.upload.method, "PUT");
    let marker = intent.upload.headers["x-amz-meta-trust-deeds-upload"].clone();
    *backend.head.lock().unwrap() = Some(HeadObject {
        size_bytes: 12_345,
        content_type: Some("image/jpeg".to_owned()),
        upload_marker: Some(marker),
        sha256: None,
    });

    assert_eq!(
        media
            .verify_uploaded_intent(&intent.token, 7, "another-loan")
            .await,
        Err(MediaError::InvalidIntent)
    );
    let mut tampered = intent.token.clone();
    tampered.push('x');
    assert_eq!(
        media.verify_uploaded_intent(&tampered, 7, "LN 1/2").await,
        Err(MediaError::InvalidIntent)
    );
    let verified = media
        .verify_uploaded_intent(&intent.token, 7, "LN 1/2")
        .await
        .unwrap();
    assert_eq!(verified.object_key, intent.object_key);
    assert_eq!(verified.size_bytes, 12_345);

    backend.head.lock().unwrap().as_mut().unwrap().size_bytes = 12_346;
    assert_eq!(
        media
            .verify_uploaded_intent(&intent.token, 7, "LN 1/2")
            .await,
        Err(MediaError::ObjectMismatch)
    );
}

#[test]
fn uploads_are_bounded_and_content_typed() {
    let media = MediaService::for_test(Arc::new(FakeBackend::default()), b"test-secret");
    let mut draft = upload_draft();
    draft.size_bytes = 25 * 1_024 * 1_024 + 1;
    assert_eq!(
        media.create_upload_intent(draft),
        Err(MediaError::InvalidInput)
    );
    let mut draft = upload_draft();
    draft.content_type = "image/svg+xml".to_owned();
    assert_eq!(
        media.create_upload_intent(draft),
        Err(MediaError::InvalidInput)
    );
    assert_eq!(
        MediaService::disabled().presign_download("emails/one/body.html"),
        Err(MediaError::Disabled)
    );
}

#[test]
fn template_views_never_render_untrusted_links_or_external_image_hosts() {
    let workspace = LoanWorkspace {
        id: 1,
        connection_id: 2,
        loan_account: "LN-1".to_owned(),
        redfin_url: Some("javascript:alert(1)".to_owned()),
        zillow_url: Some("https://www.zillow.com/example".to_owned()),
        decision_status: None,
        target_contribution: None,
        actual_contribution: None,
        notes: None,
        created_at: "2026-07-14T00:00:00Z".to_owned(),
        updated_at: "2026-07-14T00:00:00Z".to_owned(),
    };
    let form = WorkspaceFormValues::from(Some(&workspace));
    assert!(form.redfin_link.is_none());
    assert!(form.zillow_link.is_some());

    let photo = WorkspacePhotoView::from(LoanWorkspacePhoto {
        id: 3,
        connection_id: 2,
        loan_account: "LN-1".to_owned(),
        provider: "legacy".to_owned(),
        caption: None,
        source_url: "javascript:alert(1)".to_owned(),
        image_url: "https://third-party.example/tracker.jpg".to_owned(),
        sort_order: 0,
        is_featured: false,
        created_at: "2026-07-14T00:00:00Z".to_owned(),
    });
    assert!(photo.source_url.is_none());
    assert!(photo.image_url.is_none());
}

#[tokio::test]
async fn signed_reads_require_an_exact_durable_database_reference() {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    let context = AppContext::from_database(database).await.unwrap();
    let connection = context.connection().await.unwrap();
    connection
        .execute_batch(
            "INSERT INTO intg_integration_connection (id, slug, name, provider) \
             VALUES (1, 'tmo', 'The Mortgage Office', 'mortgage_office'); \
             INSERT INTO intg_loan_workspace_photo ( \
                connection_id, loan_account, provider, source_url, image_url \
             ) VALUES ( \
                1, 'LN-1', 'manual', 'manual-upload', \
                '/media/loan-workspace/LN-1/front.jpg' \
             ); \
             INSERT INTO intg_received_email ( \
                id, resend_email_id, from_address, to_addresses, received_at, body_s3_key \
             ) VALUES ( \
                2, 'email-2', 'sender@example.com', '[]', \
                '2026-07-14T12:00:00Z', 'emails/email-2/body.html' \
             );",
        )
        .await
        .unwrap();
    let media = Arc::new(MediaService::for_test(
        Arc::new(FakeBackend::default()),
        b"test-secret",
    ));

    let photo = http::serve_loan_photo(
        Extension(context.clone()),
        Extension(media.clone()),
        Path("LN-1/front.jpg".to_owned()),
    )
    .await;
    assert_eq!(photo.status(), StatusCode::TEMPORARY_REDIRECT);
    assert!(
        photo.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .contains("loan-workspace/LN-1/front.jpg")
    );

    let unreferenced = http::serve_loan_photo(
        Extension(context.clone()),
        Extension(media.clone()),
        Path("LN-1/private.jpg".to_owned()),
    )
    .await;
    assert_eq!(unreferenced.status(), StatusCode::NOT_FOUND);

    let email = http::serve_email_object(
        Extension(context.clone()),
        Extension(media.clone()),
        Path("emails/email-2/body.html".to_owned()),
    )
    .await;
    assert_eq!(email.status(), StatusCode::TEMPORARY_REDIRECT);

    let traversal = http::serve_email_object(
        Extension(context),
        Extension(media),
        Path("emails/../secret".to_owned()),
    )
    .await;
    assert_eq!(traversal.status(), StatusCode::NOT_FOUND);
}
