use sqlx::SqlitePool;

use crate::models::LoanView;

/// Get all active loans with their display fields.
pub async fn get_active_loans(pool: &SqlitePool) -> Vec<LoanView> {
    sqlx::query_as(
        "SELECT loan_account, borrower_name, property_address, property_city, property_state,
                property_type, percent_owned, note_rate, principal_balance, regular_payment,
                maturity_date, next_payment_date, interest_paid_to, is_delinquent
         FROM tmo_loan WHERE is_active = 1 ORDER BY loan_account",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}
