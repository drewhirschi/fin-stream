use std::collections::BTreeMap;

use blake3::Hasher;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    manifest::{FinancialStats, TableStats},
    model::Dataset,
};

pub fn dataset_stats(dataset: &Dataset) -> BTreeMap<&'static str, TableStats> {
    BTreeMap::from([
        (
            "app_user",
            stats(
                dataset
                    .app_users
                    .iter()
                    .map(|row| {
                        (
                            row.id.to_string(),
                            json!([
                                row.id,
                                row.email,
                                row.password_hash,
                                row.display_name,
                                row.is_active,
                                row.created_at,
                                row.updated_at
                            ]),
                        )
                    })
                    .collect(),
                Vec::new(),
            ),
        ),
        (
            "account",
            stats(
                dataset
                    .accounts
                    .iter()
                    .map(|row| {
                        (
                            row.id.to_string(),
                            json!([
                                row.id,
                                row.name,
                                row.kind,
                                float_json(row.balance),
                                row.balance_as_of_date,
                                row.source_type,
                                row.source_ref,
                                row.metadata,
                                row.balance_updated_at,
                                row.is_primary,
                                row.is_active,
                                row.notes,
                                row.created_at,
                                row.updated_at
                            ]),
                        )
                    })
                    .collect(),
                vec![financial(
                    "balance",
                    dataset.accounts.iter().filter_map(|row| row.balance),
                )],
            ),
        ),
        (
            "stream",
            stats(
                dataset
                    .streams
                    .iter()
                    .map(|row| {
                        (
                            row.id.to_string(),
                            json!([
                                row.id,
                                row.name,
                                row.stream_type,
                                row.kind,
                                row.direction,
                                row.amount_certainty,
                                row.description,
                                row.default_account_id,
                                row.configuration,
                                row.parent_id,
                                row.is_active,
                                row.created_at,
                                row.updated_at
                            ]),
                        )
                    })
                    .collect(),
                Vec::new(),
            ),
        ),
        (
            "stream_view",
            stats(
                dataset
                    .stream_views
                    .iter()
                    .map(|row| {
                        (
                            row.id.to_string(),
                            json!([
                                row.id,
                                row.name,
                                row.description,
                                row.is_default,
                                row.is_active,
                                row.created_at,
                                row.updated_at
                            ]),
                        )
                    })
                    .collect(),
                Vec::new(),
            ),
        ),
        (
            "stream_view_stream",
            stats(
                dataset
                    .stream_view_streams
                    .iter()
                    .map(|row| {
                        (
                            format!("{}:{}", row.stream_view_id, row.stream_id),
                            json!([row.stream_view_id, row.stream_id, row.created_at]),
                        )
                    })
                    .collect(),
                Vec::new(),
            ),
        ),
        (
            "stream_schedule",
            stats(
                dataset
                    .stream_schedules
                    .iter()
                    .map(|row| {
                        (
                            row.id.to_string(),
                            json!([
                                row.id,
                                row.stream_id,
                                row.account_id,
                                row.label,
                                float_json(Some(row.amount)),
                                row.frequency,
                                row.day_of_month,
                                row.start_date,
                                row.end_date,
                                row.is_active,
                                row.metadata,
                                row.created_at,
                                row.updated_at
                            ]),
                        )
                    })
                    .collect(),
                vec![financial(
                    "amount",
                    dataset.stream_schedules.iter().map(|row| row.amount),
                )],
            ),
        ),
        (
            "stream_event",
            stats(
                dataset
                    .stream_events
                    .iter()
                    .map(|row| {
                        (
                            row.id.to_string(),
                            json!([
                                row.id,
                                row.stream_id,
                                row.account_id,
                                row.label,
                                row.expected_date,
                                float_json(Some(row.amount)),
                                row.override_label,
                                row.has_label_override,
                                row.override_date,
                                float_json(row.override_amount),
                                row.override_account_id,
                                row.has_account_override,
                                row.actual_date,
                                float_json(row.actual_amount),
                                row.status,
                                row.is_excluded,
                                row.exclusion_reason,
                                row.source_id,
                                row.source_type,
                                row.metadata,
                                row.notes,
                                row.created_at,
                                row.updated_at
                            ]),
                        )
                    })
                    .collect(),
                vec![
                    financial("amount", dataset.stream_events.iter().map(|row| row.amount)),
                    financial(
                        "actual_amount",
                        dataset
                            .stream_events
                            .iter()
                            .filter_map(|row| row.actual_amount),
                    ),
                ],
            ),
        ),
        (
            "integration_connection",
            serialized_stats(
                &dataset.integration_connections,
                |row| row.id.to_string(),
                vec![],
            ),
        ),
        (
            "tmo_import_overview",
            serialized_stats(
                &dataset.tmo_import_overviews,
                |row| row.id.to_string(),
                vec![
                    financial(
                        "portfolio_value",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.portfolio_value),
                    ),
                    financial(
                        "portfolio_yield",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.portfolio_yield),
                    ),
                    financial(
                        "ytd_interest",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.ytd_interest),
                    ),
                    financial(
                        "ytd_principal",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.ytd_principal),
                    ),
                    financial(
                        "trust_balance",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.trust_balance),
                    ),
                    financial(
                        "outstanding_checks",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.outstanding_checks),
                    ),
                    financial(
                        "service_fees",
                        dataset
                            .tmo_import_overviews
                            .iter()
                            .filter_map(|row| row.service_fees),
                    ),
                ],
            ),
        ),
        (
            "tmo_import_loan",
            serialized_stats(
                &dataset.tmo_import_loans,
                |row| row.id.to_string(),
                vec![
                    financial(
                        "appraised_value",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.appraised_value),
                    ),
                    financial(
                        "ltv",
                        dataset.tmo_import_loans.iter().filter_map(|row| row.ltv),
                    ),
                    financial(
                        "percent_owned",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.percent_owned),
                    ),
                    financial(
                        "interest_rate",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.interest_rate),
                    ),
                    financial(
                        "note_rate",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.note_rate),
                    ),
                    financial(
                        "original_balance",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.original_balance),
                    ),
                    financial(
                        "loan_balance",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.loan_balance),
                    ),
                    financial(
                        "principal_balance",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.principal_balance),
                    ),
                    financial(
                        "regular_payment",
                        dataset
                            .tmo_import_loans
                            .iter()
                            .filter_map(|row| row.regular_payment),
                    ),
                ],
            ),
        ),
        (
            "tmo_import_payment",
            serialized_stats(
                &dataset.tmo_import_payments,
                |row| row.id.to_string(),
                vec![
                    financial(
                        "amount",
                        dataset.tmo_import_payments.iter().map(|row| row.amount),
                    ),
                    financial(
                        "service_fee",
                        dataset
                            .tmo_import_payments
                            .iter()
                            .map(|row| row.service_fee),
                    ),
                    financial(
                        "interest",
                        dataset.tmo_import_payments.iter().map(|row| row.interest),
                    ),
                    financial(
                        "principal",
                        dataset.tmo_import_payments.iter().map(|row| row.principal),
                    ),
                    financial(
                        "charges",
                        dataset.tmo_import_payments.iter().map(|row| row.charges),
                    ),
                    financial(
                        "late_charges",
                        dataset
                            .tmo_import_payments
                            .iter()
                            .map(|row| row.late_charges),
                    ),
                    financial(
                        "other",
                        dataset.tmo_import_payments.iter().map(|row| row.other),
                    ),
                ],
            ),
        ),
        (
            "tmo_account",
            serialized_stats(&dataset.tmo_accounts, |row| row.id.to_string(), vec![]),
        ),
        (
            "tmo_credential",
            serialized_stats(
                &dataset.tmo_credentials,
                |row| row.connection_id.to_string(),
                vec![],
            ),
        ),
        (
            "monarch_credential",
            serialized_stats(
                &dataset.monarch_credentials,
                |row| row.connection_id.to_string(),
                vec![],
            ),
        ),
        (
            "tmo_payment_event_link",
            serialized_stats(
                &dataset.tmo_payment_event_links,
                |row| row.tmo_payment_id.to_string(),
                vec![],
            ),
        ),
        (
            "portfolio_snapshot",
            serialized_stats(
                &dataset.portfolio_snapshots,
                |row| row.id.to_string(),
                vec![
                    financial(
                        "portfolio_value",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.portfolio_value),
                    ),
                    financial(
                        "portfolio_yield",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.portfolio_yield),
                    ),
                    financial(
                        "ytd_interest",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.ytd_interest),
                    ),
                    financial(
                        "ytd_principal",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.ytd_principal),
                    ),
                    financial(
                        "trust_balance",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.trust_balance),
                    ),
                    financial(
                        "outstanding_checks",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.outstanding_checks),
                    ),
                    financial(
                        "service_fees",
                        dataset
                            .portfolio_snapshots
                            .iter()
                            .filter_map(|row| row.service_fees),
                    ),
                ],
            ),
        ),
        (
            "settings",
            serialized_stats(&dataset.settings, |row| row.key.clone(), vec![]),
        ),
        (
            "sync_log",
            serialized_stats(&dataset.sync_logs, |row| row.id.to_string(), vec![]),
        ),
        (
            "loan_workspace",
            serialized_stats(
                &dataset.loan_workspaces,
                |row| row.id.to_string(),
                vec![
                    financial(
                        "target_contribution",
                        dataset
                            .loan_workspaces
                            .iter()
                            .filter_map(|row| row.target_contribution),
                    ),
                    financial(
                        "actual_contribution",
                        dataset
                            .loan_workspaces
                            .iter()
                            .filter_map(|row| row.actual_contribution),
                    ),
                ],
            ),
        ),
        (
            "loan_workspace_photo",
            serialized_stats(
                &dataset.loan_workspace_photos,
                |row| row.id.to_string(),
                vec![],
            ),
        ),
        (
            "received_email",
            serialized_stats(&dataset.received_emails, |row| row.id.to_string(), vec![]),
        ),
        (
            "received_email_attachment",
            serialized_stats(
                &dataset.received_email_attachments,
                |row| row.id.to_string(),
                vec![],
            ),
        ),
    ])
}

fn float_json(value: Option<f64>) -> Value {
    match value {
        Some(value) => Value::String(format!("0x{:016x}", value.to_bits())),
        None => Value::Null,
    }
}

fn stats(rows: Vec<(String, Value)>, financial: Vec<FinancialStats>) -> TableStats {
    let mut hasher = Hasher::new();
    for (_, row) in &rows {
        let bytes = serde_json::to_vec(row).expect("canonical row serialization cannot fail");
        hasher.update(&(bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    TableStats {
        row_count: rows.len() as u64,
        key_min: rows.first().map(|(key, _)| key.clone()),
        key_max: rows.last().map(|(key, _)| key.clone()),
        canonical_rows_blake3: hasher.finalize().to_hex().to_string(),
        financial,
    }
}

fn serialized_stats<T: Serialize>(
    rows: &[T],
    key: impl Fn(&T) -> String,
    financial: Vec<FinancialStats>,
) -> TableStats {
    stats(
        rows.iter()
            .map(|row| {
                (
                    key(row),
                    serde_json::to_value(row).expect("validated row serialization cannot fail"),
                )
            })
            .collect(),
        financial,
    )
}

fn financial(name: &str, values: impl IntoIterator<Item = f64>) -> FinancialStats {
    let mut count = 0_u64;
    let mut sum = 0.0_f64;
    let mut bits = Hasher::new();
    for value in values {
        count += 1;
        sum += value;
        bits.update(&value.to_bits().to_le_bytes());
    }
    FinancialStats {
        field: name.to_owned(),
        non_null_count: count,
        bits_blake3: bits.finalize().to_hex().to_string(),
        sum: format!("{sum:.17e}"),
        sum_bits: format!("0x{:016x}", sum.to_bits()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AccountRow;

    #[test]
    fn canonical_stats_distinguish_float_bits() {
        let account = |balance| AccountRow {
            id: 1,
            name: "Cash".into(),
            kind: "cash".into(),
            balance: Some(balance),
            balance_as_of_date: Some("2025-01-01".into()),
            source_type: None,
            source_ref: None,
            metadata: None,
            balance_updated_at: Some("2025-01-01T00:00:00.000Z".into()),
            is_primary: 1,
            is_active: 1,
            notes: None,
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
        };
        let mut positive = Dataset::default();
        positive.accounts.push(account(0.0));
        let mut negative = Dataset::default();
        negative.accounts.push(account(-0.0));
        assert_ne!(
            dataset_stats(&positive)["account"].canonical_rows_blake3,
            dataset_stats(&negative)["account"].canonical_rows_blake3
        );
    }
}
