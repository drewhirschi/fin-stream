use askama::Template;
use axum::{
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};

use crate::{
    filters,
    finance::{
        AccountView, CanvasStreamView, ForecastResponse, StreamConfigView, StreamViewEditor,
        StreamViewSummary,
    },
    integrations::{
        CapturedProviderRecord, IntegrationConnectionView, NormalizedTmoPayment, TmoImportLoan,
        TmoImportPayment, TmoLoanListItem,
    },
    media::{WorkspaceFormValues, WorkspacePhotoView},
    operations::{OperationControl, SyncRun},
    workspace_inbox::{InboxEmailListItem, ReceivedEmail, ReceivedEmailAttachment},
};

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFoundTemplate {
    title: &'static str,
}

#[derive(Template)]
#[template(path = "streams.html")]
pub struct StreamsTemplate {
    pub title: String,
    pub accounts: Vec<AccountView>,
    pub streams: Vec<StreamConfigView>,
    pub views: Vec<StreamViewEditor>,
}

#[derive(Template)]
#[template(path = "forecast.html")]
pub struct ForecastTemplate {
    pub title: String,
    pub accounts: Vec<AccountView>,
    pub streams: Vec<StreamConfigView>,
    pub views: Vec<StreamViewSummary>,
    pub forecast: Option<ForecastResponse>,
    pub selected_view_id: i64,
    pub default_stream_id: i64,
}

#[derive(Template)]
#[template(path = "canvas.html")]
pub struct CanvasTemplate {
    pub title: String,
    pub streams: Vec<CanvasStreamView>,
    pub default_stream_id: i64,
}

#[derive(Template)]
#[template(path = "integrations.html")]
pub struct IntegrationsTemplate {
    pub title: String,
    pub connections: Vec<IntegrationConnectionView>,
}

#[derive(Template)]
#[template(path = "integration_overview.html")]
pub struct IntegrationOverviewTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: &'static str,
    pub loans: Vec<TmoLoanListItem>,
    pub payments: Vec<NormalizedTmoPayment>,
    pub portfolio_value: Option<f64>,
    pub portfolio_yield: Option<f64>,
    pub ytd_interest: Option<f64>,
    pub trust_balance: Option<f64>,
    pub outstanding_checks: Option<f64>,
    pub active_loans_count: usize,
}

#[derive(Template)]
#[template(path = "integration_loans.html")]
pub struct IntegrationLoansTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: &'static str,
    pub loans: Vec<TmoLoanListItem>,
}

#[derive(Template)]
#[template(path = "integration_loan_detail.html")]
pub struct IntegrationLoanDetailTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: &'static str,
    pub loan: TmoImportLoan,
    pub workspace_form: WorkspaceFormValues,
    pub workspace_photos: Vec<WorkspacePhotoView>,
    pub payment_history: Vec<TmoImportPayment>,
    pub loan_emails: Vec<ReceivedEmail>,
}

#[derive(Template)]
#[template(path = "integration_payments.html")]
pub struct IntegrationPaymentsTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: &'static str,
    pub payments: Vec<TmoImportPayment>,
}

#[derive(Template)]
#[template(path = "integration_sync.html")]
pub struct IntegrationSyncTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: &'static str,
    pub sync_logs: Vec<SyncRun>,
    pub control: OperationControl,
}

#[derive(Template)]
#[template(path = "integration_debug.html")]
pub struct IntegrationDebugTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: &'static str,
    pub sync_logs: Vec<SyncRun>,
    pub tmo_import_payments: Vec<TmoImportPayment>,
    pub captured_records: Vec<CapturedProviderRecord>,
    pub normalized_payments: Vec<NormalizedTmoPayment>,
}

#[derive(Template)]
#[template(path = "inbox.html")]
pub struct InboxTemplate {
    pub title: String,
    pub emails: Vec<InboxEmailListItem>,
    pub loans: Vec<TmoLoanListItem>,
    pub show_linked: bool,
}

#[derive(Template)]
#[template(path = "inbox_email_detail.html")]
pub struct InboxEmailDetailTemplate {
    pub title: String,
    pub email: ReceivedEmail,
    pub attachments: Vec<ReceivedEmailAttachment>,
    pub recipients: Vec<String>,
    pub loans: Vec<TmoLoanListItem>,
}

#[derive(Template)]
#[template(path = "_inbox_email_panel.html")]
pub struct InboxEmailPanelTemplate {
    pub email: ReceivedEmail,
    pub attachments: Vec<ReceivedEmailAttachment>,
    pub recipients: Vec<String>,
}

pub fn not_found_response() -> Response {
    match (NotFoundTemplate {
        title: "Page not found · Trust Deeds",
    })
    .render()
    {
        Ok(html) => (StatusCode::NOT_FOUND, Html(html)).into_response(),
        Err(error) => {
            tracing::error!(%error, "failed to render not-found template");
            (StatusCode::NOT_FOUND, "Page not found.").into_response()
        }
    }
}

pub fn streams_response(template: StreamsTemplate) -> Response {
    render_response(template, "streams")
}

pub fn forecast_response(template: ForecastTemplate) -> Response {
    render_response(template, "forecast")
}

pub fn canvas_response(template: CanvasTemplate) -> Response {
    render_response(template, "canvas")
}

pub fn integrations_response(template: IntegrationsTemplate) -> Response {
    render_response(template, "integrations")
}

pub fn integration_overview_response(template: IntegrationOverviewTemplate) -> Response {
    render_response(template, "integration overview")
}

pub fn integration_loans_response(template: IntegrationLoansTemplate) -> Response {
    render_response(template, "integration loans")
}

pub fn integration_loan_detail_response(template: IntegrationLoanDetailTemplate) -> Response {
    render_response(template, "integration loan detail")
}

pub fn integration_payments_response(template: IntegrationPaymentsTemplate) -> Response {
    render_response(template, "integration payments")
}

pub fn integration_sync_response(template: IntegrationSyncTemplate) -> Response {
    render_response(template, "integration sync")
}

pub fn integration_debug_response(template: IntegrationDebugTemplate) -> Response {
    render_response(template, "integration debug")
}

pub fn inbox_response(template: InboxTemplate) -> Response {
    render_response(template, "Inbox")
}

pub fn inbox_email_detail_response(template: InboxEmailDetailTemplate) -> Response {
    render_response(template, "Inbox email detail")
}

pub fn inbox_email_panel_response(template: InboxEmailPanelTemplate) -> Response {
    render_response(template, "Inbox email panel")
}

fn render_response(template: impl Template, template_name: &'static str) -> Response {
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(error) => {
            tracing::error!(%error, template_name, "failed to render template");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not render this page.",
            )
                .into_response()
        }
    }
}

pub fn service_unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Authentication service temporarily unavailable.",
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finance::{CanvasStreamView, CashSourceView, ForecastResponse};

    fn account(name: &str, balance: Option<f64>) -> AccountView {
        AccountView {
            id: 1,
            name: name.into(),
            kind: "checking".into(),
            balance,
            balance_as_of_date: Some("2026-07-14".into()),
            source_type: Some("manual".into()),
            source_ref: None,
            metadata: None,
            balance_updated_at: Some("2026-07-14T12:30:00Z".into()),
            is_primary: 1,
            is_active: 1,
            notes: None,
        }
    }

    fn stream(name: &str) -> StreamConfigView {
        StreamConfigView {
            id: 7,
            name: name.into(),
            stream_type: "credit_card_due".into(),
            kind: "credit_card".into(),
            direction: "out".into(),
            amount_certainty: "estimated".into(),
            description: None,
            is_active: 1,
            default_account_id: Some(1),
            default_account_name: Some("Primary Cash".into()),
            configuration: None,
            parent_id: None,
            schedule_id: None,
            schedule_label: None,
            schedule_amount: Some(0.0),
            schedule_frequency: None,
            due_day: None,
            schedule_start_date: None,
            schedules: Vec::new(),
        }
    }

    #[test]
    fn stream_names_are_data_not_inline_javascript() {
        let html = StreamsTemplate {
            title: "Streams".into(),
            accounts: vec![account("Primary Cash", Some(0.0))],
            streams: vec![stream("Drew's Card")],
            views: Vec::new(),
        }
        .render()
        .unwrap();

        assert!(
            html.contains("data-stream-name=\"Drew's Card\"")
                || html.contains("data-stream-name=\"Drew&#39;s Card\"")
                || html.contains("data-stream-name=\"Drew&#x27;s Card\"")
        );
        assert!(html.contains("deleteStream(7, $el.dataset.streamName)"));
        assert!(!html.contains("deleteStream(7, 'Drew"));
        assert!(html.contains("Balance as of 07-14-2026"));
        assert!(html.contains("value=\"0\""));
    }

    #[test]
    fn forecast_template_preserves_confirmed_zero() {
        let html = ForecastTemplate {
            title: "Timeline".into(),
            accounts: vec![account("Primary Cash", Some(0.0))],
            streams: vec![stream("Expense")],
            views: Vec::new(),
            forecast: Some(ForecastResponse {
                starting_balance: 0.0,
                balance_as_of_date: "2026-07-14".into(),
                cash_source: CashSourceView {
                    amount: 0.0,
                    as_of_date: "2026-07-14".into(),
                    account_name: Some("Primary Cash".into()),
                    source_kind: "manual".into(),
                    detail: "Manual balance for Primary Cash".into(),
                    updated_at: Some("2026-07-14T12:30:00Z".into()),
                },
                opening_balance: 0.0,
                rows: Vec::new(),
                ending_balance: 0.0,
            }),
            selected_view_id: 0,
            default_stream_id: 7,
        }
        .render()
        .unwrap();

        assert!(html.contains("cashInput: '0'"));
        assert!(html.contains("cash_source?.amount ?? forecast?.starting_balance ?? 0"));
        assert!(html.contains("JSON.stringify({ amount, as_of_date: today })"));
        assert!(html.contains("Sync from Monarch"));
        assert!(html.contains("JSON.stringify({ as_of_date: today })"));
    }

    #[test]
    fn canvas_template_keeps_stream_names_in_escaped_data_attributes() {
        let html = CanvasTemplate {
            title: "Canvas".into(),
            streams: vec![CanvasStreamView {
                id: 19,
                name: "</button><script>alert('canvas')</script>".into(),
                kind: "tmo_trust".into(),
            }],
            default_stream_id: 19,
        }
        .render()
        .unwrap();

        assert!(html.contains("data-default-stream-id=\"19\""));
        assert!(html.contains("data-stream-id=\"19\""));
        assert!(html.contains("/static/js/canvas.js"));
        assert!(!html.contains("<script>alert('canvas')</script>"));
        assert!(html.contains("alert"));

        let runtime = include_str!("../public/static/js/canvas.js");
        assert!(runtime.contains("TrustDeedsUI.currency"));
        assert!(runtime.contains("TrustDeedsUI.date"));
        assert!(!runtime.contains("toLocaleString"));
    }
}
