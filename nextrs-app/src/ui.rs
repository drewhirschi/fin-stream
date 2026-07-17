use anyhow::Context;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{
    db::AppContext,
    finance::{
        AccountView, CanvasStreamView, FinanceRepository, StreamConfigView, StreamViewEditor,
    },
    integrations::{
        CapturedProviderRecord, IntegrationConnectionView, IntegrationRepository,
        NormalizedTmoPayment, TmoImportLoan, TmoImportOverview, TmoImportPayment, TmoLoanListItem,
    },
    media::{WorkspaceFormValues, WorkspacePhotoView},
    operations::{OperationControl, OperationRepository, SyncRun},
    workspace_inbox::{
        InboxEmailListItem, ReceivedEmail, ReceivedEmailAttachment, WorkspaceInboxRepository,
    },
};

#[derive(Serialize)]
pub(crate) struct FinanceData {
    pub accounts: Vec<AccountView>,
    pub streams: Vec<StreamConfigView>,
    pub views: Vec<StreamViewEditor>,
    pub canvas_streams: Vec<CanvasStreamView>,
}

#[derive(Serialize)]
pub(crate) struct IntegrationsData {
    pub connections: Vec<IntegrationConnectionView>,
}

#[derive(Serialize)]
pub(crate) struct IntegrationData {
    pub connection: IntegrationConnectionView,
    pub loans: Vec<TmoLoanListItem>,
    pub payments: Vec<TmoImportPayment>,
    pub normalized_payments: Vec<NormalizedTmoPayment>,
    pub overviews: Vec<TmoImportOverview>,
    pub captured_records: Vec<CapturedProviderRecord>,
    pub sync_logs: Vec<SyncRun>,
    pub control: OperationControl,
}

#[derive(Serialize)]
pub(crate) struct LoanData {
    pub connection: IntegrationConnectionView,
    pub loan: TmoImportLoan,
    pub workspace: WorkspaceFormValues,
    pub photos: Vec<WorkspacePhotoView>,
    pub payments: Vec<TmoImportPayment>,
    pub emails: Vec<ReceivedEmail>,
}

#[derive(Serialize)]
pub(crate) struct InboxData {
    pub emails: Vec<InboxEmailListItem>,
    pub loans: Vec<TmoLoanListItem>,
    pub show_linked: bool,
}

#[derive(Serialize)]
pub(crate) struct EmailData {
    pub email: ReceivedEmail,
    pub attachments: Vec<ReceivedEmailAttachment>,
    pub recipients: Vec<String>,
    pub loans: Vec<TmoLoanListItem>,
}

pub(crate) async fn finance(context: &AppContext) -> anyhow::Result<FinanceData> {
    let connection = context.connection().await?;
    let repository = FinanceRepository::new(&connection);
    Ok(FinanceData {
        accounts: repository.list_accounts().await?,
        streams: repository.list_streams().await?,
        views: repository.list_view_editors().await?,
        canvas_streams: repository.list_canvas_streams().await?,
    })
}

pub(crate) async fn integrations(context: &AppContext) -> anyhow::Result<IntegrationsData> {
    let connection = context.connection().await?;
    Ok(IntegrationsData {
        connections: IntegrationRepository::new(&connection)
            .list_connection_views()
            .await?,
    })
}

pub(crate) async fn integration(
    context: &AppContext,
    slug: &str,
    section: &str,
) -> anyhow::Result<Option<IntegrationData>> {
    let connection = context.connection().await?;
    let repository = IntegrationRepository::new(&connection);
    let Some(view) = repository.connection_view_by_slug(slug).await? else {
        return Ok(None);
    };
    let is_tmo = view.slug == "tmo" && view.provider == "mortgage_office";
    let loans = if is_tmo && matches!(section, "overview" | "loans") {
        repository.list_active_tmo_loan_views(view.id).await?
    } else {
        Vec::new()
    };
    let payments = if is_tmo && matches!(section, "overview" | "payments" | "debug") {
        repository.list_recent_tmo_payments(view.id, 100).await?
    } else {
        Vec::new()
    };
    let normalized_payments = if is_tmo && section == "debug" {
        repository
            .list_normalized_tmo_payments(view.id, 100)
            .await?
    } else {
        Vec::new()
    };
    let overviews = if is_tmo && section == "overview" {
        repository.list_tmo_overviews(view.id).await?
    } else {
        Vec::new()
    };
    let captured_records = if section == "debug" {
        repository
            .list_captured_provider_records(view.id, 100)
            .await?
    } else {
        Vec::new()
    };
    let operations = OperationRepository::new(&connection);
    let sync_logs = if matches!(section, "sync" | "debug") {
        operations.list_recent(slug, 50).await?
    } else {
        Vec::new()
    };
    Ok(Some(IntegrationData {
        connection: view,
        loans,
        payments,
        normalized_payments,
        overviews,
        captured_records,
        sync_logs,
        control: operations.control().await?,
    }))
}

pub(crate) async fn loan(
    context: &AppContext,
    slug: &str,
    loan_account: &str,
) -> anyhow::Result<Option<LoanData>> {
    let connection = context.connection().await?;
    let repository = IntegrationRepository::new(&connection);
    let Some(view) = repository.connection_view_by_slug(slug).await? else {
        return Ok(None);
    };
    if view.slug != "tmo" || view.provider != "mortgage_office" {
        return Ok(None);
    }
    let Some(loan) = repository
        .tmo_loan_by_account(view.id, loan_account)
        .await?
    else {
        return Ok(None);
    };
    let connection_id = loan.connection_id;
    let workspace_repository = WorkspaceInboxRepository::new(&connection);
    let workspace_record = workspace_repository
        .workspace(connection_id, loan_account)
        .await?;
    Ok(Some(LoanData {
        connection: view,
        loan,
        workspace: WorkspaceFormValues::from(workspace_record.as_ref()),
        photos: workspace_repository
            .list_photos(connection_id, loan_account)
            .await?
            .into_iter()
            .map(WorkspacePhotoView::from)
            .collect(),
        payments: repository
            .list_tmo_payments_for_loan(connection_id, loan_account, 36)
            .await?,
        emails: workspace_repository
            .list_emails_for_loan(loan_account)
            .await?,
    }))
}

pub(crate) async fn inbox(context: &AppContext, show_linked: bool) -> anyhow::Result<InboxData> {
    let connection = context.connection().await?;
    let emails = WorkspaceInboxRepository::new(&connection)
        .list_inbox_items(show_linked)
        .await?;
    let integrations = IntegrationRepository::new(&connection);
    let loans = match integrations.connection_by_slug("tmo").await? {
        Some(value) if value.provider == "mortgage_office" => {
            integrations.list_active_tmo_loan_views(value.id).await?
        }
        _ => Vec::new(),
    };
    Ok(InboxData {
        emails,
        loans,
        show_linked,
    })
}

pub(crate) async fn email(
    context: &AppContext,
    email_id: i64,
) -> anyhow::Result<Option<EmailData>> {
    if email_id <= 0 {
        return Ok(None);
    }
    let connection = context.connection().await?;
    let inbox = WorkspaceInboxRepository::new(&connection);
    let Some(detail) = inbox.email_detail(email_id).await? else {
        return Ok(None);
    };
    let recipients = serde_json::from_str::<Vec<String>>(&detail.email.to_addresses)
        .context("decode email recipients")?;
    let integrations = IntegrationRepository::new(&connection);
    let loans = match integrations.connection_by_slug("tmo").await? {
        Some(value) if value.provider == "mortgage_office" => {
            integrations.list_active_tmo_loan_views(value.id).await?
        }
        _ => Vec::new(),
    };
    Ok(Some(EmailData {
        email: detail.email,
        attachments: detail.attachments,
        recipients,
        loans,
    }))
}

pub(crate) fn response<T: Serialize>(result: anyhow::Result<T>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => {
            tracing::error!(%error, "React view data load failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load this view.",
            )
                .into_response()
        }
    }
}

pub(crate) fn optional_response<T: Serialize>(result: anyhow::Result<Option<T>>) -> Response {
    match result {
        Ok(Some(value)) => Json(value).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Not found.").into_response(),
        Err(error) => {
            tracing::error!(%error, "React view data load failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not load this view.",
            )
                .into_response()
        }
    }
}
