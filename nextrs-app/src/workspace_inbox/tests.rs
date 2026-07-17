use libsql::{Builder, Connection, params};

use super::{
    LoanWorkspaceDraft, LoanWorkspacePhotoDraft, ReceivedEmailAttachmentDraft, ReceivedEmailDraft,
    WorkspaceInboxError, WorkspaceInboxRepository,
};
use crate::db::AppContext;

async fn test_connection() -> Connection {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    AppContext::from_database(database)
        .await
        .unwrap()
        .connection()
        .await
        .unwrap()
}

async fn insert_connection(connection: &Connection, id: i64) {
    connection
        .execute(
            "INSERT INTO intg_integration_connection (id, slug, name, provider) \
             VALUES (?1, ?2, 'The Mortgage Office', 'mortgage_office')",
            params![id, format!("tmo-{id}")],
        )
        .await
        .unwrap();
}

fn workspace(connection_id: i64, loan_account: &str) -> LoanWorkspaceDraft {
    LoanWorkspaceDraft {
        connection_id,
        loan_account: loan_account.to_owned(),
        redfin_url: Some("https://www.redfin.com/example".to_owned()),
        zillow_url: Some("https://www.zillow.com/example".to_owned()),
        decision_status: Some("reviewing".to_owned()),
        target_contribution: Some(12_345.67),
        actual_contribution: None,
        notes: Some("Review title and survey".to_owned()),
    }
}

fn photo(connection_id: i64, loan_account: &str, suffix: &str) -> LoanWorkspacePhotoDraft {
    LoanWorkspacePhotoDraft {
        connection_id,
        loan_account: loan_account.to_owned(),
        provider: "manual_upload".to_owned(),
        caption: Some(format!("Photo {suffix}")),
        source_url: format!("s3://source/{suffix}"),
        image_url: format!("media/workspaces/LN-1/{suffix}.jpg"),
        sort_order: suffix.parse().unwrap_or(0),
    }
}

fn email(provider_id: &str) -> ReceivedEmailDraft {
    ReceivedEmailDraft {
        resend_email_id: provider_id.to_owned(),
        from_address: "sender@example.com".to_owned(),
        to_addresses: "[\"trust@example.com\",\"ops@example.com\"]".to_owned(),
        subject: Some("Funding notice".to_owned()),
        received_at: "2026-07-14T18:20:30.456Z".to_owned(),
        raw_webhook_payload: Some(
            "{\"type\":\"email.received\",\"data\":{\"unicode\":\"snowman ☃\"}}".to_owned(),
        ),
    }
}

#[tokio::test]
async fn production_migration_enforces_domains_foreign_keys_and_cascades() {
    let connection = test_connection().await;
    let mut ledger = connection
        .query("SELECT name FROM _schema_migrations WHERE version = 4", ())
        .await
        .unwrap();
    assert_eq!(
        ledger
            .next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "workspaces_inbox"
    );

    assert!(
        connection
            .execute(
                "INSERT INTO intg_loan_workspace (connection_id, loan_account) \
                 VALUES (999, 'LN-MISSING')",
                (),
            )
            .await
            .is_err()
    );
    insert_connection(&connection, 11).await;
    assert!(
        connection
            .execute(
                "INSERT INTO intg_loan_workspace (connection_id, loan_account, decision_status) \
                 VALUES (11, 'LN-BAD', 'maybe')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_loan_workspace ( \
                    connection_id, loan_account, target_contribution \
                 ) VALUES (11, 'LN-INFINITE', 1.0e999)",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_received_email ( \
                    resend_email_id, from_address, to_addresses, received_at \
                 ) VALUES ('bad-json', 'a@example.com', 'not-json', \
                           '2026-07-14T18:20:30Z')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_received_email ( \
                    resend_email_id, from_address, to_addresses, received_at \
                 ) VALUES ('bad-time', 'a@example.com', '[]', '07/14/2026')",
                (),
            )
            .await
            .is_err()
    );

    connection
        .execute(
            "INSERT INTO intg_loan_workspace (id, connection_id, loan_account) \
             VALUES (21, 11, 'LN-1')",
            (),
        )
        .await
        .unwrap();
    connection
        .execute(
            "INSERT INTO intg_loan_workspace_photo ( \
                id, connection_id, loan_account, provider, source_url, image_url \
             ) VALUES (31, 11, 'LN-1', 'manual_upload', 's3://source', 'objects/key')",
            (),
        )
        .await
        .unwrap();
    connection
        .execute("DELETE FROM intg_integration_connection WHERE id = 11", ())
        .await
        .unwrap();
    for table in ["intg_loan_workspace", "intg_loan_workspace_photo"] {
        let query = format!("SELECT COUNT(*) FROM {table}");
        let mut rows = connection.query(&query, ()).await.unwrap();
        assert_eq!(
            rows.next().await.unwrap().unwrap().get::<i64>(0).unwrap(),
            0
        );
    }

    connection
        .execute(
            "INSERT INTO intg_received_email ( \
                id, resend_email_id, from_address, to_addresses, received_at \
             ) VALUES (41, 'email-41', 'a@example.com', '[]', '2026-07-14T18:20:30Z')",
            (),
        )
        .await
        .unwrap();
    connection
        .execute(
            "INSERT INTO intg_received_email_attachment ( \
                id, email_id, resend_attachment_id, filename, content_type \
             ) VALUES (51, 41, 'att-51', 'wire.pdf', 'application/pdf')",
            (),
        )
        .await
        .unwrap();
    connection
        .execute("DELETE FROM intg_received_email WHERE id = 41", ())
        .await
        .unwrap();
    let mut attachments = connection
        .query("SELECT COUNT(*) FROM intg_received_email_attachment", ())
        .await
        .unwrap();
    assert_eq!(
        attachments
            .next()
            .await
            .unwrap()
            .unwrap()
            .get::<i64>(0)
            .unwrap(),
        0
    );

    let mut foreign_keys = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .unwrap();
    assert!(foreign_keys.next().await.unwrap().is_none());
}

#[tokio::test]
async fn typed_reads_preserve_ids_timestamps_payloads_and_object_keys_exactly() {
    let connection = test_connection().await;
    insert_connection(&connection, 101).await;
    let raw = "{\"nested\":{\"line\":\"one\\ntwo\",\"glyph\":\"\u{1f512}\"}}";
    let recipients = "[ \"one@example.com\", \"two+tag@example.com\" ]";
    let body_key = "emails/provider-201/body snowman-☃.html";
    let attachment_key = "emails/provider-201/attachments/wire + instructions 🔒.pdf";
    let image_url = "media/loan-workspace/LN-☃/front + side.jpg";
    connection
        .execute(
            "INSERT INTO intg_loan_workspace ( \
                id, connection_id, loan_account, redfin_url, decision_status, \
                target_contribution, notes, created_at, updated_at \
             ) VALUES ( \
                301, 101, 'LN-☃', 'https://redfin.invalid/a?b=1&c=2', 'committed', \
                25000.125, 'line one\nline two', '2025-01-02T03:04:05.006Z', \
                '2026-07-14T18:20:30.456Z' \
             )",
            (),
        )
        .await
        .unwrap();
    connection
        .execute(
            "INSERT INTO intg_loan_workspace_photo ( \
                id, connection_id, loan_account, provider, caption, source_url, image_url, \
                sort_order, is_featured, created_at \
             ) VALUES (401, 101, 'LN-☃', 'upload', 'front 🏠', \
                       's3://opaque/source key', ?1, -4, 1, \
                       '2026-07-14T18:20:30.456Z')",
            params![image_url],
        )
        .await
        .unwrap();
    connection
        .execute(
            "INSERT INTO intg_received_email ( \
                id, resend_email_id, from_address, to_addresses, subject, received_at, \
                body_s3_key, body_content_type, loan_account, processing_state, \
                raw_webhook_payload, created_at, updated_at \
             ) VALUES (501, 'provider-201', 'from+☃@example.com', ?1, 'Wire 🔒', \
                       '2026-07-14T18:20:30.456Z', ?2, 'text/html', 'LN-☃', 'stored', \
                       ?3, '2026-07-14T18:21:00.000Z', '2026-07-14T18:22:00.000Z')",
            params![recipients, body_key, raw],
        )
        .await
        .unwrap();
    connection
        .execute(
            "INSERT INTO intg_received_email_attachment ( \
                id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                s3_key, processing_state, created_at \
             ) VALUES (601, 501, 'provider-att-1', 'wire 🔒.pdf', 'application/pdf', \
                       987654, ?1, 'stored', '2026-07-14T18:21:00.000Z')",
            params![attachment_key],
        )
        .await
        .unwrap();

    let repository = WorkspaceInboxRepository::new(&connection);
    let workspace = repository.workspace(101, "LN-☃").await.unwrap().unwrap();
    assert_eq!(workspace.id, 301);
    assert_eq!(workspace.created_at, "2025-01-02T03:04:05.006Z");
    assert_eq!(workspace.updated_at, "2026-07-14T18:20:30.456Z");
    assert_eq!(workspace.notes.as_deref(), Some("line one\nline two"));

    let photo = repository.list_photos(101, "LN-☃").await.unwrap().remove(0);
    assert_eq!(photo.id, 401);
    assert_eq!(photo.image_url.as_bytes(), image_url.as_bytes());
    assert!(photo.is_featured);

    let detail = repository.email_detail(501).await.unwrap().unwrap();
    assert_eq!(detail.email.id, 501);
    assert_eq!(detail.email.to_addresses.as_bytes(), recipients.as_bytes());
    assert_eq!(detail.email.body_s3_key.as_deref(), Some(body_key));
    assert_eq!(
        detail
            .email
            .raw_webhook_payload
            .as_deref()
            .unwrap()
            .as_bytes(),
        raw.as_bytes()
    );
    assert_eq!(detail.attachments[0].id, 601);
    assert_eq!(
        detail.attachments[0].s3_key.as_deref().unwrap().as_bytes(),
        attachment_key.as_bytes()
    );
}

#[tokio::test]
async fn workspace_upsert_is_atomic_and_retains_source_identity() {
    let connection = test_connection().await;
    insert_connection(&connection, 7).await;
    let repository = WorkspaceInboxRepository::new(&connection);

    let first = repository
        .upsert_workspace(&workspace(7, "LN-700"))
        .await
        .unwrap();
    let mut invalid = workspace(7, "LN-700");
    invalid.notes = Some("must not be written".to_owned());
    invalid.decision_status = Some("unknown".to_owned());
    assert!(matches!(
        repository.upsert_workspace(&invalid).await.unwrap_err(),
        WorkspaceInboxError::Validation(_)
    ));
    let mut non_finite = workspace(7, "LN-700");
    non_finite.target_contribution = Some(f64::INFINITY);
    assert!(matches!(
        repository.upsert_workspace(&non_finite).await.unwrap_err(),
        WorkspaceInboxError::Validation(_)
    ));
    let mut unsafe_url = workspace(7, "LN-700");
    unsafe_url.redfin_url = Some("javascript:alert(1)".to_owned());
    assert!(matches!(
        repository.upsert_workspace(&unsafe_url).await.unwrap_err(),
        WorkspaceInboxError::Validation(_)
    ));
    let mut negative = workspace(7, "LN-700");
    negative.target_contribution = Some(-1.0);
    assert!(matches!(
        repository.upsert_workspace(&negative).await.unwrap_err(),
        WorkspaceInboxError::Validation(_)
    ));
    let unchanged = repository.workspace(7, "LN-700").await.unwrap().unwrap();
    assert_eq!(unchanged.notes, first.notes);
    assert_eq!(unchanged.id, first.id);
    assert_eq!(unchanged.created_at, first.created_at);

    let mut update = workspace(7, "LN-700");
    update.decision_status = Some("funded".to_owned());
    update.actual_contribution = Some(11_000.25);
    update.notes = Some("closed".to_owned());
    let updated = repository.upsert_workspace(&update).await.unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.created_at, first.created_at);
    assert_eq!(updated.decision_status.as_deref(), Some("funded"));
    assert_eq!(updated.actual_contribution, Some(11_000.25));
    assert_eq!(repository.list_workspaces(7).await.unwrap().len(), 1);
}

#[tokio::test]
async fn photo_metadata_supports_legacy_orphans_and_exactly_one_featured_photo() {
    let connection = test_connection().await;
    insert_connection(&connection, 8).await;
    let repository = WorkspaceInboxRepository::new(&connection);

    // Legacy uploads were allowed for an imported loan before a workspace row
    // existed. The target preserves that source behavior.
    let first = repository
        .create_photo_metadata(&photo(8, "LN-ORPHAN", "1"))
        .await
        .unwrap();
    let second = repository
        .create_photo_metadata(&photo(8, "LN-ORPHAN", "2"))
        .await
        .unwrap();
    repository
        .set_featured_photo(8, "LN-ORPHAN", first.id)
        .await
        .unwrap();
    repository
        .set_featured_photo(8, "LN-ORPHAN", second.id)
        .await
        .unwrap();
    assert!(matches!(
        repository
            .set_featured_photo(8, "LN-ORPHAN", 9999)
            .await
            .unwrap_err(),
        WorkspaceInboxError::NotFound(_)
    ));

    let photos = repository.list_photos(8, "LN-ORPHAN").await.unwrap();
    assert_eq!(photos.iter().filter(|photo| photo.is_featured).count(), 1);
    assert_eq!(
        photos.iter().find(|photo| photo.is_featured).unwrap().id,
        second.id
    );

    let deleted = repository
        .delete_photo_metadata(8, "LN-ORPHAN", first.id)
        .await
        .unwrap();
    assert_eq!(deleted.image_url, "media/workspaces/LN-1/1.jpg");
    assert!(
        repository
            .photo(8, "LN-ORPHAN", first.id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn direct_upload_finalization_is_idempotent_and_allocates_sort_order_atomically() {
    let connection = test_connection().await;
    insert_connection(&connection, 9).await;
    let repository = WorkspaceInboxRepository::new(&connection);
    repository
        .create_photo_metadata(&photo(9, "LN-9", "1"))
        .await
        .unwrap();

    let first = repository
        .create_manual_photo_metadata(
            9,
            "LN-9",
            Some("Front elevation.jpg"),
            "/media/loan-workspace/LN-9/manual-one.jpg",
        )
        .await
        .unwrap();
    let replay = repository
        .create_manual_photo_metadata(
            9,
            "LN-9",
            Some("Front elevation.jpg"),
            "/media/loan-workspace/LN-9/manual-one.jpg",
        )
        .await
        .unwrap();
    assert_eq!(first.id, replay.id);
    assert_eq!(first.sort_order, 2);
    assert_eq!(repository.list_photos(9, "LN-9").await.unwrap().len(), 2);
}

#[tokio::test]
async fn email_webhook_replay_is_idempotent_and_never_resets_processing_state() {
    let connection = test_connection().await;
    let repository = WorkspaceInboxRepository::new(&connection);
    let draft = email("provider-email-1");

    let first = repository.upsert_received_email(&draft).await.unwrap();
    assert!(first.inserted);
    repository
        .mark_email_error(first.email.id, "provider temporarily unavailable")
        .await
        .unwrap();

    let mut replay_payload_changed = draft.clone();
    replay_payload_changed.subject = Some("A replay must not overwrite this".to_owned());
    let replay = repository
        .upsert_received_email(&replay_payload_changed)
        .await
        .unwrap();
    assert!(!replay.inserted);
    assert_eq!(replay.email.id, first.email.id);
    assert_eq!(replay.email.processing_state, "error");
    assert_eq!(replay.email.subject, draft.subject);

    repository
        .reset_email_for_retry(first.email.id)
        .await
        .unwrap();
    let stored = repository
        .mark_email_body_stored(
            first.email.id,
            "emails/provider-email-1/body.html",
            "text/html",
        )
        .await
        .unwrap();
    assert_eq!(stored.processing_state, "stored");
    let replay = repository.upsert_received_email(&draft).await.unwrap();
    assert!(!replay.inserted);
    assert_eq!(replay.email.processing_state, "stored");
    assert_eq!(
        replay.email.body_s3_key.as_deref(),
        Some("emails/provider-email-1/body.html")
    );
}

#[tokio::test]
async fn attachment_conflict_returns_the_existing_row() {
    let connection = test_connection().await;
    let repository = WorkspaceInboxRepository::new(&connection);
    let email_id = repository
        .upsert_received_email(&email("provider-email-2"))
        .await
        .unwrap()
        .email
        .id;
    let draft = ReceivedEmailAttachmentDraft {
        email_id,
        resend_attachment_id: "provider-attachment-1".to_owned(),
        filename: "original.pdf".to_owned(),
        content_type: "application/pdf".to_owned(),
    };
    let first = repository.create_attachment_metadata(&draft).await.unwrap();
    assert!(first.inserted);

    let mut replay = draft.clone();
    replay.filename = "changed-by-replay.pdf".to_owned();
    replay.content_type = "application/octet-stream".to_owned();
    let existing = repository
        .create_attachment_metadata(&replay)
        .await
        .unwrap();
    assert!(!existing.inserted);
    assert_eq!(existing.attachment.id, first.attachment.id);
    assert_eq!(existing.attachment.filename, "original.pdf");
    assert_eq!(existing.attachment.content_type, "application/pdf");
}

#[tokio::test]
async fn email_link_retry_attachment_and_delete_transitions_are_checked() {
    let connection = test_connection().await;
    let repository = WorkspaceInboxRepository::new(&connection);
    let email_id = repository
        .upsert_received_email(&email("provider-email-3"))
        .await
        .unwrap()
        .email
        .id;
    let attachment = repository
        .create_attachment_metadata(&ReceivedEmailAttachmentDraft {
            email_id,
            resend_attachment_id: "provider-attachment-3".to_owned(),
            filename: "wire.pdf".to_owned(),
            content_type: "application/pdf".to_owned(),
        })
        .await
        .unwrap()
        .attachment;

    repository.link_email(email_id, "LN-1").await.unwrap();
    assert!(matches!(
        repository.link_email(email_id, "LN-2").await.unwrap_err(),
        WorkspaceInboxError::Conflict(_)
    ));
    assert_eq!(repository.list_emails(false).await.unwrap().len(), 0);
    assert_eq!(
        repository.list_emails_for_loan("LN-1").await.unwrap().len(),
        1
    );
    repository.unlink_email(email_id).await.unwrap();
    assert_eq!(repository.list_emails(false).await.unwrap().len(), 1);

    repository
        .mark_attachment_error(attachment.id)
        .await
        .unwrap();
    repository
        .reset_attachment_for_retry(attachment.id)
        .await
        .unwrap();
    let stored_attachment = repository
        .mark_attachment_stored(
            attachment.id,
            "emails/provider-email-3/attachments/wire.pdf",
            4096,
        )
        .await
        .unwrap();
    assert_eq!(stored_attachment.processing_state, "stored");
    assert!(matches!(
        repository
            .mark_attachment_error(attachment.id)
            .await
            .unwrap_err(),
        WorkspaceInboxError::Conflict(_)
    ));

    let deleted = repository.delete_email_metadata(email_id).await.unwrap();
    assert_eq!(deleted.email.resend_email_id, "provider-email-3");
    assert_eq!(deleted.attachments, vec![stored_attachment]);
    assert!(repository.email(email_id).await.unwrap().is_none());
    assert!(
        repository
            .list_attachments(email_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn browser_link_is_bounded_and_requires_an_active_imported_tmo_loan_atomically() {
    let connection = test_connection().await;
    connection
        .execute_batch(
            "INSERT INTO intg_integration_connection ( \
                id, slug, name, provider, status \
             ) VALUES (91, 'tmo', 'The Mortgage Office', 'mortgage_office', 'degraded'); \
             INSERT INTO intg_tmo_import_loan ( \
                connection_id, loan_account, is_active \
             ) VALUES (91, 'LN-ACTIVE', 1), (91, 'LN-INACTIVE', 0);",
        )
        .await
        .unwrap();
    let repository = WorkspaceInboxRepository::new(&connection);

    let linked_id = repository
        .upsert_received_email(&email("provider-browser-link-1"))
        .await
        .unwrap()
        .email
        .id;
    let linked = repository
        .link_email_to_imported_tmo_loan(linked_id, "LN-ACTIVE")
        .await
        .unwrap();
    assert_eq!(linked.loan_account.as_deref(), Some("LN-ACTIVE"));

    let rejected_id = repository
        .upsert_received_email(&email("provider-browser-link-2"))
        .await
        .unwrap()
        .email
        .id;
    assert!(matches!(
        repository
            .link_email_to_imported_tmo_loan(rejected_id, "LN-INACTIVE")
            .await
            .unwrap_err(),
        WorkspaceInboxError::NotFound(_)
    ));
    assert!(
        repository
            .email(rejected_id)
            .await
            .unwrap()
            .unwrap()
            .loan_account
            .is_none()
    );

    assert!(matches!(
        repository
            .link_email_to_imported_tmo_loan(rejected_id, &"x".repeat(129))
            .await
            .unwrap_err(),
        WorkspaceInboxError::Validation(_)
    ));
    assert!(matches!(
        repository
            .link_email_to_imported_tmo_loan(rejected_id, " LN-ACTIVE")
            .await
            .unwrap_err(),
        WorkspaceInboxError::Validation(_)
    ));

    connection
        .execute(
            "UPDATE intg_integration_connection SET status = 'paused' WHERE id = 91",
            (),
        )
        .await
        .unwrap();
    assert!(matches!(
        repository
            .link_email_to_imported_tmo_loan(rejected_id, "LN-ACTIVE")
            .await
            .unwrap_err(),
        WorkspaceInboxError::NotFound(_)
    ));
    assert!(
        repository
            .email(rejected_id)
            .await
            .unwrap()
            .unwrap()
            .loan_account
            .is_none()
    );
}

#[tokio::test]
async fn read_failures_are_returned_instead_of_becoming_empty_results() {
    let connection = test_connection().await;
    connection
        .execute("DROP TABLE intg_received_email_attachment", ())
        .await
        .unwrap();
    let error = WorkspaceInboxRepository::new(&connection)
        .list_attachments(1)
        .await
        .unwrap_err();
    assert!(matches!(error, WorkspaceInboxError::Storage(_)));
}
