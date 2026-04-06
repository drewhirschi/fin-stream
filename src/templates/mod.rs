use askama::Template;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::models::*;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub title: String,
    pub loans: Vec<LoanView>,
    pub recent_payments: Vec<PaymentView>,
    pub upcoming: Vec<PaymentView>,
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
#[template(path = "payments.html")]
pub struct PaymentsTemplate {
    pub title: String,
    pub payments: Vec<PaymentView>,
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
    pub streams: Vec<(i64, String)>,
    pub expenses_stream_id: i64,
}

#[derive(Template)]
#[template(path = "sync_logs_partial.html")]
pub struct SyncLogsPartialTemplate {
    pub logs: Vec<SyncLog>,
}

// Implement IntoResponse for all templates
macro_rules! impl_into_response {
    ($($t:ty),*) => {
        $(
            impl IntoResponse for $t {
                fn into_response(self) -> axum::response::Response {
                    match self.render() {
                        Ok(html) => axum::response::Html(html).into_response(),
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

impl_into_response!(IndexTemplate, LoansTemplate, PaymentsTemplate, SyncTemplate, ForecastTemplate, SyncLogsPartialTemplate);
