use askama::Template;
use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use axum::response::IntoResponse;

use crate::filters;
use crate::models::*;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub loans: Vec<LoanView>,
    pub recent_payments: Vec<PaymentView>,
    pub portfolio_value: Option<f64>,
    pub portfolio_yield: Option<f64>,
    pub ytd_interest: Option<f64>,
    pub trust_balance: Option<f64>,
    pub outstanding_checks: Option<f64>,
}

#[derive(Template)]
#[template(path = "loans.html")]
pub struct LoansTemplate {
    pub title: String,
    pub loans: Vec<LoanView>,
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
    pub current_section: String,
    pub loans: Vec<LoanView>,
    pub payments: Vec<PaymentView>,
}

#[derive(Template)]
#[template(path = "integration_loans.html")]
pub struct IntegrationLoansTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: String,
    pub loans: Vec<LoanView>,
}

#[derive(Template)]
#[template(path = "integration_loan_detail.html")]
pub struct IntegrationLoanDetailTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: String,
    pub loan: LoanDetailView,
    pub workspace: LoanWorkspaceView,
    pub workspace_photos: Vec<LoanWorkspacePhotoView>,
    pub payment_history: Vec<TmoImportPaymentView>,
    pub workspace_saved: bool,
    pub workspace_error: bool,
    pub photo_uploaded: bool,
    pub photo_error: bool,
    pub feature_saved: bool,
}

#[derive(Template)]
#[template(path = "integration_payments.html")]
pub struct IntegrationPaymentsTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: String,
    pub payments: Vec<TmoImportPaymentView>,
}

#[derive(Template)]
#[template(path = "integration_sync.html")]
pub struct IntegrationSyncTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: String,
    pub sync_logs: Vec<SyncLog>,
}

#[derive(Template)]
#[template(path = "integration_debug.html")]
pub struct IntegrationDebugTemplate {
    pub title: String,
    pub connection: IntegrationConnectionView,
    pub current_section: String,
    pub sync_logs: Vec<SyncLog>,
    pub tmo_import_payments: Vec<TmoImportPaymentView>,
    pub captured_records: Vec<CapturedProviderRecordView>,
    pub normalized_payments: Vec<PaymentView>,
}

#[derive(Template)]
#[template(path = "sync.html")]
pub struct SyncTemplate {
    pub title: String,
    pub logs: Vec<SyncLog>,
    pub current_status: Option<SyncStatus>,
}

#[derive(Template)]
#[template(path = "forecast.html")]
pub struct ForecastTemplate {
    pub title: String,
    pub has_balance: bool,
    pub streams: Vec<StreamConfigView>,
    pub views: Vec<StreamViewSummary>,
    pub accounts: Vec<AccountView>,
    pub default_view_id: Option<i64>,
    pub selected_view_id: i64,
    pub default_stream_id: i64,
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
#[template(path = "canvas.html")]
pub struct CanvasTemplate {
    pub title: String,
    pub streams: Vec<StreamConfigView>,
    pub default_stream_id: i64,
}

#[derive(Template)]
#[template(path = "inbox.html")]
pub struct InboxTemplate {
    pub title: String,
    pub emails: Vec<ReceivedEmailView>,
    pub loans: Vec<LoanView>,
}

#[derive(Template)]
#[template(path = "inbox_email_detail.html")]
pub struct InboxEmailDetailTemplate {
    pub title: String,
    pub email: ReceivedEmailView,
    pub attachments: Vec<ReceivedEmailAttachmentView>,
    pub loans: Vec<LoanView>,
}

#[derive(Template)]
#[template(path = "404.html")]
pub struct NotFoundTemplate {
    pub title: String,
    pub path: String,
}

#[derive(Template)]
#[template(path = "sync_logs_partial.html")]
pub struct SyncLogsPartialTemplate {
    pub logs: Vec<SyncLog>,
}

#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub title: String,
    pub error: Option<String>,
}

// Implement IntoResponse for all templates
macro_rules! impl_into_response {
    ($($t:ty),*) => {
        $(
            impl IntoResponse for $t {
                fn into_response(self) -> axum::response::Response {
                    match self.render() {
                        Ok(html) => (
                            [(CACHE_CONTROL, "private, max-age=60")],
                            axum::response::Html(html),
                        )
                            .into_response(),
                        Err(e) => {
                            tracing::error!("template render error: {e}");
                            (StatusCode::INTERNAL_SERVER_ERROR, "Template render error").into_response()
                        }
                    }
                }
            }
        )*
    };
}

impl_into_response!(
    IndexTemplate,
    LoansTemplate,
    IntegrationsTemplate,
    IntegrationOverviewTemplate,
    IntegrationLoansTemplate,
    IntegrationLoanDetailTemplate,
    IntegrationPaymentsTemplate,
    IntegrationSyncTemplate,
    IntegrationDebugTemplate,
    SyncTemplate,
    ForecastTemplate,
    StreamsTemplate,
    CanvasTemplate,
    SyncLogsPartialTemplate,
    InboxTemplate,
    InboxEmailDetailTemplate,
    LoginTemplate
);

impl IntoResponse for NotFoundTemplate {
    fn into_response(self) -> axum::response::Response {
        match self.render() {
            Ok(html) => (
                StatusCode::NOT_FOUND,
                [(CACHE_CONTROL, "private, max-age=60")],
                axum::response::Html(html),
            )
                .into_response(),
            Err(e) => {
                tracing::error!("template render error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Template render error").into_response()
            }
        }
    }
}
