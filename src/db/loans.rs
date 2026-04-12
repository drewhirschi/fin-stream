use sqlx::PgPool;

use crate::models::{LoanDetailView, LoanView};

/// Get all active loans with their display fields.
pub async fn get_active_loans(pool: &PgPool) -> Vec<LoanView> {
    sqlx::query_as(
        "SELECT loan_account, borrower_name, property_address, property_city, property_state,
                property_type, percent_owned, note_rate, principal_balance, regular_payment,
                maturity_date::text as maturity_date,
                next_payment_date::text as next_payment_date,
                interest_paid_to::text as interest_paid_to,
                is_delinquent
         FROM intg.tmo_import_loan WHERE is_active = 1 ORDER BY loan_account",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn get_loan_by_account(pool: &PgPool, loan_account: &str) -> Option<LoanDetailView> {
    sqlx::query_as(
        "SELECT loan_account,
                borrower_name,
                property_address,
                property_city,
                property_state,
                property_zip,
                property_description,
                property_type,
                occupancy,
                percent_owned,
                note_rate,
                original_balance,
                principal_balance,
                regular_payment,
                payment_frequency,
                maturity_date::text as maturity_date,
                next_payment_date::text as next_payment_date,
                interest_paid_to::text as interest_paid_to,
                billed_through::text as billed_through,
                appraised_value,
                ltv,
                is_delinquent
         FROM intg.tmo_import_loan
         WHERE loan_account = $1
         LIMIT 1",
    )
    .bind(loan_account)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}
