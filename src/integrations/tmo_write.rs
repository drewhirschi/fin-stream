use std::collections::HashSet;

use anyhow::Context;
use chrono::NaiveDate;
use libsql::{Transaction, params};

use crate::providers::tmo::{
    TmoLoanDetail, TmoLoanSummary, TmoOverview, TmoPayment, TmoUserInfo, normalize_check_number,
};

use super::IntegrationResult;

macro_rules! validation_bail {
    ($($argument:tt)*) => {
        return Err(super::IntegrationError::validation(format!($($argument)*)))
    };
}

macro_rules! storage_bail {
    ($($argument:tt)*) => {
        return Err(anyhow::anyhow!($($argument)*).into())
    };
}

macro_rules! configuration_bail {
    ($($argument:tt)*) => {
        return Err(super::IntegrationError::configuration(format!($($argument)*)))
    };
}

/// A complete provider capture. All HTTP work is finished before this value is
/// handed to the transactional repository, so a database write lock is never
/// held across the TMO network boundary.
#[derive(Clone)]
pub struct TmoSyncCapture {
    pub connection_id: i64,
    pub captured_at: String,
    pub snapshot_date: String,
    pub company_id: String,
    pub account_number: String,
    pub user: TmoUserInfo,
    pub overview: TmoOverview,
    pub loans: Vec<TmoLoanSummary>,
    pub loan_details: Vec<TmoLoanDetail>,
    pub loan_detail_failures: i64,
    pub payments: Vec<TmoPayment>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TmoSyncPersistence {
    pub events_upserted: i64,
    pub loans_upserted: i64,
    pub snapshots_created: i64,
}

/// Transaction-only write side of the integrations anti-corruption boundary.
///
/// The caller owns the transaction so it can conditionally transition the
/// durable execution record to `success` before committing the imported data.
pub struct IntegrationWriteRepository<'transaction> {
    transaction: &'transaction Transaction,
}

impl<'transaction> IntegrationWriteRepository<'transaction> {
    pub fn new(transaction: &'transaction Transaction) -> Self {
        Self { transaction }
    }

    /// Persist scheduler observability in the same short transaction that
    /// reads the cadence used to calculate it. `None` truthfully represents a
    /// manual or invalid cadence; this column never acts as the execution
    /// claim or lock.
    pub async fn set_next_scheduled_at(
        &self,
        connection_slug: &str,
        next_scheduled_at: Option<&str>,
        updated_at: &str,
    ) -> IntegrationResult<()> {
        if connection_slug.trim().is_empty() {
            validation_bail!("integration connection slug cannot be empty");
        }
        let changed = self
            .transaction
            .execute(
                "UPDATE intg_integration_connection \
                 SET next_scheduled_at = ?2, \
                     updated_at = CASE \
                         WHEN next_scheduled_at IS ?2 THEN updated_at ELSE ?3 \
                     END \
                 WHERE slug = ?1",
                params![connection_slug, next_scheduled_at, updated_at],
            )
            .await
            .context("update integration next scheduled time")?;
        if changed != 1 {
            storage_bail!("integration connection disappeared while recording its schedule");
        }
        Ok(())
    }

    pub async fn apply_tmo_sync(
        &self,
        capture: &TmoSyncCapture,
    ) -> IntegrationResult<TmoSyncPersistence> {
        validate_capture(capture)?;
        require_tmo_connection(self.transaction, capture.connection_id).await?;
        let stream_id = require_trust_deed_stream(self.transaction).await?;

        let metadata = serde_json::json!({
            "company_id": capture.company_id,
            "account": capture.account_number,
        })
        .to_string();

        let detail_warning = (capture.loan_detail_failures > 0).then(|| {
            format!(
                "{} TMO loan detail request(s) failed; portfolio summaries were synchronized.",
                capture.loan_detail_failures
            )
        });
        self.transaction
            .execute(
                "UPDATE intg_integration_connection \
                 SET metadata = ?2, status = ?3, last_error = ?4, \
                     last_synced_at = ?5, updated_at = ?5 \
                 WHERE id = ?1 AND slug = 'tmo'",
                params![
                    capture.connection_id,
                    metadata,
                    if detail_warning.is_some() {
                        "degraded"
                    } else {
                        "active"
                    },
                    detail_warning,
                    capture.captured_at.clone(),
                ],
            )
            .await
            .context("update TMO connection after sync")?;

        self.transaction
            .execute(
                "INSERT INTO intg_tmo_account ( \
                    id, company_id, account_number, source_rec_id, display_name, email, \
                    last_login_at, created_at, updated_at \
                 ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?6, ?6) \
                 ON CONFLICT(id) DO UPDATE SET \
                    company_id = excluded.company_id, \
                    account_number = excluded.account_number, \
                    source_rec_id = excluded.source_rec_id, \
                    display_name = excluded.display_name, \
                    email = excluded.email, \
                    last_login_at = excluded.last_login_at, \
                    updated_at = excluded.updated_at",
                params![
                    capture.user.company_id.clone(),
                    capture.user.account.clone(),
                    capture.user.source_rec_id.clone(),
                    capture.user.name.clone(),
                    capture.user.email.clone(),
                    capture.captured_at.clone(),
                ],
            )
            .await
            .context("upsert TMO account")?;

        let overview_payload =
            serde_json::to_string(&capture.overview).context("serialize TMO overview capture")?;
        self.transaction
            .execute(
                "INSERT INTO intg_tmo_import_overview ( \
                    connection_id, snapshot_date, portfolio_value, portfolio_yield, \
                    portfolio_count, ytd_interest, ytd_principal, trust_balance, \
                    outstanding_checks, service_fees, processing_state, raw_payload, \
                    created_at, updated_at \
                 ) VALUES ( \
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'captured', ?11, ?12, ?12 \
                 ) \
                 ON CONFLICT(connection_id, snapshot_date) DO UPDATE SET \
                    portfolio_value = excluded.portfolio_value, \
                    portfolio_yield = excluded.portfolio_yield, \
                    portfolio_count = excluded.portfolio_count, \
                    ytd_interest = excluded.ytd_interest, \
                    ytd_principal = excluded.ytd_principal, \
                    trust_balance = excluded.trust_balance, \
                    outstanding_checks = excluded.outstanding_checks, \
                    service_fees = excluded.service_fees, \
                    processing_state = excluded.processing_state, \
                    raw_payload = excluded.raw_payload, \
                    updated_at = excluded.updated_at",
                params![
                    capture.connection_id,
                    capture.snapshot_date.clone(),
                    capture.overview.portfolio_value,
                    capture.overview.portfolio_yield,
                    i64::from(capture.overview.portfolio_count),
                    capture.overview.ytd_interest,
                    capture.overview.ytd_principal,
                    capture.overview.trust_balance,
                    capture.overview.outstanding_checks_value,
                    capture.overview.ytd_serv_fees,
                    overview_payload,
                    capture.captured_at.clone(),
                ],
            )
            .await
            .context("upsert TMO overview capture")?;

        self.transaction
            .execute(
                "INSERT INTO portfolio_snapshot ( \
                    snapshot_date, portfolio_value, portfolio_yield, portfolio_count, \
                    ytd_interest, ytd_principal, trust_balance, outstanding_checks, \
                    service_fees, synced_at \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) \
                 ON CONFLICT(snapshot_date) DO UPDATE SET \
                    portfolio_value = excluded.portfolio_value, \
                    portfolio_yield = excluded.portfolio_yield, \
                    portfolio_count = excluded.portfolio_count, \
                    ytd_interest = excluded.ytd_interest, \
                    ytd_principal = excluded.ytd_principal, \
                    trust_balance = excluded.trust_balance, \
                    outstanding_checks = excluded.outstanding_checks, \
                    service_fees = excluded.service_fees, \
                    synced_at = excluded.synced_at",
                params![
                    capture.snapshot_date.clone(),
                    capture.overview.portfolio_value,
                    capture.overview.portfolio_yield,
                    i64::from(capture.overview.portfolio_count),
                    capture.overview.ytd_interest,
                    capture.overview.ytd_principal,
                    capture.overview.trust_balance,
                    capture.overview.outstanding_checks_value,
                    capture.overview.ytd_serv_fees,
                    capture.captured_at.clone(),
                ],
            )
            .await
            .context("upsert portfolio snapshot")?;

        // Portfolio captures are complete snapshots. Stage the previous set as
        // inactive before reactivating every loan present in this capture so
        // paid-off or removed loans cannot linger in active portfolio views.
        self.transaction
            .execute(
                "UPDATE intg_tmo_import_loan \
                 SET is_active = 0, updated_at = ?2 \
                 WHERE connection_id = ?1 AND is_active = 1",
                params![capture.connection_id, capture.captured_at.clone()],
            )
            .await
            .context("stage existing TMO loans for snapshot replacement")?;

        for loan in &capture.loans {
            let maturity_date = required_date("loan maturity date", &loan.maturity_date)?;
            let next_payment_date =
                required_date("loan next-payment date", &loan.next_payment_date)?;
            let interest_paid_to =
                required_date("loan interest-paid-to date", &loan.interest_paid_to_date)?;
            let billed_through =
                optional_date("loan billed-through date", loan.billed_through.as_deref())?;
            let raw_payload =
                serde_json::to_string(loan).context("serialize TMO loan summary capture")?;
            self.transaction
                .execute(
                    "INSERT INTO intg_tmo_import_loan ( \
                        connection_id, stream_id, loan_account, borrower_name, \
                        property_address, property_city, property_state, property_zip, \
                        percent_owned, interest_rate, note_rate, maturity_date, \
                        term_left_months, next_payment_date, interest_paid_to, billed_through, \
                        regular_payment, loan_balance, principal_balance, is_delinquent, \
                        is_active, raw_summary_payload, summary_imported_at, created_at, updated_at \
                     ) VALUES ( \
                        ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?11, ?12, ?13, \
                        ?14, ?15, ?16, ?17, ?17, ?18, 1, ?19, ?20, ?20, ?20 \
                     ) \
                     ON CONFLICT(connection_id, loan_account) DO UPDATE SET \
                        stream_id = COALESCE(intg_tmo_import_loan.stream_id, excluded.stream_id), \
                        borrower_name = excluded.borrower_name, \
                        property_address = excluded.property_address, \
                        property_city = excluded.property_city, \
                        property_state = excluded.property_state, \
                        property_zip = excluded.property_zip, \
                        percent_owned = excluded.percent_owned, \
                        interest_rate = excluded.interest_rate, \
                        note_rate = excluded.note_rate, \
                        maturity_date = excluded.maturity_date, \
                        term_left_months = excluded.term_left_months, \
                        next_payment_date = excluded.next_payment_date, \
                        interest_paid_to = excluded.interest_paid_to, \
                        billed_through = excluded.billed_through, \
                        regular_payment = excluded.regular_payment, \
                        loan_balance = excluded.loan_balance, \
                        principal_balance = excluded.principal_balance, \
                        is_delinquent = excluded.is_delinquent, \
                        is_active = 1, \
                        raw_summary_payload = excluded.raw_summary_payload, \
                        summary_imported_at = excluded.summary_imported_at, \
                        updated_at = excluded.updated_at",
                    params![
                        capture.connection_id,
                        stream_id,
                        loan.loan_account.clone(),
                        loan.borrower_name.clone(),
                        loan.primary_street.clone(),
                        loan.primary_city.clone(),
                        loan.primary_state.clone(),
                        loan.primary_zip.clone(),
                        loan.percent_owned,
                        loan.interest_rate,
                        maturity_date,
                        i64::from(loan.term_left),
                        next_payment_date,
                        interest_paid_to,
                        billed_through,
                        loan.regular_payment,
                        loan.loan_balance,
                        bool_i64(loan.is_delinquent),
                        raw_payload,
                        capture.captured_at.clone(),
                    ],
                )
                .await
                .context("upsert TMO loan summary")?;
        }

        for detail in &capture.loan_details {
            let maturity_date = required_date("loan detail maturity date", &detail.maturity_date)?;
            let next_payment_date =
                required_date("loan detail next-payment date", &detail.next_payment_date)?;
            let interest_paid_to = required_date(
                "loan detail interest-paid-to date",
                &detail.interest_paid_to_date,
            )?;
            let raw_payload =
                serde_json::to_string(detail).context("serialize TMO loan detail capture")?;
            let changed = self
                .transaction
                .execute(
                    "UPDATE intg_tmo_import_loan \
                     SET borrower_name = ?3, property_address = ?4, property_city = ?5, \
                         property_state = ?6, property_zip = ?7, property_description = ?8, \
                         property_type = ?9, property_priority = ?10, occupancy = ?11, \
                         ltv = ?12, appraised_value = ?13, priority = ?14, \
                         original_balance = ?15, principal_balance = ?16, note_rate = ?17, \
                         maturity_date = ?18, next_payment_date = ?19, interest_paid_to = ?20, \
                         regular_payment = ?21, payment_frequency = ?22, loan_type = ?23, \
                         raw_detail_payload = ?24, detail_imported_at = ?25, updated_at = ?25 \
                     WHERE connection_id = ?1 AND loan_account = ?2",
                    params![
                        capture.connection_id,
                        detail.loan_account.clone(),
                        detail.borrower_name.clone(),
                        detail.primary_street.clone(),
                        detail.primary_city.clone(),
                        detail.primary_state.clone(),
                        detail.primary_zip.clone(),
                        detail.property_description.clone(),
                        detail.property_type.clone(),
                        detail.property_priority.map(i64::from),
                        detail.occupancy.clone(),
                        detail.ltv,
                        detail.appraised_value,
                        detail.priority.map(i64::from),
                        detail.original_balance,
                        detail.principal_balance,
                        detail.note_rate,
                        maturity_date,
                        next_payment_date,
                        interest_paid_to,
                        detail.regular_payment,
                        detail.payment_frequency.clone(),
                        i64::from(detail.loan_type),
                        raw_payload,
                        capture.captured_at.clone(),
                    ],
                )
                .await
                .context("apply TMO loan detail")?;
            if changed != 1 {
                validation_bail!("TMO loan detail did not match its captured portfolio summary");
            }
        }

        self.replace_payment_projection(capture, stream_id).await?;

        Ok(TmoSyncPersistence {
            events_upserted: i64::try_from(capture.payments.len())
                .context("TMO payment count exceeds durable counter range")?,
            loans_upserted: i64::try_from(capture.loans.len())
                .context("TMO loan count exceeds durable counter range")?,
            // The legacy counter treated both INSERT and conflict UPDATE as a
            // snapshot created; preserve that operator-facing behavior.
            snapshots_created: 1,
        })
    }

    pub async fn mark_tmo_error(
        &self,
        connection_id: i64,
        public_message: &str,
        updated_at: &str,
    ) -> IntegrationResult<()> {
        let changed = self
            .transaction
            .execute(
                "UPDATE intg_integration_connection \
                 SET status = 'error', last_error = ?2, updated_at = ?3 \
                 WHERE id = ?1 AND slug = 'tmo'",
                params![connection_id, public_message, updated_at],
            )
            .await
            .context("mark TMO connection error")?;
        if changed != 1 {
            storage_bail!("TMO integration connection disappeared during error transition");
        }
        Ok(())
    }

    /// Typed settings write retained at the integration boundary for provider
    /// values that are genuinely settings. The TMO sync does not invent any
    /// new setting keys; its portfolio data remains in snapshot tables.
    pub async fn upsert_setting(
        &self,
        key: &str,
        value: &str,
        updated_at: &str,
    ) -> IntegrationResult<()> {
        if key.trim().is_empty() {
            validation_bail!("setting key cannot be empty");
        }
        self.transaction
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                params![key.trim(), value, updated_at],
            )
            .await
            .context("upsert integration setting")?;
        Ok(())
    }

    async fn replace_payment_projection(
        &self,
        capture: &TmoSyncCapture,
        stream_id: i64,
    ) -> IntegrationResult<()> {
        self.transaction
            .execute(
                "UPDATE intg_tmo_import_payment SET processing_state = 'stale' \
                 WHERE connection_id = ?1",
                params![capture.connection_id],
            )
            .await
            .context("stage existing TMO payments for snapshot replacement")?;

        for payment in &capture.payments {
            let check_date = required_date("payment check date", &payment.check_date)?;
            let external_id = payment_external_id(payment, &check_date)?;
            let check_number = normalize_check_number(&payment.check_number);
            let raw_payload =
                serde_json::to_string(payment).context("serialize TMO payment capture")?;
            let payment_id = query_i64(
                self.transaction,
                "INSERT INTO intg_tmo_import_payment ( \
                    connection_id, external_id, loan_account, borrower_name, property_name, \
                    check_number, check_date, amount, service_fee, interest, principal, \
                    charges, late_charges, other, processing_state, \
                    normalized_event_source_id, raw_payload, imported_at, updated_at \
                 ) VALUES ( \
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
                    'normalized', ?2, ?15, ?16, ?16 \
                 ) \
                 ON CONFLICT(connection_id, external_id) DO UPDATE SET \
                    loan_account = excluded.loan_account, \
                    borrower_name = excluded.borrower_name, \
                    property_name = excluded.property_name, \
                    check_number = excluded.check_number, \
                    check_date = excluded.check_date, \
                    amount = excluded.amount, \
                    service_fee = excluded.service_fee, \
                    interest = excluded.interest, \
                    principal = excluded.principal, \
                    charges = excluded.charges, \
                    late_charges = excluded.late_charges, \
                    other = excluded.other, \
                    processing_state = 'normalized', \
                    normalized_event_source_id = excluded.normalized_event_source_id, \
                    raw_payload = excluded.raw_payload, \
                    updated_at = excluded.updated_at \
                 RETURNING id",
                params![
                    capture.connection_id,
                    external_id.clone(),
                    payment.loan_account.clone(),
                    payment.borrower_name.clone(),
                    payment.property_name.clone(),
                    check_number.clone(),
                    check_date.clone(),
                    payment.amount,
                    payment.service_fee,
                    payment.interest,
                    payment.principal,
                    payment.charges,
                    payment.late_charges,
                    payment.other,
                    raw_payload,
                    capture.captured_at.clone(),
                ],
                "upsert TMO payment",
            )
            .await?;

            let amount = payment.amount.abs();
            let is_received = check_number.is_some();
            let label = format!("{} - {}", payment.borrower_name, payment.property_name);
            let event_id = query_i64(
                self.transaction,
                "INSERT INTO stream_event ( \
                    stream_id, account_id, label, expected_date, amount, actual_date, \
                    actual_amount, status, source_id, source_type, created_at, updated_at \
                 ) VALUES ( \
                    ?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'tmo_history', ?9, ?9 \
                 ) \
                 ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET \
                    label = excluded.label, \
                    expected_date = excluded.expected_date, \
                    amount = excluded.amount, \
                    actual_date = excluded.actual_date, \
                    actual_amount = excluded.actual_amount, \
                    status = excluded.status, \
                    updated_at = excluded.updated_at \
                 RETURNING id",
                params![
                    stream_id,
                    label,
                    check_date.clone(),
                    amount,
                    if is_received { Some(check_date) } else { None },
                    if is_received { Some(amount) } else { None },
                    if is_received { "received" } else { "confirmed" },
                    external_id,
                    capture.captured_at.clone(),
                ],
                "upsert normalized TMO stream event",
            )
            .await?;

            self.transaction
                .execute(
                    "INSERT INTO intg_tmo_payment_event_link ( \
                        tmo_payment_id, stream_event_id, created_at \
                     ) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(tmo_payment_id) DO UPDATE SET stream_event_id = excluded.stream_event_id",
                    params![payment_id, event_id, capture.captured_at.clone()],
                )
                .await
                .context("link TMO payment to normalized stream event")?;
        }

        self.transaction
            .execute(
                "DELETE FROM stream_event \
                 WHERE id IN ( \
                    SELECT link.stream_event_id \
                    FROM intg_tmo_payment_event_link link \
                    JOIN intg_tmo_import_payment payment ON payment.id = link.tmo_payment_id \
                    WHERE payment.connection_id = ?1 AND payment.processing_state = 'stale' \
                 )",
                params![capture.connection_id],
            )
            .await
            .context("remove stream events for payments absent from TMO snapshot")?;
        self.transaction
            .execute(
                "DELETE FROM intg_tmo_import_payment \
                 WHERE connection_id = ?1 AND processing_state = 'stale'",
                params![capture.connection_id],
            )
            .await
            .context("remove payments absent from TMO snapshot")?;
        self.transaction
            .execute(
                "DELETE FROM stream_event \
                 WHERE stream_id = ?1 AND source_type = 'tmo_history' \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM intg_tmo_payment_event_link link \
                       JOIN intg_tmo_import_payment payment ON payment.id = link.tmo_payment_id \
                       WHERE link.stream_event_id = stream_event.id AND payment.connection_id = ?2 \
                   )",
                params![stream_id, capture.connection_id],
            )
            .await
            .context("remove orphaned TMO history events")?;

        // This source type was emitted by the pre-stream-schedule TMO code.
        // It is safe to remove; canonical schedule projections and user
        // overrides use `stream_schedule` and are deliberately untouched.
        self.transaction
            .execute(
                "DELETE FROM stream_event \
                 WHERE status = 'projected' AND source_type = 'schedule'",
                (),
            )
            .await
            .context("remove legacy TMO schedule projections")?;
        Ok(())
    }
}

async fn require_tmo_connection(
    transaction: &Transaction,
    connection_id: i64,
) -> IntegrationResult<()> {
    let mut rows = transaction
        .query(
            "SELECT 1 FROM intg_integration_connection WHERE id = ?1 AND slug = 'tmo' LIMIT 1",
            params![connection_id],
        )
        .await
        .context("verify TMO integration connection")?;
    if rows
        .next()
        .await
        .context("read TMO integration connection check")?
        .is_none()
    {
        storage_bail!("TMO integration connection is missing");
    }
    Ok(())
}

async fn require_trust_deed_stream(transaction: &Transaction) -> IntegrationResult<i64> {
    let mut rows = transaction
        .query(
            "SELECT id FROM stream \
             WHERE is_active = 1 AND (type = 'mortgage_portfolio' OR kind = 'tmo_trust') \
             ORDER BY CASE WHEN type = 'mortgage_portfolio' THEN 0 ELSE 1 END, id LIMIT 1",
            (),
        )
        .await
        .context("query Trust Deeds stream for TMO normalization")?;
    let Some(row) = rows
        .next()
        .await
        .context("read Trust Deeds stream for TMO normalization")?
    else {
        configuration_bail!(
            "Trust Deeds stream is missing; run the explicit application bootstrap"
        );
    };
    Ok(row.get(0).context("decode Trust Deeds stream id")?)
}

async fn query_i64(
    transaction: &Transaction,
    sql: &str,
    parameters: impl libsql::params::IntoParams,
    label: &'static str,
) -> IntegrationResult<i64> {
    let mut rows = transaction
        .query(sql, parameters)
        .await
        .with_context(|| label.to_owned())?;
    let row = rows
        .next()
        .await
        .with_context(|| format!("read {label}"))?
        .with_context(|| format!("{label} returned no row"))?;
    Ok(row.get(0).with_context(|| format!("decode {label} id"))?)
}

fn validate_capture(capture: &TmoSyncCapture) -> IntegrationResult<()> {
    if capture.connection_id <= 0
        || capture.company_id.trim().is_empty()
        || capture.account_number.trim().is_empty()
    {
        validation_bail!("TMO capture identity is invalid");
    }
    required_date("TMO snapshot date", &capture.snapshot_date)?;
    if capture.overview.portfolio_count < 0
        || usize::try_from(capture.overview.portfolio_count)
            .ok()
            .is_none_or(|count| count != capture.loans.len())
    {
        validation_bail!("TMO portfolio count does not match the complete loan capture");
    }
    require_finite(
        "TMO overview",
        &[
            capture.overview.portfolio_value,
            capture.overview.portfolio_yield,
            capture.overview.ytd_interest,
            capture.overview.ytd_principal,
            capture.overview.trust_balance,
            capture.overview.outstanding_checks_value,
            capture.overview.ytd_serv_fees,
        ],
    )?;

    let mut loan_accounts = HashSet::new();
    for loan in &capture.loans {
        if loan.loan_account.trim().is_empty() || !loan_accounts.insert(&loan.loan_account) {
            validation_bail!("TMO portfolio contains an empty or duplicate loan account");
        }
        require_finite(
            "TMO loan summary",
            &[
                loan.percent_owned,
                loan.interest_rate,
                loan.regular_payment,
                loan.loan_balance,
            ],
        )?;
    }
    for detail in &capture.loan_details {
        if !loan_accounts.contains(&detail.loan_account) {
            validation_bail!("TMO loan detail is absent from the captured portfolio");
        }
        require_finite(
            "TMO loan detail",
            &[
                detail.original_balance,
                detail.principal_balance,
                detail.note_rate,
                detail.regular_payment,
            ],
        )?;
        require_optional_finite(
            "TMO optional loan detail",
            &[detail.ltv, detail.appraised_value],
        )?;
    }
    let detail_attempts = i64::try_from(capture.loan_details.len())
        .context("TMO loan-detail count exceeds durable counter range")?
        .checked_add(capture.loan_detail_failures)
        .context("TMO loan-detail attempt count overflowed")?;
    if capture.loan_detail_failures < 0
        || detail_attempts
            != i64::try_from(capture.loans.len())
                .context("TMO loan count exceeds durable counter range")?
    {
        validation_bail!("TMO loan-detail outcome counters do not match the portfolio");
    }

    let mut payment_ids = HashSet::new();
    for payment in &capture.payments {
        if payment.loan_account.trim().is_empty() {
            validation_bail!("TMO payment loan account is empty");
        }
        require_finite(
            "TMO payment",
            &[
                payment.amount,
                payment.service_fee,
                payment.interest,
                payment.principal,
                payment.charges,
                payment.late_charges,
                payment.other,
            ],
        )?;
        let check_date = required_date("TMO payment check date", &payment.check_date)?;
        let external_id = payment_external_id(payment, &check_date)?;
        if !payment_ids.insert(external_id) {
            validation_bail!("TMO payment history contains a duplicate canonical payment identity");
        }
    }
    Ok(())
}

fn payment_external_id(payment: &TmoPayment, check_date: &str) -> IntegrationResult<String> {
    let cents = payment.amount * 100.0;
    if !cents.is_finite() || cents < i64::MIN as f64 || cents > i64::MAX as f64 {
        validation_bail!("TMO payment amount cannot be represented as canonical cents");
    }
    Ok(format!(
        "history:{}:{}:{}",
        payment.loan_account,
        check_date,
        cents.round() as i64
    ))
}

fn required_date(label: &str, value: &str) -> IntegrationResult<String> {
    let value = value.split('T').next().unwrap_or(value).trim();
    if NaiveDate::parse_from_str(value, "%Y-%m-%d").is_err() {
        validation_bail!("{label} is not a valid ISO date");
    }
    Ok(value.to_owned())
}

fn optional_date(label: &str, value: Option<&str>) -> IntegrationResult<Option<String>> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| required_date(label, value))
        .transpose()
}

fn require_finite(label: &str, values: &[f64]) -> IntegrationResult<()> {
    if values.iter().any(|value| !value.is_finite()) {
        validation_bail!("{label} contains a non-finite number");
    }
    Ok(())
}

fn require_optional_finite(label: &str, values: &[Option<f64>]) -> IntegrationResult<()> {
    if values.iter().flatten().any(|value| !value.is_finite()) {
        validation_bail!("{label} contains a non-finite number");
    }
    Ok(())
}

const fn bool_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
