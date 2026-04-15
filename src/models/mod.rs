use serde::{Deserialize, Serialize};

// ── Sync status (in-memory, shown in UI) ──

#[derive(Clone, Debug, Serialize)]
pub struct SyncStatus {
    pub connection_slug: String,
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
    pub connection_slug: Option<String>,
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

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TmoPaginatedResponse<T> {
    pub page: i32,
    pub rows_per_page: i32,
    pub total_count: i32,
    pub data: Vec<T>,
}

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
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
    pub featured_image_url: Option<String>,
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
pub struct LoanDetailView {
    pub loan_account: String,
    pub borrower_name: Option<String>,
    pub property_address: Option<String>,
    pub property_city: Option<String>,
    pub property_state: Option<String>,
    pub property_zip: Option<String>,
    pub property_description: Option<String>,
    pub property_type: Option<String>,
    pub occupancy: Option<String>,
    pub percent_owned: Option<f64>,
    pub note_rate: Option<f64>,
    pub original_balance: Option<f64>,
    pub principal_balance: Option<f64>,
    pub regular_payment: Option<f64>,
    pub payment_frequency: Option<String>,
    pub maturity_date: Option<String>,
    pub next_payment_date: Option<String>,
    pub interest_paid_to: Option<String>,
    pub billed_through: Option<String>,
    pub appraised_value: Option<f64>,
    pub ltv: Option<f64>,
    pub is_delinquent: Option<i32>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LoanWorkspaceView {
    pub loan_account: String,
    pub redfin_url: Option<String>,
    pub zillow_url: Option<String>,
    pub decision_status: Option<String>,
    pub target_contribution: Option<f64>,
    pub actual_contribution: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct LoanWorkspacePhotoView {
    pub id: i64,
    pub loan_account: String,
    pub provider: String,
    pub caption: Option<String>,
    pub source_url: String,
    pub image_url: String,
    pub sort_order: i32,
    pub is_featured: bool,
}

impl LoanWorkspaceView {
    pub fn empty(loan_account: impl Into<String>) -> Self {
        Self {
            loan_account: loan_account.into(),
            redfin_url: None,
            zillow_url: None,
            decision_status: None,
            target_contribution: None,
            actual_contribution: None,
            notes: None,
        }
    }
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PaymentView {
    pub id: i64,
    pub label: Option<String>,
    pub scheduled_date: String,
    pub expected_date: Option<String>,
    pub actual_date: Option<String>,
    pub amount: f64,
    pub status: String,
    pub source_type: Option<String>,
    pub is_pending_print_check: bool,
    pub check_number: Option<String>,
    pub loan_account: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoanPaymentHistoryView {
    pub id: i64,
    pub label: Option<String>,
    pub effective_date: String,
    pub display_date: String,
    pub scheduled_date: String,
    pub expected_date: Option<String>,
    pub actual_date: Option<String>,
    pub amount: f64,
    pub status: String,
    pub state_label: String,
    pub timing_label: String,
    pub check_number: Option<String>,
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AccountView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub balance: Option<f64>,
    pub source_type: Option<String>,
    pub source_ref: Option<String>,
    pub metadata: Option<String>,
    pub balance_updated_at: Option<String>,
    pub is_primary: i32,
    pub is_active: i32,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StreamConfigView {
    pub id: i64,
    pub name: String,
    #[sqlx(rename = "type")]
    pub stream_type: String,
    pub kind: String,
    pub description: Option<String>,
    pub is_active: i32,
    pub default_account_id: i64,
    pub default_account_name: Option<String>,
    pub schedule_id: Option<i64>,
    pub schedule_label: Option<String>,
    pub schedule_amount: Option<f64>,
    pub schedule_frequency: Option<String>,
    pub due_day: Option<i32>,
    pub schedule_start_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct StreamViewSummary {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_default: i32,
    pub is_active: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamViewMember {
    pub stream_id: i64,
    pub stream_name: String,
    pub included: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamViewEditor {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_default: i32,
    pub is_active: i32,
    pub members: Vec<StreamViewMember>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CashSourceView {
    pub amount: f64,
    pub account_name: Option<String>,
    pub source_kind: String,
    pub detail: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct IntegrationConnectionView {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub provider: String,
    pub status: String,
    pub sync_cadence: String,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub record_count: i64,
    pub normalized_count: i64,
    pub pending_count: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CapturedProviderRecordView {
    pub entity_type: String,
    pub external_id: String,
    pub effective_date: Option<String>,
    pub summary: Option<String>,
    pub amount: Option<f64>,
    pub raw_payload: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TmoImportPaymentView {
    pub id: i64,
    pub connection_id: i64,
    pub external_id: String,
    pub loan_account: String,
    pub borrower_name: String,
    pub property_name: String,
    pub check_number: String,
    pub check_date: String,
    pub amount: f64,
    pub service_fee: f64,
    pub interest: f64,
    pub principal: f64,
    pub charges: f64,
    pub late_charges: f64,
    pub other: f64,
    pub processing_state: String,
    pub normalized_event_source_id: Option<String>,
    pub raw_payload: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReceivedEmailView {
    pub id: i64,
    pub resend_email_id: String,
    pub from_address: String,
    pub to_addresses: String,
    pub subject: Option<String>,
    pub received_at: String,
    pub body_s3_key: Option<String>,
    pub body_content_type: Option<String>,
    pub loan_account: Option<String>,
    pub processing_state: String,
    pub error_message: Option<String>,
    pub attachment_count: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ReceivedEmailAttachmentView {
    pub id: i64,
    pub resend_attachment_id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: Option<i32>,
    pub s3_key: Option<String>,
    pub processing_state: String,
}

#[derive(Debug, Clone)]
pub struct TmoCredential {
    pub company_id: String,
    pub account_number: String,
    pub pin: String,
}

#[derive(Debug, Clone)]
pub struct MonarchCredential {
    pub access_token: String,
    pub default_account_id: String,
}
