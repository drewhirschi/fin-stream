use axum::{
    Router,
    extract::{Form, Path, Query, State},
    http::Uri,
    response::{IntoResponse, Redirect},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::AppState;
use crate::db;
use crate::templates;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(index))
        .route("/streams", get(streams))
        .route("/forecast", get(forecast))
        .route("/canvas", get(canvas))
        .route("/inbox", get(inbox))
        .route("/inbox/{email_id}", get(inbox_email_detail))
        .route("/inbox/{email_id}/panel", get(inbox_email_panel))
        .route(
            "/inbox/{email_id}/attachments/{attachment_id}/viewer",
            get(inbox_attachment_viewer),
        )
        .route("/inbox/{email_id}/link", post(link_email_to_loan))
        .route("/inbox/{email_id}/unlink", post(unlink_email_from_loan))
        .route("/inbox/{email_id}/retry", post(retry_email_fetch))
}

// Intentionally minimal: the global dashboard is a placeholder until a
// multi-integration summary is designed. Per-integration stats now live on
// /integrations/{slug}.
async fn index() -> templates::IndexTemplate {
    templates::IndexTemplate {
        title: "Trust Deeds - Dashboard".into(),
    }
}

async fn forecast(State(state): State<Arc<AppState>>) -> templates::ForecastTemplate {
    let has_balance = db::forecasts::get_starting_balance(&state.db)
        .await
        .is_some();

    let streams = db::streams::list_streams(&state.db).await;
    let views = db::streams::list_view_summaries(&state.db).await;
    let accounts = db::accounts::list_accounts(&state.db).await;
    let default_view_id = db::streams::default_view_id(&state.db).await;
    let selected_view_id = default_view_id
        .or_else(|| views.first().map(|view| view.id))
        .unwrap_or(0);
    let default_stream_id = streams.first().map(|stream| stream.id).unwrap_or(0);

    templates::ForecastTemplate {
        title: "Trust Deeds - Timeline".into(),
        has_balance,
        streams,
        views,
        accounts,
        default_view_id,
        selected_view_id,
        default_stream_id,
    }
}

async fn streams(State(state): State<Arc<AppState>>) -> templates::StreamsTemplate {
    templates::StreamsTemplate {
        title: "Trust Deeds - Streams".into(),
        accounts: db::accounts::list_accounts(&state.db).await,
        streams: db::streams::list_streams(&state.db).await,
        views: db::streams::list_view_editors(&state.db)
            .await
            .unwrap_or_default(),
    }
}

async fn canvas(State(state): State<Arc<AppState>>) -> templates::CanvasTemplate {
    let streams = db::streams::list_streams(&state.db).await;
    let default_stream_id = streams
        .iter()
        .find(|stream| stream.name == "Trust Deeds" || stream.kind == "tmo_trust")
        .map(|stream| stream.id)
        .or_else(|| streams.first().map(|stream| stream.id))
        .unwrap_or(0);

    templates::CanvasTemplate {
        title: "Trust Deeds - Canvas".into(),
        streams,
        default_stream_id,
    }
}

#[derive(Deserialize, Default)]
struct LinkEmailForm {
    loan_account: String,
}

#[derive(Deserialize, Default)]
struct InboxQuery {
    #[serde(default)]
    show_linked: bool,
}

async fn inbox(
    State(state): State<Arc<AppState>>,
    Query(query): Query<InboxQuery>,
) -> templates::InboxTemplate {
    let emails = db::emails::list_inbox_emails(&state.db, query.show_linked).await;
    let loans = db::integrations::list_active_tmo_loans(&state.db).await;

    templates::InboxTemplate {
        title: "Trust Deeds - Inbox".into(),
        emails,
        loans,
        show_linked: query.show_linked,
    }
}

async fn inbox_email_detail(
    State(state): State<Arc<AppState>>,
    Path(email_id): Path<i64>,
) -> axum::response::Response {
    let Some(email) = db::emails::get_email_by_id(&state.db, email_id).await else {
        return templates::NotFoundTemplate {
            title: "Trust Deeds - Not Found".into(),
            path: format!("/inbox/{email_id}"),
        }
        .into_response();
    };
    let attachments = db::emails::list_attachments_for_email(&state.db, email_id).await;
    let loans = db::integrations::list_active_tmo_loans(&state.db).await;

    templates::InboxEmailDetailTemplate {
        title: format!(
            "Trust Deeds - {}",
            email.subject.as_deref().unwrap_or("(no subject)")
        ),
        email,
        attachments,
        loans,
    }
    .into_response()
}

async fn inbox_email_panel(
    State(state): State<Arc<AppState>>,
    Path(email_id): Path<i64>,
) -> axum::response::Response {
    let Some(email) = db::emails::get_email_by_id(&state.db, email_id).await else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::response::Html("<div class=\"alert alert-error\">Email not found.</div>"),
        )
            .into_response();
    };
    let attachments = db::emails::list_attachments_for_email(&state.db, email_id).await;

    templates::EmailPanelPartial { email, attachments }.into_response()
}

async fn inbox_attachment_viewer(
    State(state): State<Arc<AppState>>,
    Path((email_id, attachment_id)): Path<(i64, i64)>,
) -> axum::response::Response {
    let attachments = db::emails::list_attachments_for_email(&state.db, email_id).await;
    let Some(attachment) = attachments.into_iter().find(|att| att.id == attachment_id) else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::response::Html("<div class=\"alert alert-error\">Attachment not found.</div>"),
        )
            .into_response();
    };
    let Some(key) = attachment.s3_key.clone() else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            axum::response::Html(
                "<div class=\"alert alert-warning\">Attachment is not yet stored.</div>",
            ),
        )
            .into_response();
    };

    templates::DocViewerPartial {
        attachment,
        media_url: format!("/media/emails/{key}"),
        back_url: format!("/inbox/{email_id}/panel"),
    }
    .into_response()
}

async fn link_email_to_loan(
    State(state): State<Arc<AppState>>,
    Path(email_id): Path<i64>,
    Form(form): Form<LinkEmailForm>,
) -> axum::response::Response {
    if form.loan_account.is_empty() {
        return Redirect::to("/inbox").into_response();
    }

    if let Err(e) =
        db::emails::link_email_to_loan(&state.db, email_id, &form.loan_account).await
    {
        tracing::error!("failed to link email {email_id}: {e}");
    }

    Redirect::to("/inbox").into_response()
}

async fn unlink_email_from_loan(
    State(state): State<Arc<AppState>>,
    Path(email_id): Path<i64>,
) -> axum::response::Response {
    if let Err(e) = db::emails::unlink_email(&state.db, email_id).await {
        tracing::error!("failed to unlink email {email_id}: {e}");
    }

    Redirect::to("/inbox").into_response()
}

async fn retry_email_fetch(
    State(state): State<Arc<AppState>>,
    Path(email_id): Path<i64>,
) -> axum::response::Response {
    let resend_email_id = match db::emails::get_resend_email_id(&state.db, email_id).await {
        Ok(Some(id)) => id,
        Ok(None) => {
            tracing::warn!("retry requested for unknown email id {email_id}");
            return Redirect::to("/inbox").into_response();
        }
        Err(e) => {
            tracing::error!("failed to look up email {email_id} for retry: {e}");
            return Redirect::to("/inbox").into_response();
        }
    };

    let attachment_ids = match db::emails::list_attachment_fetch_targets(&state.db, email_id).await
    {
        Ok(list) => list,
        Err(e) => {
            tracing::error!("failed to list attachments for retry on email {email_id}: {e}");
            Vec::new()
        }
    };

    if let Err(e) = db::emails::reset_email_for_retry(&state.db, email_id).await {
        tracing::error!("failed to reset email {email_id} for retry: {e}");
        return Redirect::to("/inbox").into_response();
    }

    let pool = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::routes::webhooks::fetch_and_store_email(
            &pool,
            &resend_email_id,
            email_id,
            &attachment_ids,
        )
        .await
        {
            tracing::error!("retry failed for email {resend_email_id}: {e}");
            let _ = db::emails::mark_email_error(&pool, email_id, &e.to_string()).await;
        }
    });

    Redirect::to("/inbox").into_response()
}

pub async fn not_found(uri: Uri) -> templates::NotFoundTemplate {
    templates::NotFoundTemplate {
        title: "Trust Deeds - Not Found".into(),
        path: uri.path().to_string(),
    }
}
