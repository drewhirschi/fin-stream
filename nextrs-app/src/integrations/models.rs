use std::fmt;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IntegrationConnection {
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

/// Connection plus the provider-staging counts shown by the integration UI.
///
/// Keeping this projection in the integration boundary prevents page handlers
/// from knowing how provider rows are split across the flattened `intg_*`
/// tables.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IntegrationConnectionView {
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
    pub record_count: i64,
    pub normalized_count: i64,
    pub pending_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TmoLoanListItem {
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
    pub is_delinquent: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CapturedProviderRecord {
    pub entity_type: String,
    pub external_id: String,
    pub effective_date: Option<String>,
    pub summary: Option<String>,
    pub amount: Option<f64>,
    pub raw_payload: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NormalizedTmoPayment {
    pub id: i64,
    pub label: Option<String>,
    pub expected_date: String,
    pub actual_date: Option<String>,
    pub amount: f64,
    pub status: String,
    pub check_number: Option<String>,
    pub loan_account: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TmoImportOverview {
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TmoImportLoan {
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

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TmoImportPayment {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TmoAccount {
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

/// Encrypted-at-rest provider credentials. This layer never decrypts or logs
/// them; provider adapters will do so at their explicit boundary.
#[derive(Clone, Eq, PartialEq)]
pub struct TmoCredentialRecord {
    pub connection_id: i64,
    pub company_id: String,
    pub account_number: String,
    pub pin_ciphertext: String,
    pub pin_nonce: String,
    pub key_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl fmt::Debug for TmoCredentialRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TmoCredentialRecord")
            .field("connection_id", &self.connection_id)
            .field("company_id", &self.company_id)
            .field("account_number", &"[redacted]")
            .field("pin_ciphertext", &"[redacted]")
            .field("pin_nonce", &"[redacted]")
            .field("key_version", &self.key_version)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct MonarchCredentialRecord {
    pub connection_id: i64,
    pub access_token_ciphertext: String,
    pub access_token_nonce: String,
    pub default_account_id: String,
    pub key_version: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl fmt::Debug for MonarchCredentialRecord {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MonarchCredentialRecord")
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TmoPaymentEventLink {
    pub tmo_payment_id: i64,
    pub stream_event_id: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PortfolioSnapshot {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub updated_at: String,
}
