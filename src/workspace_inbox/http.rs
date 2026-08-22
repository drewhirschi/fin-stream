use std::fmt::Display;

use axum::{
    Form,
    extract::{Extension, Path, Query},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;

use crate::{
    db::AppContext,
    integrations::{IntegrationRepository, TmoLoanListItem},
    templates::{self, InboxEmailDetailTemplate, InboxEmailPanelTemplate, InboxTemplate},
};

use super::{WorkspaceInboxError, WorkspaceInboxRepository};

const TMO_SLUG: &str = "tmo";
const TMO_PROVIDER: &str = "mortgage_office";

#[derive(Debug, Default, Deserialize)]
pub struct InboxQuery {
    #[serde(default)]
    pub show_linked: bool,
}

#[derive(Debug, Deserialize)]
pub struct LinkEmailForm {
    pub loan_account: String,
    #[serde(default)]
    pub return_to: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct InboxActionForm {
    #[serde(default)]
    pub return_to: Option<String>,
}

pub async fn inbox_page(
    Extension(context): Extension<AppContext>,
    Query(query): Query<InboxQuery>,
) -> Response {
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return page_storage_error("open Inbox database", error),
    };
    let repository = WorkspaceInboxRepository::new(&connection);
    let emails = match repository.list_inbox_items(query.show_linked).await {
        Ok(emails) => emails,
        Err(error) => return page_storage_error("load Inbox emails", error),
    };
    let loans = match active_tmo_loans(&connection).await {
        Ok(loans) => loans,
        Err(response) => return response,
    };

    templates::inbox_response(InboxTemplate {
        title: "Trust Deeds - Inbox".into(),
        emails,
        loans,
        show_linked: query.show_linked,
    })
}

pub async fn email_detail_page(
    Extension(context): Extension<AppContext>,
    Path(email_id): Path<i64>,
) -> Response {
    if email_id <= 0 {
        return templates::not_found_response();
    }
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return page_storage_error("open Inbox email database", error),
    };
    let detail = match WorkspaceInboxRepository::new(&connection)
        .email_detail(email_id)
        .await
    {
        Ok(Some(detail)) => detail,
        Ok(None) => return templates::not_found_response(),
        Err(error) => return page_storage_error("load Inbox email", error),
    };
    let recipients = match recipient_addresses(&detail.email.to_addresses) {
        Ok(recipients) => recipients,
        Err(error) => return page_storage_error("decode Inbox recipients", error),
    };
    let loans = match active_tmo_loans(&connection).await {
        Ok(loans) => loans,
        Err(response) => return response,
    };
    let subject = detail
        .email
        .subject
        .as_deref()
        .filter(|subject| !subject.is_empty())
        .unwrap_or("(no subject)");

    templates::inbox_email_detail_response(InboxEmailDetailTemplate {
        title: format!("Trust Deeds - {subject}"),
        email: detail.email,
        attachments: detail.attachments,
        recipients,
        loans,
    })
}

pub async fn email_panel(
    Extension(context): Extension<AppContext>,
    Path(email_id): Path<i64>,
) -> Response {
    if email_id <= 0 {
        return panel_not_found();
    }
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return panel_storage_error("open Inbox panel database", error),
    };
    let detail = match WorkspaceInboxRepository::new(&connection)
        .email_detail(email_id)
        .await
    {
        Ok(Some(detail)) => detail,
        Ok(None) => return panel_not_found(),
        Err(error) => return panel_storage_error("load Inbox panel", error),
    };
    let recipients = match recipient_addresses(&detail.email.to_addresses) {
        Ok(recipients) => recipients,
        Err(error) => return panel_storage_error("decode Inbox panel recipients", error),
    };
    templates::inbox_email_panel_response(InboxEmailPanelTemplate {
        email: detail.email,
        attachments: detail.attachments,
        recipients,
    })
}

pub async fn link_email(
    Extension(context): Extension<AppContext>,
    Path(email_id): Path<i64>,
    Form(form): Form<LinkEmailForm>,
) -> Response {
    if email_id <= 0 {
        return action_error(StatusCode::NOT_FOUND, "Email not found.");
    }
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return action_storage_error("open email link database", error),
    };
    match WorkspaceInboxRepository::new(&connection)
        .link_email_to_imported_tmo_loan(email_id, &form.loan_account)
        .await
    {
        Ok(_) => action_redirect(email_id, form.return_to.as_deref()),
        Err(error) => action_repository_error("link Inbox email", error),
    }
}

pub async fn unlink_email(
    Extension(context): Extension<AppContext>,
    Path(email_id): Path<i64>,
    Form(form): Form<InboxActionForm>,
) -> Response {
    if email_id <= 0 {
        return action_error(StatusCode::NOT_FOUND, "Email not found.");
    }
    let connection = match context.connection().await {
        Ok(connection) => connection,
        Err(error) => return action_storage_error("open email unlink database", error),
    };
    match WorkspaceInboxRepository::new(&connection)
        .unlink_email(email_id)
        .await
    {
        Ok(_) => action_redirect(email_id, form.return_to.as_deref()),
        Err(error) => action_repository_error("unlink Inbox email", error),
    }
}

async fn active_tmo_loans(
    connection: &libsql::Connection,
) -> Result<Vec<TmoLoanListItem>, Response> {
    let repository = IntegrationRepository::new(connection);
    let integration = match repository.connection_by_slug(TMO_SLUG).await {
        Ok(Some(integration)) => integration,
        Ok(None) => return Ok(Vec::new()),
        Err(error) => return Err(page_storage_error("load Inbox TMO connection", error)),
    };
    if integration.provider != TMO_PROVIDER
        || !matches!(integration.status.as_str(), "active" | "degraded" | "error")
    {
        tracing::error!(
            slug = %integration.slug,
            provider = %integration.provider,
            status = %integration.status,
            "Inbox rejected an unsupported integration identity or state"
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not load this page.",
        )
            .into_response());
    }
    repository
        .list_active_tmo_loan_views(integration.id)
        .await
        .map_err(|error| page_storage_error("load Inbox TMO loans", error))
}

fn recipient_addresses(raw: &str) -> anyhow::Result<Vec<String>> {
    let values: Vec<serde_json::Value> = serde_json::from_str(raw)?;
    values
        .into_iter()
        .map(|value| match value {
            serde_json::Value::String(address) => Ok(address),
            _ => anyhow::bail!("Inbox recipient is not a string"),
        })
        .collect()
}

fn action_redirect(email_id: i64, return_to: Option<&str>) -> Response {
    let destination = if return_to == Some("detail") {
        format!("/inbox/{email_id}")
    } else {
        "/inbox".to_owned()
    };
    Redirect::to(&destination).into_response()
}

fn action_repository_error(operation: &'static str, error: WorkspaceInboxError) -> Response {
    match error {
        WorkspaceInboxError::Validation(_) => {
            action_error(StatusCode::BAD_REQUEST, "Invalid loan selection.")
        }
        WorkspaceInboxError::NotFound(_) => action_error(
            StatusCode::NOT_FOUND,
            "The email or active imported loan was not found.",
        ),
        WorkspaceInboxError::Conflict(_) => action_error(
            StatusCode::CONFLICT,
            "The email link changed. Reload the page and try again.",
        ),
        WorkspaceInboxError::Storage(error) => action_storage_error(operation, error),
    }
}

fn page_storage_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "Inbox page storage failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not load this page.",
    )
        .into_response()
}

fn panel_storage_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "Inbox panel storage failure");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::response::Html("<div class=\"alert alert-error\">Could not load this email.</div>"),
    )
        .into_response()
}

fn action_storage_error(operation: &'static str, error: impl Display) -> Response {
    tracing::error!(%error, operation, "Inbox action storage failure");
    action_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not update this email.",
    )
}

fn action_error(status: StatusCode, message: &'static str) -> Response {
    (status, message).into_response()
}

fn panel_not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::response::Html("<div class=\"alert alert-warning\">Email not found.</div>"),
    )
        .into_response()
}

#[cfg(all(test, feature = "local-db"))]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use libsql::Builder;
    use tower::ServiceExt;

    use super::*;
    use crate::{
        cron_auth::CronAuthenticator, crypto::CredentialCipher, db::AppContext,
        app::router_with_store, operations::OperationRepository, session_store::LibsqlSessionStore,
    };

    const EMAIL: &str = "admin@example.com";
    const PASSWORD: &str = "correct horse battery staple";

    async fn context() -> AppContext {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        AppContext::from_database(database).await.unwrap()
    }

    async fn seed_inbox(context: &AppContext) -> i64 {
        let connection = context.connection().await.unwrap();
        connection
            .execute_batch(
                "INSERT INTO intg_integration_connection ( \
                    id, slug, name, provider, status \
                 ) VALUES (1, 'tmo', 'The Mortgage Office', 'mortgage_office', 'active'); \
                 INSERT INTO intg_tmo_import_loan ( \
                    connection_id, loan_account, borrower_name, is_active \
                 ) VALUES (1, 'LN-100', 'Ada Borrower', 1); \
                 INSERT INTO intg_received_email ( \
                    id, resend_email_id, from_address, to_addresses, subject, received_at, \
                    body_s3_key, body_content_type, processing_state, created_at, updated_at \
                 ) VALUES (10, 'resend-10', 'sender@example.com', \
                           '[\"ops@example.com\"]', 'Funding notice', \
                           '2026-07-14T18:20:30.456Z', 'emails/resend-10/body.html', \
                           'text/html', 'stored', '2026-07-14T18:20:30.456Z', \
                           '2026-07-14T18:20:30.456Z'); \
                 INSERT INTO intg_received_email_attachment ( \
                    email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state \
                 ) VALUES (10, 'attachment-1', 'wire.pdf', 'application/pdf', 1234567, \
                           'emails/resend-10/wire.pdf', 'stored');",
            )
            .await
            .unwrap();
        10
    }

    #[tokio::test]
    async fn pages_render_metadata_without_claiming_object_access() {
        let context = context().await;
        seed_inbox(&context).await;

        let list = inbox_page(
            Extension(context.clone()),
            Query(InboxQuery { show_linked: true }),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let html = String::from_utf8(
            to_bytes(list.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("Funding notice"));
        assert!(html.contains("data-local=\"2026-07-14T18:20:30.456Z\""));
        assert!(html.contains(">1<"));

        let detail = email_detail_page(Extension(context), Path(10)).await;
        assert_eq!(detail.status(), StatusCode::OK);
        let html = String::from_utf8(
            to_bytes(detail.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        assert!(html.contains("1,234,567 bytes"));
        assert!(html.contains("Open stored body"));
        assert!(html.contains(">Download</a>"));
        assert!(html.contains("/media/emails/"));
    }

    #[tokio::test]
    async fn missing_detail_and_panel_return_real_not_found_statuses() {
        let context = context().await;
        assert_eq!(
            email_detail_page(Extension(context.clone()), Path(999))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            email_panel(Extension(context), Path(999)).await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn full_router_authenticates_then_enforces_durable_write_gate() {
        let context = context().await;
        context
            .bootstrap_admin(Some(EMAIL), Some(PASSWORD))
            .await
            .unwrap();
        let email_id = seed_inbox(&context).await;
        let store = LibsqlSessionStore::new(context.clone());
        let app = router_with_store(
            context.clone(),
            store,
            false,
            Arc::new(CredentialCipher::new("inbox-router-test-key").unwrap()),
            CronAuthenticator::new(Some("test-cron-secret")),
            Arc::new(crate::media::MediaService::disabled()),
            Arc::new(crate::resend::ResendService::disabled()),
        );

        let anonymous = app
            .clone()
            .oneshot(
                Request::post(format!("/inbox/{email_id}/link"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("loan_account=LN-100"))
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
        assert_eq!(login.status(), StatusCode::SEE_OTHER);
        let cookie = login
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("__td_session="))
            .and_then(|value| value.split(';').next())
            .unwrap()
            .to_owned();

        let blocked = app
            .clone()
            .oneshot(
                Request::post(format!("/inbox/{email_id}/link"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, &cookie)
                    .body(Body::from("loan_account=LN-100"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(blocked.headers().contains_key(header::RETRY_AFTER));
        assert!(
            WorkspaceInboxRepository::new(&context.connection().await.unwrap())
                .email(email_id)
                .await
                .unwrap()
                .unwrap()
                .loan_account
                .is_none()
        );

        OperationRepository::new(&context.connection().await.unwrap())
            .enable_writes("2026-07-14T18:20:30.456Z")
            .await
            .unwrap();
        let linked = app
            .oneshot(
                Request::post(format!("/inbox/{email_id}/link"))
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, cookie)
                    .body(Body::from("loan_account=LN-100&return_to=detail"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(linked.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            linked.headers()[header::LOCATION],
            format!("/inbox/{email_id}")
        );
        assert_eq!(
            WorkspaceInboxRepository::new(&context.connection().await.unwrap())
                .email(email_id)
                .await
                .unwrap()
                .unwrap()
                .loan_account
                .as_deref(),
            Some("LN-100")
        );
    }

    #[tokio::test]
    async fn action_errors_are_statusful_and_sanitized() {
        let context = context().await;
        let email_id = seed_inbox(&context).await;
        let response = link_email(
            Extension(context),
            Path(email_id),
            Form(LinkEmailForm {
                loan_account: "NOT-IMPORTED".to_owned(),
                return_to: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body = String::from_utf8_lossy(&body);
        assert!(!body.contains("NOT-IMPORTED"));
        assert!(!body.contains("SELECT"));
    }

    #[tokio::test]
    async fn inbox_list_storage_failure_is_not_rendered_as_empty() {
        let context = context().await;
        context
            .connection()
            .await
            .unwrap()
            .execute("DROP TABLE intg_received_email_attachment", ())
            .await
            .unwrap();
        let response = inbox_page(Extension(context), Query(InboxQuery::default())).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&body[..], b"Could not load this page.");
    }

    #[tokio::test]
    async fn list_projection_counts_attachments_in_one_typed_read() {
        let context = context().await;
        seed_inbox(&context).await;
        let connection = context.connection().await.unwrap();
        let items = WorkspaceInboxRepository::new(&connection)
            .list_inbox_items(true)
            .await
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].attachment_count, 1);
        assert_eq!(items[0].email.id, 10);
    }
}
