use libsql::{Builder, Connection, params};

use super::IntegrationRepository;
use crate::db::AppContext;

async fn test_connection() -> Connection {
    let database = Builder::new_local(":memory:").build().await.unwrap();
    AppContext::from_database(database)
        .await
        .unwrap()
        .connection()
        .await
        .unwrap()
}

#[tokio::test]
async fn typed_reads_preserve_provider_rows_and_source_identity() {
    let connection = test_connection().await;
    connection
        .execute_batch(
            "INSERT INTO intg_integration_connection ( \
                 id, slug, name, provider, status, sync_cadence, last_synced_at, last_error, \
                 metadata, next_scheduled_at, created_at, updated_at \
             ) VALUES ( \
                 41, 'tmo', 'The Mortgage Office', 'mortgage_office', 'active', 'every_6h', \
                 '2026-07-14T18:00:00.000Z', NULL, '{\"company\":\"vci\"}', \
                 '2026-07-15T00:00:00.000Z', '2026-01-01T00:00:00.000Z', \
                 '2026-07-14T18:00:00.000Z' \
             ); \
             INSERT INTO intg_tmo_import_overview ( \
                 id, connection_id, snapshot_date, portfolio_value, portfolio_yield, \
                 portfolio_count, ytd_interest, ytd_principal, trust_balance, \
                 outstanding_checks, service_fees, processing_state, raw_payload, \
                 created_at, updated_at \
             ) VALUES ( \
                 51, 41, '2026-07-14', 1234.5, 7.25, 2, 31.0, 19.0, 450.0, 12.0, \
                 3.5, 'captured', '{\"provider\":\"overview\"}', \
                 '2026-07-14T18:00:00.000Z', '2026-07-14T18:00:00.000Z' \
             ); \
             INSERT INTO intg_tmo_import_loan ( \
                 id, connection_id, loan_account, borrower_name, loan_balance, \
                 principal_balance, regular_payment, payment_frequency, maturity_date, \
                 next_payment_date, interest_paid_to, billed_through, is_delinquent, is_active, \
                 raw_summary_payload, raw_detail_payload, created_at, updated_at \
             ) VALUES ( \
                 61, 41, 'LN-009', 'Borrower', 900.25, 899.75, 42.50, 'Monthly', \
                 '2030-01-01', '2026-08-01', '2026-07-01', '2026-07-01', 0, 1, \
                 '{\"source\":\"summary\"}', '{\"source\":\"detail\"}', \
                 '2026-07-14T18:00:00.000Z', '2026-07-14T18:00:00.000Z' \
             ); \
             INSERT INTO intg_tmo_import_payment ( \
                 id, connection_id, external_id, loan_account, borrower_name, property_name, \
                 check_number, check_date, amount, service_fee, interest, principal, charges, \
                 late_charges, other, processing_state, normalized_event_source_id, raw_payload, \
                 imported_at, updated_at \
             ) VALUES ( \
                 71, 41, 'history:LN-009:2026-07-01:4250', 'LN-009', 'Borrower', 'Property', \
                 NULL, '2026-07-01', 42.5, 1.5, 20.0, 20.0, 1.0, 0.0, 0.0, 'normalized', \
                 'tmo:payment:71', '{\"source\":\"payment\"}', \
                 '2026-07-14T18:00:00.000Z', '2026-07-14T18:00:00.000Z' \
             ); \
             INSERT INTO intg_tmo_account ( \
                 id, company_id, account_number, source_rec_id, display_name, email, \
                 last_login_at, created_at, updated_at \
             ) VALUES ( \
                 1, 'vci', '3589', 'rec-1', 'Admin', 'admin@example.com', \
                 '2026-07-14T18:00:00.000Z', '2026-01-01T00:00:00.000Z', \
                 '2026-07-14T18:00:00.000Z' \
             ); \
             INSERT INTO portfolio_snapshot ( \
                 id, snapshot_date, portfolio_value, portfolio_yield, portfolio_count, \
                 ytd_interest, ytd_principal, trust_balance, outstanding_checks, service_fees, \
                 synced_at \
             ) VALUES ( \
                 81, '2026-07-14', 1234.5, 7.25, 2, 31.0, 19.0, 450.0, 12.0, 3.5, \
                 '2026-07-14T18:00:00.000Z' \
             ); \
             INSERT INTO settings (key, value, updated_at) \
             VALUES ('balance_source', 'tmo', '2026-07-14T18:00:00.000Z'); \
             INSERT INTO stream ( \
                 id, name, type, kind, direction, amount_certainty, is_active \
             ) VALUES (91, 'Imported TMO', 'mortgage_portfolio', 'mortgage_portfolio', \
                       'in', 'known', 1); \
             INSERT INTO stream_event ( \
                 id, stream_id, expected_date, amount, actual_date, actual_amount, status, \
                 source_id, source_type \
             ) VALUES (101, 91, '2026-07-01', 42.5, '2026-07-01', 42.5, 'received', \
                       'tmo:payment:71', 'tmo_history'); \
             INSERT INTO intg_tmo_payment_event_link ( \
                 tmo_payment_id, stream_event_id, created_at \
             ) VALUES (71, 101, '2026-07-14T18:00:00.000Z');",
        )
        .await
        .unwrap();

    let repository = IntegrationRepository::new(&connection);
    let connections = repository.list_connections().await.unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].id, 41);
    assert_eq!(
        connections[0].metadata.as_deref(),
        Some("{\"company\":\"vci\"}")
    );
    assert_eq!(
        repository.connection_by_slug("tmo").await.unwrap(),
        Some(connections[0].clone())
    );
    let connection_views = repository.list_connection_views().await.unwrap();
    assert_eq!(connection_views[0].record_count, 3);
    assert_eq!(connection_views[0].normalized_count, 1);
    assert_eq!(connection_views[0].pending_count, 0);
    assert_eq!(
        repository
            .connection_view_by_slug("tmo")
            .await
            .unwrap()
            .unwrap()
            .id,
        41
    );

    let overview = repository.list_tmo_overviews(41).await.unwrap().remove(0);
    assert_eq!(overview.id, 51);
    assert_eq!(
        overview.raw_payload.as_deref(),
        Some("{\"provider\":\"overview\"}")
    );

    let loan = repository.list_tmo_loans(41).await.unwrap().remove(0);
    assert_eq!(loan.id, 61);
    assert_eq!(loan.loan_balance, Some(900.25));
    assert_eq!(
        loan.raw_detail_payload.as_deref(),
        Some("{\"source\":\"detail\"}")
    );
    assert_eq!(
        repository
            .list_active_tmo_loan_views(41)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        repository
            .tmo_loan_by_account(41, "LN-009")
            .await
            .unwrap()
            .unwrap()
            .id,
        61
    );
    assert!(
        repository
            .tmo_loan_by_account(999, "LN-009")
            .await
            .unwrap()
            .is_none()
    );

    let payment = repository.list_tmo_payments(41).await.unwrap().remove(0);
    assert_eq!(payment.id, 71);
    assert_eq!(payment.external_id, "history:LN-009:2026-07-01:4250");
    assert_eq!(
        payment.normalized_event_source_id.as_deref(),
        Some("tmo:payment:71")
    );
    assert_eq!(
        repository
            .list_recent_tmo_payments(41, 1)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        repository
            .list_tmo_payments_for_loan(41, "LN-009", 1)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        repository
            .list_normalized_tmo_payments(41, 1)
            .await
            .unwrap()[0]
            .id,
        101
    );
    assert_eq!(
        repository
            .list_captured_provider_records(41, 10)
            .await
            .unwrap()
            .len(),
        2
    );

    assert_eq!(repository.tmo_account().await.unwrap().unwrap().id, 1);
    assert_eq!(
        repository.list_tmo_payment_event_links(41).await.unwrap()[0].stream_event_id,
        101
    );
    assert_eq!(
        repository.list_portfolio_snapshots().await.unwrap()[0].id,
        81
    );
    assert_eq!(
        repository
            .setting("balance_source")
            .await
            .unwrap()
            .unwrap()
            .value,
        "tmo"
    );
    assert_eq!(repository.list_settings().await.unwrap().len(), 1);
}

#[tokio::test]
async fn encrypted_credentials_round_trip_byte_for_byte() {
    let connection = test_connection().await;
    connection
        .execute(
            "INSERT INTO intg_integration_connection (id, slug, name, provider) \
             VALUES (7, 'tmo', 'TMO', 'mortgage_office')",
            (),
        )
        .await
        .unwrap();

    let pin_ciphertext = "cipher:Aa+/=☃\nopaque";
    let pin_nonce = "nonce:00/++==";
    let token_ciphertext = "token:🔒:+/=\nopaque";
    let token_nonce = "nonce:αβγ";
    connection
        .execute(
            "INSERT INTO intg_tmo_credential ( \
                 connection_id, company_id, account_number, pin_ciphertext, pin_nonce, key_version \
             ) VALUES (?1, 'vci', '3589', ?2, ?3, 4)",
            params![7_i64, pin_ciphertext, pin_nonce],
        )
        .await
        .unwrap();
    connection
        .execute(
            "INSERT INTO intg_monarch_credential ( \
                 connection_id, access_token_ciphertext, access_token_nonce, \
                 default_account_id, key_version \
             ) VALUES (?1, ?2, ?3, 'account-99', 5)",
            params![7_i64, token_ciphertext, token_nonce],
        )
        .await
        .unwrap();

    let repository = IntegrationRepository::new(&connection);
    let tmo = repository.tmo_credential(7).await.unwrap().unwrap();
    let monarch = repository.monarch_credential(7).await.unwrap().unwrap();
    assert_eq!(tmo.pin_ciphertext.as_bytes(), pin_ciphertext.as_bytes());
    assert_eq!(tmo.pin_nonce.as_bytes(), pin_nonce.as_bytes());
    assert_eq!(
        monarch.access_token_ciphertext.as_bytes(),
        token_ciphertext.as_bytes()
    );
    assert_eq!(
        monarch.access_token_nonce.as_bytes(),
        token_nonce.as_bytes()
    );
}

#[tokio::test]
async fn integration_foreign_keys_and_checks_fail_closed() {
    let connection = test_connection().await;
    assert!(
        connection
            .execute(
                "INSERT INTO intg_tmo_import_overview (connection_id, snapshot_date) \
                 VALUES (999, '2026-07-14')",
                (),
            )
            .await
            .is_err()
    );
    connection
        .execute(
            "INSERT INTO intg_integration_connection (id, slug, name, provider) \
             VALUES (1, 'tmo', 'TMO', 'mortgage_office')",
            (),
        )
        .await
        .unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO intg_tmo_import_overview ( \
                     connection_id, snapshot_date, portfolio_value \
                 ) VALUES (1, '2026-07-14', ?1)",
                params![f64::INFINITY],
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_tmo_import_overview ( \
                     connection_id, snapshot_date, raw_payload \
                 ) VALUES (1, '2026-07-14', 'not-json')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_tmo_import_payment ( \
                     connection_id, external_id, loan_account, borrower_name, property_name, \
                     check_date, amount, service_fee, interest, principal, charges, late_charges, \
                     other \
                 ) VALUES ( \
                     1, 'nan-payment', 'LN-1', 'Borrower', 'Property', '2026-07-14', \
                     ?1, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0 \
                 )",
                params![f64::NAN],
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_integration_connection ( \
                     id, slug, name, provider, created_at \
                 ) VALUES (2, 'bad-time', 'Bad time', 'test', 'not-a-timestamp')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO intg_tmo_account (id, company_id, account_number) \
                 VALUES (2, 'vci', '3589')",
                (),
            )
            .await
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO portfolio_snapshot (snapshot_date) VALUES ('07/14/2026')",
                (),
            )
            .await
            .is_err()
    );

    let mut foreign_keys = connection
        .query("PRAGMA foreign_key_check", ())
        .await
        .unwrap();
    assert!(foreign_keys.next().await.unwrap().is_none());
    let mut integrity = connection
        .query("PRAGMA integrity_check", ())
        .await
        .unwrap();
    assert_eq!(
        integrity
            .next()
            .await
            .unwrap()
            .unwrap()
            .get::<String>(0)
            .unwrap(),
        "ok"
    );
}
