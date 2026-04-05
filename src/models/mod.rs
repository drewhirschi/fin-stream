use serde::{Deserialize, Serialize};

// ── Sync status (in-memory, shown in UI) ──

#[derive(Clone, Debug, Serialize)]
pub struct SyncStatus {
    pub phase: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub is_running: bool,
    pub error: Option<String>,
    pub loans_synced: i32,
    pub payments_synced: i32,
}

// ── Sync log (persisted) ──

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct SyncLog {
    pub id: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub endpoints_hit: Option<String>,
    pub events_upserted: Option<i32>,
    pub loans_upserted: Option<i32>,
    pub snapshots_created: Option<i32>,
}

// ── TMO API response types ──

#[derive(Debug, Deserialize)]
pub struct TmoResponse<T> {
    pub data: T,
    pub success: bool,
    pub error: Option<String>,
    #[serde(rename = "errorType")]
    pub error_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoLoginData {
    pub is_valid_user: bool,
    pub user_information: TmoUserInfo,
    pub message: Option<String>,
    pub requires_mfa: bool,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TmoUserInfo {
    pub source_rec_id: String,
    pub company_id: String,
    pub account: String,
    pub name: String,
    pub email: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoPaginatedResponse<T> {
    pub page: i32,
    pub rows_per_page: i32,
    pub total_count: i32,
    pub data: Vec<T>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoLoanSummary {
    pub loan_account: String,
    pub borrower_name: String,
    pub primary_street: String,
    pub primary_city: String,
    pub primary_state: String,
    pub primary_zip: String,
    pub percent_owned: f64,
    pub interest_rate: f64,
    pub maturity_date: String,
    pub term_left: i32,
    pub next_payment_date: String,
    pub interest_paid_to_date: String,
    pub billed_through: Option<String>,
    pub regular_payment: f64,
    pub loan_balance: f64,
    pub is_delinquent: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoLoanDetail {
    pub loan_account: String,
    pub borrower_name: String,
    pub primary_street: String,
    pub primary_city: String,
    pub primary_state: String,
    pub primary_zip: String,
    pub property_description: Option<String>,
    pub property_type: Option<String>,
    pub property_priority: Option<i32>,
    pub occupancy: Option<String>,
    pub ltv: Option<f64>,
    pub appraised_value: Option<f64>,
    pub priority: Option<i32>,
    pub original_balance: f64,
    pub principal_balance: f64,
    pub note_rate: f64,
    pub maturity_date: String,
    pub next_payment_date: String,
    pub interest_paid_to_date: String,
    pub regular_payment: f64,
    pub payment_frequency: String,
    pub loan_type: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoPayment {
    pub check_number: String,
    pub loan_account: String,
    pub check_date: String,
    pub amount: f64,
    pub service_fee: f64,
    pub interest: f64,
    pub principal: f64,
    pub charges: f64,
    pub late_charges: f64,
    pub other: f64,
    pub borrower_name: String,
    pub property_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoOverview {
    pub portfolio_value: f64,
    pub portfolio_yield: f64,
    pub ytd_interest: f64,
    pub ytd_principal: f64,
    pub portfolio_count: i32,
    pub trust_balance: f64,
    pub outstanding_checks_value: f64,
    pub ytd_serv_fees: f64,
}

// ── View models for templates ──

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct LoanView {
    pub loan_account: String,
    pub borrower_name: Option<String>,
    pub property_address: Option<String>,
    pub property_city: Option<String>,
    pub property_state: Option<String>,
    pub property_type: Option<String>,
    pub percent_owned: Option<f64>,
    pub note_rate: Option<f64>,
    pub principal_balance: Option<f64>,
    pub regular_payment: Option<f64>,
    pub maturity_date: Option<String>,
    pub next_payment_date: Option<String>,
    pub interest_paid_to: Option<String>,
    pub is_delinquent: Option<i32>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PaymentView {
    pub id: i64,
    pub label: Option<String>,
    pub scheduled_date: String,
    pub actual_date: Option<String>,
    pub amount: f64,
    pub status: String,
    pub metadata: Option<String>,
}
