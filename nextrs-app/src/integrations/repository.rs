use anyhow::Context;
use libsql::{Connection, Row, Rows, params};

use super::{
    CapturedProviderRecord, IntegrationConnection, IntegrationConnectionView, IntegrationResult,
    MonarchCredentialRecord, NormalizedTmoPayment, PortfolioSnapshot, Setting, TmoAccount,
    TmoCredentialRecord, TmoImportLoan, TmoImportOverview, TmoImportPayment, TmoLoanListItem,
    TmoPaymentEventLink,
};

/// Read-only anti-corruption boundary for provider-shaped data. Provider
/// clients may be added behind this module later; finance code should not
/// query `intg_*` tables directly.
pub struct IntegrationRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> IntegrationRepository<'connection> {
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub async fn list_connections(&self) -> IntegrationResult<Vec<IntegrationConnection>> {
        let rows = self
            .connection
            .query(
                "SELECT id, slug, name, provider, status, sync_cadence, last_synced_at, \
                        last_error, metadata, next_scheduled_at, created_at, updated_at \
                 FROM intg_integration_connection ORDER BY slug COLLATE NOCASE, id",
                (),
            )
            .await
            .context("query integration connections")?;
        collect(rows, connection_from_row, "integration connection").await
    }

    pub async fn list_connection_views(&self) -> IntegrationResult<Vec<IntegrationConnectionView>> {
        let rows = self
            .connection
            .query(
                "SELECT c.id, c.slug, c.name, c.provider, c.status, c.sync_cadence, \
                        c.last_synced_at, c.last_error, c.metadata, c.next_scheduled_at, \
                        c.created_at, c.updated_at, \
                        COALESCE(o.record_count, 0) + COALESCE(l.record_count, 0) \
                            + COALESCE(p.record_count, 0) AS record_count, \
                        COALESCE(p.normalized_count, 0) AS normalized_count, \
                        COALESCE(p.pending_count, 0) AS pending_count \
                 FROM intg_integration_connection c \
                 LEFT JOIN ( \
                     SELECT connection_id, COUNT(*) AS record_count \
                     FROM intg_tmo_import_overview GROUP BY connection_id \
                 ) o ON o.connection_id = c.id \
                 LEFT JOIN ( \
                     SELECT connection_id, COUNT(*) AS record_count \
                     FROM intg_tmo_import_loan GROUP BY connection_id \
                 ) l ON l.connection_id = c.id \
                 LEFT JOIN ( \
                     SELECT connection_id, COUNT(*) AS record_count, \
                            SUM(CASE WHEN processing_state = 'normalized' THEN 1 ELSE 0 END) \
                                AS normalized_count, \
                            SUM(CASE WHEN processing_state <> 'normalized' THEN 1 ELSE 0 END) \
                                AS pending_count \
                     FROM intg_tmo_import_payment GROUP BY connection_id \
                 ) p ON p.connection_id = c.id \
                 ORDER BY c.name COLLATE NOCASE, c.id",
                (),
            )
            .await
            .context("query integration connection views")?;
        collect(
            rows,
            connection_view_from_row,
            "integration connection view",
        )
        .await
    }

    pub async fn connection_view_by_slug(
        &self,
        slug: &str,
    ) -> IntegrationResult<Option<IntegrationConnectionView>> {
        let rows = self
            .connection
            .query(
                "SELECT c.id, c.slug, c.name, c.provider, c.status, c.sync_cadence, \
                        c.last_synced_at, c.last_error, c.metadata, c.next_scheduled_at, \
                        c.created_at, c.updated_at, \
                        COALESCE(o.record_count, 0) + COALESCE(l.record_count, 0) \
                            + COALESCE(p.record_count, 0) AS record_count, \
                        COALESCE(p.normalized_count, 0) AS normalized_count, \
                        COALESCE(p.pending_count, 0) AS pending_count \
                 FROM intg_integration_connection c \
                 LEFT JOIN ( \
                     SELECT connection_id, COUNT(*) AS record_count \
                     FROM intg_tmo_import_overview GROUP BY connection_id \
                 ) o ON o.connection_id = c.id \
                 LEFT JOIN ( \
                     SELECT connection_id, COUNT(*) AS record_count \
                     FROM intg_tmo_import_loan GROUP BY connection_id \
                 ) l ON l.connection_id = c.id \
                 LEFT JOIN ( \
                     SELECT connection_id, COUNT(*) AS record_count, \
                            SUM(CASE WHEN processing_state = 'normalized' THEN 1 ELSE 0 END) \
                                AS normalized_count, \
                            SUM(CASE WHEN processing_state <> 'normalized' THEN 1 ELSE 0 END) \
                                AS pending_count \
                     FROM intg_tmo_import_payment GROUP BY connection_id \
                 ) p ON p.connection_id = c.id \
                 WHERE c.slug = ?1 LIMIT 1",
                params![slug],
            )
            .await
            .context("query integration connection view by slug")?;
        one(
            rows,
            connection_view_from_row,
            "integration connection view",
        )
        .await
    }

    pub async fn connection_by_slug(
        &self,
        slug: &str,
    ) -> IntegrationResult<Option<IntegrationConnection>> {
        let rows = self
            .connection
            .query(
                "SELECT id, slug, name, provider, status, sync_cadence, last_synced_at, \
                        last_error, metadata, next_scheduled_at, created_at, updated_at \
                 FROM intg_integration_connection WHERE slug = ?1 LIMIT 1",
                params![slug],
            )
            .await
            .context("query integration connection by slug")?;
        one(rows, connection_from_row, "integration connection").await
    }

    pub async fn list_tmo_overviews(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Vec<TmoImportOverview>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, snapshot_date, portfolio_value, portfolio_yield, \
                        portfolio_count, ytd_interest, ytd_principal, trust_balance, \
                        outstanding_checks, service_fees, processing_state, raw_payload, \
                        created_at, updated_at \
                 FROM intg_tmo_import_overview WHERE connection_id = ?1 \
                 ORDER BY snapshot_date DESC, id DESC",
                params![connection_id],
            )
            .await
            .context("query TMO overview imports")?;
        collect(rows, overview_from_row, "TMO overview import").await
    }

    pub async fn list_tmo_loans(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Vec<TmoImportLoan>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, stream_id, loan_account, borrower_name, \
                        property_address, property_city, property_state, property_zip, \
                        property_description, property_type, property_priority, occupancy, \
                        appraised_value, ltv, percent_owned, priority, loan_type, interest_rate, \
                        note_rate, original_balance, loan_balance, principal_balance, \
                        regular_payment, payment_frequency, maturity_date, next_payment_date, \
                        interest_paid_to, billed_through, term_left_months, is_delinquent, \
                        is_active, raw_summary_payload, raw_detail_payload, summary_imported_at, \
                        detail_imported_at, created_at, updated_at \
                 FROM intg_tmo_import_loan WHERE connection_id = ?1 \
                 ORDER BY loan_account COLLATE NOCASE, id",
                params![connection_id],
            )
            .await
            .context("query TMO loan imports")?;
        collect(rows, loan_from_row, "TMO loan import").await
    }

    pub async fn list_active_tmo_loan_views(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Vec<TmoLoanListItem>> {
        let rows = self
            .connection
            .query(
                "SELECT loan.loan_account, loan.borrower_name, loan.property_address, \
                        loan.property_city, loan.property_state, \
                        (SELECT photo.image_url \
                         FROM intg_loan_workspace_photo photo \
                         WHERE photo.connection_id = loan.connection_id \
                           AND photo.loan_account = loan.loan_account \
                         ORDER BY photo.is_featured DESC, photo.sort_order, photo.id \
                         LIMIT 1) AS featured_image_url, \
                        loan.property_type, loan.percent_owned, loan.note_rate, \
                        loan.principal_balance, loan.regular_payment, loan.maturity_date, \
                        loan.next_payment_date, loan.interest_paid_to, loan.is_delinquent \
                 FROM intg_tmo_import_loan loan \
                 WHERE loan.connection_id = ?1 AND loan.is_active = 1 \
                 ORDER BY loan.loan_account COLLATE NOCASE, loan.id",
                params![connection_id],
            )
            .await
            .context("query active TMO loan views")?;
        collect(rows, loan_view_from_row, "active TMO loan view").await
    }

    pub async fn tmo_loan_by_account(
        &self,
        connection_id: i64,
        loan_account: &str,
    ) -> IntegrationResult<Option<TmoImportLoan>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, stream_id, loan_account, borrower_name, \
                        property_address, property_city, property_state, property_zip, \
                        property_description, property_type, property_priority, occupancy, \
                        appraised_value, ltv, percent_owned, priority, loan_type, interest_rate, \
                        note_rate, original_balance, loan_balance, principal_balance, \
                        regular_payment, payment_frequency, maturity_date, next_payment_date, \
                        interest_paid_to, billed_through, term_left_months, is_delinquent, \
                        is_active, raw_summary_payload, raw_detail_payload, summary_imported_at, \
                        detail_imported_at, created_at, updated_at \
                 FROM intg_tmo_import_loan \
                 WHERE connection_id = ?1 AND loan_account = ?2 LIMIT 1",
                params![connection_id, loan_account],
            )
            .await
            .context("query TMO loan by account")?;
        one(rows, loan_from_row, "TMO loan import").await
    }

    pub async fn list_tmo_payments(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Vec<TmoImportPayment>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, external_id, loan_account, borrower_name, \
                        property_name, check_number, check_date, amount, service_fee, interest, \
                        principal, charges, late_charges, other, processing_state, \
                        normalized_event_source_id, raw_payload, imported_at, updated_at \
                 FROM intg_tmo_import_payment WHERE connection_id = ?1 \
                 ORDER BY check_date DESC, id DESC",
                params![connection_id],
            )
            .await
            .context("query TMO payment imports")?;
        collect(rows, payment_from_row, "TMO payment import").await
    }

    pub async fn list_recent_tmo_payments(
        &self,
        connection_id: i64,
        limit: u16,
    ) -> IntegrationResult<Vec<TmoImportPayment>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, external_id, loan_account, borrower_name, \
                        property_name, check_number, check_date, amount, service_fee, interest, \
                        principal, charges, late_charges, other, processing_state, \
                        normalized_event_source_id, raw_payload, imported_at, updated_at \
                 FROM intg_tmo_import_payment WHERE connection_id = ?1 \
                 ORDER BY check_date DESC, id DESC LIMIT ?2",
                params![connection_id, i64::from(limit)],
            )
            .await
            .context("query recent TMO payment imports")?;
        collect(rows, payment_from_row, "TMO payment import").await
    }

    pub async fn list_tmo_payments_for_loan(
        &self,
        connection_id: i64,
        loan_account: &str,
        limit: u16,
    ) -> IntegrationResult<Vec<TmoImportPayment>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, external_id, loan_account, borrower_name, \
                        property_name, check_number, check_date, amount, service_fee, interest, \
                        principal, charges, late_charges, other, processing_state, \
                        normalized_event_source_id, raw_payload, imported_at, updated_at \
                 FROM intg_tmo_import_payment \
                 WHERE connection_id = ?1 AND loan_account = ?2 \
                 ORDER BY check_date DESC, id DESC LIMIT ?3",
                params![connection_id, loan_account, i64::from(limit)],
            )
            .await
            .context("query TMO payment imports for loan")?;
        collect(rows, payment_from_row, "TMO payment import").await
    }

    pub async fn list_normalized_tmo_payments(
        &self,
        connection_id: i64,
        limit: u16,
    ) -> IntegrationResult<Vec<NormalizedTmoPayment>> {
        let rows = self
            .connection
            .query(
                "SELECT event.id, event.label, event.expected_date, event.actual_date, \
                        COALESCE(event.actual_amount, event.override_amount, event.amount), \
                        event.status, payment.check_number, payment.loan_account \
                 FROM intg_tmo_payment_event_link link \
                 JOIN intg_tmo_import_payment payment ON payment.id = link.tmo_payment_id \
                 JOIN stream_event event ON event.id = link.stream_event_id \
                 WHERE payment.connection_id = ?1 \
                 ORDER BY COALESCE(event.actual_date, event.override_date, event.expected_date) \
                          DESC, event.id DESC \
                 LIMIT ?2",
                params![connection_id, i64::from(limit)],
            )
            .await
            .context("query normalized TMO payments")?;
        collect(rows, normalized_payment_from_row, "normalized TMO payment").await
    }

    pub async fn list_captured_provider_records(
        &self,
        connection_id: i64,
        limit: u16,
    ) -> IntegrationResult<Vec<CapturedProviderRecord>> {
        let rows = self
            .connection
            .query(
                "SELECT entity_type, external_id, effective_date, summary, amount, \
                        raw_payload, updated_at \
                 FROM ( \
                     SELECT 'tmo_overview' AS entity_type, snapshot_date AS external_id, \
                            snapshot_date AS effective_date, \
                            'Portfolio overview snapshot' AS summary, trust_balance AS amount, \
                            COALESCE(raw_payload, '{}') AS raw_payload, updated_at \
                     FROM intg_tmo_import_overview WHERE connection_id = ?1 \
                     UNION ALL \
                     SELECT 'tmo_loan' AS entity_type, loan_account AS external_id, \
                            next_payment_date AS effective_date, \
                            CASE \
                                WHEN borrower_name IS NULL THEN property_address \
                                WHEN property_address IS NULL THEN borrower_name \
                                ELSE borrower_name || ' - ' || property_address \
                            END AS summary, \
                            regular_payment AS amount, \
                            COALESCE(raw_detail_payload, raw_summary_payload, '{}') AS raw_payload, \
                            updated_at \
                     FROM intg_tmo_import_loan WHERE connection_id = ?1 \
                 ) captured \
                 ORDER BY effective_date IS NULL, effective_date DESC, updated_at DESC \
                 LIMIT ?2",
                params![connection_id, i64::from(limit)],
            )
            .await
            .context("query captured provider records")?;
        collect(rows, captured_record_from_row, "captured provider record").await
    }

    pub async fn tmo_account(&self) -> IntegrationResult<Option<TmoAccount>> {
        let rows = self
            .connection
            .query(
                "SELECT id, company_id, account_number, source_rec_id, display_name, email, \
                        last_login_at, created_at, updated_at \
                 FROM intg_tmo_account WHERE id = 1",
                (),
            )
            .await
            .context("query TMO account")?;
        one(rows, tmo_account_from_row, "TMO account").await
    }

    pub async fn tmo_credential(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Option<TmoCredentialRecord>> {
        let rows = self
            .connection
            .query(
                "SELECT connection_id, company_id, account_number, pin_ciphertext, pin_nonce, \
                        key_version, created_at, updated_at \
                 FROM intg_tmo_credential WHERE connection_id = ?1",
                params![connection_id],
            )
            .await
            .context("query encrypted TMO credential")?;
        one(rows, tmo_credential_from_row, "encrypted TMO credential").await
    }

    pub async fn monarch_credential(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Option<MonarchCredentialRecord>> {
        let rows = self
            .connection
            .query(
                "SELECT connection_id, access_token_ciphertext, access_token_nonce, \
                        default_account_id, key_version, created_at, updated_at \
                 FROM intg_monarch_credential WHERE connection_id = ?1",
                params![connection_id],
            )
            .await
            .context("query encrypted Monarch credential")?;
        one(
            rows,
            monarch_credential_from_row,
            "encrypted Monarch credential",
        )
        .await
    }

    pub async fn list_tmo_payment_event_links(
        &self,
        connection_id: i64,
    ) -> IntegrationResult<Vec<TmoPaymentEventLink>> {
        let rows = self
            .connection
            .query(
                "SELECT link.tmo_payment_id, link.stream_event_id, link.created_at \
                 FROM intg_tmo_payment_event_link link \
                 JOIN intg_tmo_import_payment payment ON payment.id = link.tmo_payment_id \
                 WHERE payment.connection_id = ?1 \
                 ORDER BY link.tmo_payment_id",
                params![connection_id],
            )
            .await
            .context("query TMO payment event links")?;
        collect(rows, payment_link_from_row, "TMO payment event link").await
    }

    pub async fn list_portfolio_snapshots(&self) -> IntegrationResult<Vec<PortfolioSnapshot>> {
        let rows = self
            .connection
            .query(
                "SELECT id, snapshot_date, portfolio_value, portfolio_yield, portfolio_count, \
                        ytd_interest, ytd_principal, trust_balance, outstanding_checks, \
                        service_fees, synced_at \
                 FROM portfolio_snapshot ORDER BY snapshot_date DESC, id DESC",
                (),
            )
            .await
            .context("query portfolio snapshots")?;
        collect(rows, portfolio_snapshot_from_row, "portfolio snapshot").await
    }

    pub async fn setting(&self, key: &str) -> IntegrationResult<Option<Setting>> {
        let rows = self
            .connection
            .query(
                "SELECT key, value, updated_at FROM settings WHERE key = ?1",
                params![key],
            )
            .await
            .context("query setting")?;
        one(rows, setting_from_row, "setting").await
    }

    pub async fn list_settings(&self) -> IntegrationResult<Vec<Setting>> {
        let rows = self
            .connection
            .query(
                "SELECT key, value, updated_at FROM settings ORDER BY key",
                (),
            )
            .await
            .context("query settings")?;
        collect(rows, setting_from_row, "setting").await
    }
}

async fn collect<T>(
    mut rows: Rows,
    decode: fn(&Row) -> anyhow::Result<T>,
    label: &'static str,
) -> IntegrationResult<Vec<T>> {
    let mut values = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .with_context(|| format!("read {label} row"))?
    {
        values.push(decode(&row)?);
    }
    Ok(values)
}

async fn one<T>(
    mut rows: Rows,
    decode: fn(&Row) -> anyhow::Result<T>,
    label: &'static str,
) -> IntegrationResult<Option<T>> {
    let Some(row) = rows
        .next()
        .await
        .with_context(|| format!("read {label} row"))?
    else {
        return Ok(None);
    };
    Ok(Some(decode(&row)?))
}

fn connection_from_row(row: &Row) -> anyhow::Result<IntegrationConnection> {
    Ok(IntegrationConnection {
        id: row.get(0).context("decode integration connection id")?,
        slug: row.get(1).context("decode integration connection slug")?,
        name: row.get(2).context("decode integration connection name")?,
        provider: row
            .get(3)
            .context("decode integration connection provider")?,
        status: row.get(4).context("decode integration connection status")?,
        sync_cadence: row
            .get(5)
            .context("decode integration connection cadence")?,
        last_synced_at: row
            .get(6)
            .context("decode integration connection last sync")?,
        last_error: row
            .get(7)
            .context("decode integration connection last error")?,
        metadata: row
            .get(8)
            .context("decode integration connection metadata")?,
        next_scheduled_at: row
            .get(9)
            .context("decode integration connection next schedule")?,
        created_at: row
            .get(10)
            .context("decode integration connection creation time")?,
        updated_at: row
            .get(11)
            .context("decode integration connection update time")?,
    })
}

fn connection_view_from_row(row: &Row) -> anyhow::Result<IntegrationConnectionView> {
    Ok(IntegrationConnectionView {
        id: row.get(0).context("decode integration connection id")?,
        slug: row.get(1).context("decode integration connection slug")?,
        name: row.get(2).context("decode integration connection name")?,
        provider: row
            .get(3)
            .context("decode integration connection provider")?,
        status: row.get(4).context("decode integration connection status")?,
        sync_cadence: row
            .get(5)
            .context("decode integration connection cadence")?,
        last_synced_at: row
            .get(6)
            .context("decode integration connection last sync")?,
        last_error: row
            .get(7)
            .context("decode integration connection last error")?,
        metadata: row
            .get(8)
            .context("decode integration connection metadata")?,
        next_scheduled_at: row
            .get(9)
            .context("decode integration connection next schedule")?,
        created_at: row
            .get(10)
            .context("decode integration connection creation time")?,
        updated_at: row
            .get(11)
            .context("decode integration connection update time")?,
        record_count: row
            .get(12)
            .context("decode integration connection record count")?,
        normalized_count: row
            .get(13)
            .context("decode integration connection normalized count")?,
        pending_count: row
            .get(14)
            .context("decode integration connection pending count")?,
    })
}

fn overview_from_row(row: &Row) -> anyhow::Result<TmoImportOverview> {
    Ok(TmoImportOverview {
        id: row.get(0).context("decode overview id")?,
        connection_id: row.get(1).context("decode overview connection id")?,
        snapshot_date: row.get(2).context("decode overview date")?,
        portfolio_value: row.get(3).context("decode portfolio value")?,
        portfolio_yield: row.get(4).context("decode portfolio yield")?,
        portfolio_count: row.get(5).context("decode portfolio count")?,
        ytd_interest: row.get(6).context("decode YTD interest")?,
        ytd_principal: row.get(7).context("decode YTD principal")?,
        trust_balance: row.get(8).context("decode trust balance")?,
        outstanding_checks: row.get(9).context("decode outstanding checks")?,
        service_fees: row.get(10).context("decode service fees")?,
        processing_state: row.get(11).context("decode overview processing state")?,
        raw_payload: row.get(12).context("decode overview raw payload")?,
        created_at: row.get(13).context("decode overview creation time")?,
        updated_at: row.get(14).context("decode overview update time")?,
    })
}

fn loan_from_row(row: &Row) -> anyhow::Result<TmoImportLoan> {
    Ok(TmoImportLoan {
        id: row.get(0).context("decode loan id")?,
        connection_id: row.get(1).context("decode loan connection id")?,
        stream_id: row.get(2).context("decode loan stream id")?,
        loan_account: row.get(3).context("decode loan account")?,
        borrower_name: row.get(4).context("decode borrower name")?,
        property_address: row.get(5).context("decode property address")?,
        property_city: row.get(6).context("decode property city")?,
        property_state: row.get(7).context("decode property state")?,
        property_zip: row.get(8).context("decode property ZIP")?,
        property_description: row.get(9).context("decode property description")?,
        property_type: row.get(10).context("decode property type")?,
        property_priority: row.get(11).context("decode property priority")?,
        occupancy: row.get(12).context("decode occupancy")?,
        appraised_value: row.get(13).context("decode appraised value")?,
        ltv: row.get(14).context("decode LTV")?,
        percent_owned: row.get(15).context("decode percent owned")?,
        priority: row.get(16).context("decode loan priority")?,
        loan_type: row.get(17).context("decode loan type")?,
        interest_rate: row.get(18).context("decode interest rate")?,
        note_rate: row.get(19).context("decode note rate")?,
        original_balance: row.get(20).context("decode original balance")?,
        loan_balance: row.get(21).context("decode loan balance")?,
        principal_balance: row.get(22).context("decode principal balance")?,
        regular_payment: row.get(23).context("decode regular payment")?,
        payment_frequency: row.get(24).context("decode payment frequency")?,
        maturity_date: row.get(25).context("decode maturity date")?,
        next_payment_date: row.get(26).context("decode next payment date")?,
        interest_paid_to: row.get(27).context("decode interest-paid-to date")?,
        billed_through: row.get(28).context("decode billed-through date")?,
        term_left_months: row.get(29).context("decode term left")?,
        is_delinquent: row.get(30).context("decode delinquency flag")?,
        is_active: row.get(31).context("decode active flag")?,
        raw_summary_payload: row.get(32).context("decode raw summary payload")?,
        raw_detail_payload: row.get(33).context("decode raw detail payload")?,
        summary_imported_at: row.get(34).context("decode summary import time")?,
        detail_imported_at: row.get(35).context("decode detail import time")?,
        created_at: row.get(36).context("decode loan creation time")?,
        updated_at: row.get(37).context("decode loan update time")?,
    })
}

fn loan_view_from_row(row: &Row) -> anyhow::Result<TmoLoanListItem> {
    Ok(TmoLoanListItem {
        loan_account: row.get(0).context("decode loan account")?,
        borrower_name: row.get(1).context("decode borrower name")?,
        property_address: row.get(2).context("decode property address")?,
        property_city: row.get(3).context("decode property city")?,
        property_state: row.get(4).context("decode property state")?,
        featured_image_url: row.get(5).context("decode featured image URL")?,
        property_type: row.get(6).context("decode property type")?,
        percent_owned: row.get(7).context("decode percent owned")?,
        note_rate: row.get(8).context("decode note rate")?,
        principal_balance: row.get(9).context("decode principal balance")?,
        regular_payment: row.get(10).context("decode regular payment")?,
        maturity_date: row.get(11).context("decode maturity date")?,
        next_payment_date: row.get(12).context("decode next payment date")?,
        interest_paid_to: row.get(13).context("decode interest-paid-to date")?,
        is_delinquent: row.get(14).context("decode delinquency flag")?,
    })
}

fn payment_from_row(row: &Row) -> anyhow::Result<TmoImportPayment> {
    Ok(TmoImportPayment {
        id: row.get(0).context("decode payment id")?,
        connection_id: row.get(1).context("decode payment connection id")?,
        external_id: row.get(2).context("decode payment external id")?,
        loan_account: row.get(3).context("decode payment loan account")?,
        borrower_name: row.get(4).context("decode payment borrower")?,
        property_name: row.get(5).context("decode payment property")?,
        check_number: row.get(6).context("decode check number")?,
        check_date: row.get(7).context("decode check date")?,
        amount: row.get(8).context("decode payment amount")?,
        service_fee: row.get(9).context("decode payment service fee")?,
        interest: row.get(10).context("decode payment interest")?,
        principal: row.get(11).context("decode payment principal")?,
        charges: row.get(12).context("decode payment charges")?,
        late_charges: row.get(13).context("decode payment late charges")?,
        other: row.get(14).context("decode payment other amount")?,
        processing_state: row.get(15).context("decode payment processing state")?,
        normalized_event_source_id: row.get(16).context("decode normalized event source id")?,
        raw_payload: row.get(17).context("decode payment raw payload")?,
        imported_at: row.get(18).context("decode payment import time")?,
        updated_at: row.get(19).context("decode payment update time")?,
    })
}

fn normalized_payment_from_row(row: &Row) -> anyhow::Result<NormalizedTmoPayment> {
    Ok(NormalizedTmoPayment {
        id: row.get(0).context("decode normalized payment event id")?,
        label: row.get(1).context("decode normalized payment label")?,
        expected_date: row
            .get(2)
            .context("decode normalized payment expected date")?,
        actual_date: row
            .get(3)
            .context("decode normalized payment actual date")?,
        amount: row.get(4).context("decode normalized payment amount")?,
        status: row.get(5).context("decode normalized payment status")?,
        check_number: row
            .get(6)
            .context("decode normalized payment check number")?,
        loan_account: row
            .get(7)
            .context("decode normalized payment loan account")?,
    })
}

fn captured_record_from_row(row: &Row) -> anyhow::Result<CapturedProviderRecord> {
    Ok(CapturedProviderRecord {
        entity_type: row.get(0).context("decode captured record entity type")?,
        external_id: row.get(1).context("decode captured record external id")?,
        effective_date: row
            .get(2)
            .context("decode captured record effective date")?,
        summary: row.get(3).context("decode captured record summary")?,
        amount: row.get(4).context("decode captured record amount")?,
        raw_payload: row.get(5).context("decode captured record payload")?,
        updated_at: row.get(6).context("decode captured record update time")?,
    })
}

fn tmo_account_from_row(row: &Row) -> anyhow::Result<TmoAccount> {
    Ok(TmoAccount {
        id: row.get(0).context("decode TMO account id")?,
        company_id: row.get(1).context("decode TMO company id")?,
        account_number: row.get(2).context("decode TMO account number")?,
        source_rec_id: row.get(3).context("decode TMO source record id")?,
        display_name: row.get(4).context("decode TMO display name")?,
        email: row.get(5).context("decode TMO email")?,
        last_login_at: row.get(6).context("decode TMO last login")?,
        created_at: row.get(7).context("decode TMO account creation time")?,
        updated_at: row.get(8).context("decode TMO account update time")?,
    })
}

fn tmo_credential_from_row(row: &Row) -> anyhow::Result<TmoCredentialRecord> {
    Ok(TmoCredentialRecord {
        connection_id: row.get(0).context("decode TMO credential connection id")?,
        company_id: row.get(1).context("decode TMO credential company id")?,
        account_number: row.get(2).context("decode TMO credential account")?,
        pin_ciphertext: row.get(3).context("decode TMO credential ciphertext")?,
        pin_nonce: row.get(4).context("decode TMO credential nonce")?,
        key_version: row.get(5).context("decode TMO credential key version")?,
        created_at: row.get(6).context("decode TMO credential creation time")?,
        updated_at: row.get(7).context("decode TMO credential update time")?,
    })
}

fn monarch_credential_from_row(row: &Row) -> anyhow::Result<MonarchCredentialRecord> {
    Ok(MonarchCredentialRecord {
        connection_id: row
            .get(0)
            .context("decode Monarch credential connection id")?,
        access_token_ciphertext: row.get(1).context("decode Monarch credential ciphertext")?,
        access_token_nonce: row.get(2).context("decode Monarch credential nonce")?,
        default_account_id: row
            .get(3)
            .context("decode Monarch credential default account")?,
        key_version: row
            .get(4)
            .context("decode Monarch credential key version")?,
        created_at: row
            .get(5)
            .context("decode Monarch credential creation time")?,
        updated_at: row
            .get(6)
            .context("decode Monarch credential update time")?,
    })
}

fn payment_link_from_row(row: &Row) -> anyhow::Result<TmoPaymentEventLink> {
    Ok(TmoPaymentEventLink {
        tmo_payment_id: row.get(0).context("decode linked TMO payment id")?,
        stream_event_id: row.get(1).context("decode linked stream event id")?,
        created_at: row.get(2).context("decode payment link creation time")?,
    })
}

fn portfolio_snapshot_from_row(row: &Row) -> anyhow::Result<PortfolioSnapshot> {
    Ok(PortfolioSnapshot {
        id: row.get(0).context("decode portfolio snapshot id")?,
        snapshot_date: row.get(1).context("decode portfolio snapshot date")?,
        portfolio_value: row.get(2).context("decode snapshot portfolio value")?,
        portfolio_yield: row.get(3).context("decode snapshot portfolio yield")?,
        portfolio_count: row.get(4).context("decode snapshot portfolio count")?,
        ytd_interest: row.get(5).context("decode snapshot YTD interest")?,
        ytd_principal: row.get(6).context("decode snapshot YTD principal")?,
        trust_balance: row.get(7).context("decode snapshot trust balance")?,
        outstanding_checks: row.get(8).context("decode snapshot outstanding checks")?,
        service_fees: row.get(9).context("decode snapshot service fees")?,
        synced_at: row.get(10).context("decode portfolio snapshot sync time")?,
    })
}

fn setting_from_row(row: &Row) -> anyhow::Result<Setting> {
    Ok(Setting {
        key: row.get(0).context("decode setting key")?,
        value: row.get(1).context("decode setting value")?,
        updated_at: row.get(2).context("decode setting update time")?,
    })
}
