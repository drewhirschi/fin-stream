pub mod tmo;

use serde::Serialize;

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

// Integration-layer view types are defined in `models::tmo` and re-exported
// here for template ergonomics. The quarantine boundary check treats these
// re-exports as opaque — anything that explicitly needs TMO-shaped fields
// should import from `models::tmo` directly.
pub use tmo::{
    LoanDetailView, LoanView, PaymentView, TmoCredential, TmoImportPaymentView, TmoLoanDetail,
    TmoLoanSummary, TmoLoginData, TmoOverview, TmoPaginatedResponse, TmoPayment, TmoResponse,
    TmoUserInfo,
};

// ── View models for templates ──

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
    pub next_scheduled_at: Option<String>,
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
pub struct MonarchCredential {
    pub access_token: String,
    pub default_account_id: String,
}
