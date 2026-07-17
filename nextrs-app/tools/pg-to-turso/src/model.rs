use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AppUserRow {
    pub id: i64,
    pub email: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub is_active: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AccountRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub balance: Option<f64>,
    pub balance_as_of_date: Option<String>,
    pub source_type: Option<String>,
    pub source_ref: Option<String>,
    pub metadata: Option<String>,
    pub balance_updated_at: Option<String>,
    pub is_primary: i64,
    pub is_active: i64,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StreamRow {
    pub id: i64,
    pub name: String,
    pub stream_type: String,
    pub kind: String,
    pub direction: String,
    pub amount_certainty: String,
    pub description: Option<String>,
    pub default_account_id: Option<i64>,
    pub configuration: Option<String>,
    pub parent_id: Option<i64>,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StreamViewRow {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_default: i64,
    pub is_active: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StreamViewStreamRow {
    pub stream_view_id: i64,
    pub stream_id: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StreamScheduleRow {
    pub id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub label: Option<String>,
    pub amount: f64,
    pub frequency: String,
    pub day_of_month: Option<i64>,
    pub start_date: String,
    pub end_date: Option<String>,
    pub is_active: i64,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StreamEventRow {
    pub id: i64,
    pub stream_id: i64,
    pub account_id: Option<i64>,
    pub label: Option<String>,
    pub expected_date: String,
    pub amount: f64,
    pub override_label: Option<String>,
    pub has_label_override: i64,
    pub override_date: Option<String>,
    pub override_amount: Option<f64>,
    pub override_account_id: Option<i64>,
    pub has_account_override: i64,
    pub actual_date: Option<String>,
    pub actual_amount: Option<f64>,
    pub status: String,
    pub is_excluded: i64,
    pub exclusion_reason: Option<String>,
    pub source_id: Option<String>,
    pub source_type: Option<String>,
    pub metadata: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct IntegrationConnectionRow {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub provider: String,
    pub status: String,
    pub sync_cadence: String,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub metadata: Option<String>,
    pub next_scheduled_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TmoImportOverviewRow {
    pub id: i64,
    pub connection_id: i64,
    pub snapshot_date: String,
    pub portfolio_value: Option<f64>,
    pub portfolio_yield: Option<f64>,
    pub portfolio_count: Option<i64>,
    pub ytd_interest: Option<f64>,
    pub ytd_principal: Option<f64>,
    pub trust_balance: Option<f64>,
    pub outstanding_checks: Option<f64>,
    pub service_fees: Option<f64>,
    pub processing_state: String,
    pub raw_payload: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TmoImportLoanRow {
    pub id: i64,
    pub connection_id: i64,
    pub stream_id: Option<i64>,
    pub loan_account: String,
    pub borrower_name: Option<String>,
    pub property_address: Option<String>,
    pub property_city: Option<String>,
    pub property_state: Option<String>,
    pub property_zip: Option<String>,
    pub property_description: Option<String>,
    pub property_type: Option<String>,
    pub property_priority: Option<i64>,
    pub occupancy: Option<String>,
    pub appraised_value: Option<f64>,
    pub ltv: Option<f64>,
    pub percent_owned: Option<f64>,
    pub priority: Option<i64>,
    pub loan_type: Option<i64>,
    pub interest_rate: Option<f64>,
    pub note_rate: Option<f64>,
    pub original_balance: Option<f64>,
    pub loan_balance: Option<f64>,
    pub principal_balance: Option<f64>,
    pub regular_payment: Option<f64>,
    pub payment_frequency: Option<String>,
    pub maturity_date: Option<String>,
    pub next_payment_date: Option<String>,
    pub interest_paid_to: Option<String>,
    pub billed_through: Option<String>,
    pub term_left_months: Option<i64>,
    pub is_delinquent: Option<i64>,
    pub is_active: Option<i64>,
    pub raw_summary_payload: Option<String>,
    pub raw_detail_payload: Option<String>,
    pub summary_imported_at: Option<String>,
    pub detail_imported_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TmoImportPaymentRow {
    pub id: i64,
    pub connection_id: i64,
    pub external_id: String,
    pub loan_account: String,
    pub borrower_name: String,
    pub property_name: String,
    pub check_number: Option<String>,
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
    pub imported_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmoAccountRow {
    pub id: i64,
    pub company_id: String,
    pub account_number: String,
    pub source_rec_id: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub last_login_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmoCredentialRow {
    pub connection_id: i64,
    pub company_id: String,
    pub account_number: String,
    pub pin_ciphertext: String,
    pub pin_nonce: String,
    pub key_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl fmt::Debug for TmoCredentialRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TmoCredentialRow")
            .field("connection_id", &self.connection_id)
            .field("company_id", &self.company_id)
            .field("account_number", &self.account_number)
            .field("pin_ciphertext", &"[redacted]")
            .field("pin_nonce", &"[redacted]")
            .field("key_version", &self.key_version)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct MonarchCredentialRow {
    pub connection_id: i64,
    pub access_token_ciphertext: String,
    pub access_token_nonce: String,
    pub default_account_id: String,
    pub key_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl fmt::Debug for MonarchCredentialRow {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MonarchCredentialRow")
            .field("connection_id", &self.connection_id)
            .field("access_token_ciphertext", &"[redacted]")
            .field("access_token_nonce", &"[redacted]")
            .field("default_account_id", &self.default_account_id)
            .field("key_version", &self.key_version)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmoPaymentEventLinkRow {
    pub tmo_payment_id: i64,
    pub stream_event_id: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PortfolioSnapshotRow {
    pub id: i64,
    pub snapshot_date: String,
    pub portfolio_value: Option<f64>,
    pub portfolio_yield: Option<f64>,
    pub portfolio_count: Option<i64>,
    pub ytd_interest: Option<f64>,
    pub ytd_principal: Option<f64>,
    pub trust_balance: Option<f64>,
    pub outstanding_checks: Option<f64>,
    pub service_fees: Option<f64>,
    pub synced_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SettingRow {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SyncLogRow {
    pub id: i64,
    pub connection_slug: String,
    pub scheduled_for: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub endpoints_hit: Option<String>,
    pub events_upserted: i64,
    pub loans_upserted: i64,
    pub snapshots_created: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct LoanWorkspaceRow {
    pub id: i64,
    pub connection_id: i64,
    pub loan_account: String,
    pub redfin_url: Option<String>,
    pub zillow_url: Option<String>,
    pub decision_status: Option<String>,
    pub target_contribution: Option<f64>,
    pub actual_contribution: Option<f64>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoanWorkspacePhotoRow {
    pub id: i64,
    pub connection_id: i64,
    pub loan_account: String,
    pub provider: String,
    pub caption: Option<String>,
    pub source_url: String,
    pub image_url: String,
    pub sort_order: i64,
    pub is_featured: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReceivedEmailRow {
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
    pub raw_webhook_payload: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReceivedEmailAttachmentRow {
    pub id: i64,
    pub email_id: i64,
    pub resend_attachment_id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: Option<i64>,
    pub s3_key: Option<String>,
    pub processing_state: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Dataset {
    pub app_users: Vec<AppUserRow>,
    pub accounts: Vec<AccountRow>,
    pub streams: Vec<StreamRow>,
    pub stream_views: Vec<StreamViewRow>,
    pub stream_view_streams: Vec<StreamViewStreamRow>,
    pub stream_schedules: Vec<StreamScheduleRow>,
    pub stream_events: Vec<StreamEventRow>,
    pub integration_connections: Vec<IntegrationConnectionRow>,
    pub tmo_import_overviews: Vec<TmoImportOverviewRow>,
    pub tmo_import_loans: Vec<TmoImportLoanRow>,
    pub tmo_import_payments: Vec<TmoImportPaymentRow>,
    pub tmo_accounts: Vec<TmoAccountRow>,
    pub tmo_credentials: Vec<TmoCredentialRow>,
    pub monarch_credentials: Vec<MonarchCredentialRow>,
    pub tmo_payment_event_links: Vec<TmoPaymentEventLinkRow>,
    pub portfolio_snapshots: Vec<PortfolioSnapshotRow>,
    pub settings: Vec<SettingRow>,
    pub sync_logs: Vec<SyncLogRow>,
    pub loan_workspaces: Vec<LoanWorkspaceRow>,
    pub loan_workspace_photos: Vec<LoanWorkspacePhotoRow>,
    pub received_emails: Vec<ReceivedEmailRow>,
    pub received_email_attachments: Vec<ReceivedEmailAttachmentRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SequenceState {
    pub table: String,
    pub source_sequence: String,
    pub source_effective_next: i64,
    pub imported_max: Option<i64>,
    pub target_effective_next: i64,
}
