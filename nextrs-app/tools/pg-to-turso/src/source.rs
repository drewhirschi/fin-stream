use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use sqlx::{Connection, PgConnection, Row, postgres::PgRow};

use crate::{
    blocker::ManifestSafeBlocker,
    convert::{
        argon2id_password_hash, balance_as_of_date, boolean_integer, canonical_timestamp,
        finite_magnitude, finite_number, iso_date, json_array_text, json_text, nonempty, one_of,
        timestamp_unix_seconds,
    },
    manifest::{ColumnInventory, RelationClassification, RelationInventory, RelationKind},
    model::{
        AccountRow, AppUserRow, Dataset, IntegrationConnectionRow, LoanWorkspacePhotoRow,
        LoanWorkspaceRow, MonarchCredentialRow, PortfolioSnapshotRow, ReceivedEmailAttachmentRow,
        ReceivedEmailRow, SequenceState, SettingRow, StreamEventRow, StreamRow, StreamScheduleRow,
        StreamViewRow, StreamViewStreamRow, SyncLogRow, TmoAccountRow, TmoCredentialRow,
        TmoImportLoanRow, TmoImportOverviewRow, TmoImportPaymentRow, TmoPaymentEventLinkRow,
    },
    schedule_transform::{LegacyEventRow, transform_event},
};

const MAPPED_RELATIONS: [(&str, &str); 22] = [
    ("public", "app_user"),
    ("public", "account"),
    ("public", "stream"),
    ("public", "stream_view"),
    ("public", "stream_view_stream"),
    ("public", "stream_schedule"),
    ("public", "stream_event"),
    ("intg", "integration_connection"),
    ("intg", "tmo_import_overview"),
    ("intg", "tmo_import_loan"),
    ("intg", "tmo_import_payment"),
    ("intg", "tmo_account"),
    ("intg", "tmo_credential"),
    ("intg", "monarch_credential"),
    ("intg", "tmo_payment_event_link"),
    ("public", "portfolio_snapshot"),
    ("public", "settings"),
    ("public", "sync_log"),
    ("intg", "loan_workspace"),
    ("intg", "loan_workspace_photo"),
    ("intg", "received_email"),
    ("intg", "received_email_attachment"),
];

#[derive(Clone, Copy)]
struct SerialRelation {
    source_schema: &'static str,
    source_table: &'static str,
    target_table: &'static str,
}

const SERIAL_RELATIONS: [SerialRelation; 16] = [
    serial("public", "app_user", "app_user"),
    serial("public", "account", "account"),
    serial("public", "stream", "stream"),
    serial("public", "stream_view", "stream_view"),
    serial("public", "stream_schedule", "stream_schedule"),
    serial("public", "stream_event", "stream_event"),
    serial(
        "intg",
        "integration_connection",
        "intg_integration_connection",
    ),
    serial("intg", "tmo_import_overview", "intg_tmo_import_overview"),
    serial("intg", "tmo_import_loan", "intg_tmo_import_loan"),
    serial("intg", "tmo_import_payment", "intg_tmo_import_payment"),
    serial("public", "portfolio_snapshot", "portfolio_snapshot"),
    serial("public", "sync_log", "sync_log"),
    serial("intg", "loan_workspace", "intg_loan_workspace"),
    serial("intg", "loan_workspace_photo", "intg_loan_workspace_photo"),
    serial("intg", "received_email", "intg_received_email"),
    serial(
        "intg",
        "received_email_attachment",
        "intg_received_email_attachment",
    ),
];

const fn serial(
    source_schema: &'static str,
    source_table: &'static str,
    target_table: &'static str,
) -> SerialRelation {
    SerialRelation {
        source_schema,
        source_table,
        target_table,
    }
}

pub struct SourceSnapshot {
    connection: PgConnection,
    pub snapshot_id: String,
    stream_shape: Option<StreamSourceShape>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StreamSourceShape {
    Upgraded,
    LegacySigned,
}

impl SourceSnapshot {
    pub async fn open(source_url: &str) -> Result<Self> {
        let mut connection = PgConnection::connect(source_url)
            .await
            .map_err(|_| anyhow::anyhow!("could not connect to source PostgreSQL"))?;
        sqlx::query("BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .execute(&mut connection)
            .await
            .context("could not open read-only repeatable-read source snapshot")?;
        let row = sqlx::query(
            "SELECT current_setting('transaction_isolation') AS isolation, \
                    current_setting('transaction_read_only') AS read_only, \
                    txid_current_snapshot()::text AS snapshot_id",
        )
        .fetch_one(&mut connection)
        .await
        .context("could not verify source snapshot")?;
        let isolation: String = row.try_get("isolation")?;
        let read_only: String = row.try_get("read_only")?;
        if isolation != "repeatable read" || read_only != "on" {
            bail!("source transaction did not enter REPEATABLE READ, READ ONLY mode")
        }
        Ok(Self {
            connection,
            snapshot_id: row.try_get("snapshot_id")?,
            stream_shape: None,
        })
    }

    pub async fn inventory(&mut self) -> Result<Vec<RelationInventory>> {
        let relation_rows = sqlx::query(
            "SELECT DISTINCT ns.nspname AS schema_name, c.relname, c.relkind::text AS relkind,
                    CASE WHEN owner.oid IS NULL THEN NULL
                         ELSE owner_ns.nspname || '.' || owner.relname END AS owned_by
             FROM pg_class c
             JOIN pg_namespace ns ON ns.oid = c.relnamespace
             LEFT JOIN pg_depend dep
               ON c.relkind = 'S'
              AND dep.objid = c.oid
              AND dep.classid = 'pg_class'::regclass
              AND dep.refclassid = 'pg_class'::regclass
              AND dep.deptype IN ('a', 'i')
             LEFT JOIN pg_class owner ON owner.oid = dep.refobjid
             LEFT JOIN pg_namespace owner_ns ON owner_ns.oid = owner.relnamespace
             WHERE ns.nspname IN ('public', 'intg', 'tower_sessions')
               AND c.relkind IN ('r', 'p', 'v', 'm', 'S', 'f')
             ORDER BY ns.nspname, c.relname",
        )
        .fetch_all(&mut self.connection)
        .await
        .context("could not inventory PostgreSQL relations")?;

        let mut relations = Vec::with_capacity(relation_rows.len());
        for row in relation_rows {
            let schema: String = row.try_get("schema_name")?;
            let name: String = row.try_get("relname")?;
            let kind = RelationKind::from_pg_relkind(row.try_get::<&str, _>("relkind")?);
            let owned_by: Option<String> = row.try_get("owned_by")?;
            let (classification, reason) =
                classify_relation(&schema, &name, &kind, owned_by.as_deref());
            let columns = if kind.has_rows() {
                self.columns(&schema, &name).await?
            } else {
                Vec::new()
            };
            let source_count = if kind.has_rows() {
                Some(self.count_relation(&schema, &name).await?)
            } else {
                None
            };
            if schema == "public" && name == "stream" && kind.has_rows() {
                self.stream_shape = stream_source_shape(&columns);
            }
            relations.push(RelationInventory {
                schema,
                name,
                kind,
                classification,
                reason,
                owned_by,
                source_count,
                columns,
                source_stats: None,
                destination_stats: None,
            });
        }
        Ok(relations)
    }

    async fn columns(&mut self, schema: &str, table: &str) -> Result<Vec<ColumnInventory>> {
        let rows = sqlx::query(
            "SELECT column_name, udt_name, is_nullable, ordinal_position
             FROM information_schema.columns
             WHERE table_schema = $1 AND table_name = $2
             ORDER BY ordinal_position",
        )
        .bind(schema)
        .bind(table)
        .fetch_all(&mut self.connection)
        .await
        .with_context(|| format!("could not inventory columns for {schema}.{table}"))?;
        rows.into_iter()
            .map(|row| {
                Ok(ColumnInventory {
                    name: row.try_get("column_name")?,
                    pg_type: row.try_get("udt_name")?,
                    nullable: row.try_get::<String, _>("is_nullable")? == "YES",
                    ordinal: row.try_get("ordinal_position")?,
                })
            })
            .collect()
    }

    async fn count_relation(&mut self, schema: &str, table: &str) -> Result<i64> {
        let query = format!(
            "SELECT COUNT(*)::BIGINT AS row_count FROM {}.{}",
            quote_identifier(schema),
            quote_identifier(table)
        );
        sqlx::query(&query)
            .fetch_one(&mut self.connection)
            .await
            .with_context(|| format!("could not count {schema}.{table}"))?
            .try_get("row_count")
            .context("relation count was not a BIGINT")
    }

    pub async fn read_dataset(&mut self) -> Result<Dataset> {
        let stream_shape = self
            .stream_shape
            .context("source stream shape was not established by inventory")?;
        let streams = self.read_streams(stream_shape).await?;
        let stream_schedules = self.read_stream_schedules(stream_shape).await?;
        let stream_events = self
            .read_stream_events(&streams, &stream_schedules, stream_shape)
            .await?;
        let integration_connections = self.read_integration_connections().await?;
        let received_emails = self.read_received_emails().await?;
        let received_email_attachments = self
            .read_received_email_attachments(&received_emails)
            .await?;
        Ok(Dataset {
            app_users: self.read_app_users().await?,
            accounts: self.read_accounts().await?,
            streams,
            stream_views: self.read_stream_views().await?,
            stream_view_streams: self.read_stream_view_streams().await?,
            stream_schedules,
            stream_events,
            tmo_import_overviews: self.read_tmo_import_overviews().await?,
            tmo_import_loans: self.read_tmo_import_loans().await?,
            tmo_import_payments: self.read_tmo_import_payments().await?,
            tmo_accounts: self.read_tmo_accounts().await?,
            tmo_credentials: self.read_tmo_credentials().await?,
            monarch_credentials: self.read_monarch_credentials().await?,
            tmo_payment_event_links: self.read_tmo_payment_event_links().await?,
            portfolio_snapshots: self.read_portfolio_snapshots().await?,
            settings: self.read_settings().await?,
            sync_logs: self.read_sync_logs(&integration_connections).await?,
            loan_workspaces: self.read_loan_workspaces().await?,
            loan_workspace_photos: self.read_loan_workspace_photos().await?,
            received_emails,
            received_email_attachments,
            integration_connections,
        })
    }

    async fn read_app_users(&mut self) -> Result<Vec<AppUserRow>> {
        let rows = sqlx::query(
            "SELECT id, email, password_hash, display_name, is_active::INTEGER::BIGINT AS is_active,
                    created_at, updated_at
             FROM public.app_user ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        let users: Vec<AppUserRow> = rows
            .into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let is_active = boolean_integer(
                    row.try_get("is_active")?,
                    &format!("app_user[{id}].is_active"),
                )?;
                let password_hash = nonempty(
                    row.try_get("password_hash")?,
                    &format!("app_user[{id}].password_hash"),
                )?;
                let password_hash = if is_active == 1 {
                    match argon2id_password_hash(
                        password_hash,
                        &format!("app_user[{id}].password_hash"),
                    ) {
                        Ok(hash) => hash,
                        Err(error) => {
                            let detail = error.to_string();
                            return Err(anyhow::Error::new(ManifestSafeBlocker::new(
                                "an active app_user password hash is not an Argon2id v19 PHC hash with the target runtime's supported parameters",
                            )))
                            .with_context(|| {
                                format!("app_user[{id}] cannot be migrated: {detail}")
                            });
                        }
                    }
                } else {
                    password_hash
                };
                Ok(AppUserRow {
                    id,
                    email: nonempty(row.try_get("email")?, &format!("app_user[{id}].email"))?,
                    password_hash,
                    display_name: row.try_get("display_name")?,
                    is_active,
                    created_at: timestamp_unix_seconds(
                        row.try_get("created_at")?,
                        &format!("app_user[{id}].created_at"),
                    )?,
                    updated_at: timestamp_unix_seconds(
                        row.try_get("updated_at")?,
                        &format!("app_user[{id}].updated_at"),
                    )?,
                })
            })
            .collect::<Result<_>>()?;
        if !users.iter().any(|user| user.is_active == 1) {
            return Err(ManifestSafeBlocker::new(
                "the source has no active app_user; the target would have no usable login",
            )
            .into());
        }
        Ok(users)
    }

    async fn read_accounts(&mut self) -> Result<Vec<AccountRow>> {
        let rows = sqlx::query(
            "SELECT id, name, kind, balance, source_type, source_ref, metadata,
                    balance_updated_at, is_primary::INTEGER::BIGINT AS is_primary,
                    is_active::INTEGER::BIGINT AS is_active, notes, created_at, updated_at
             FROM public.account ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let balance = row
                    .try_get::<Option<f64>, _>("balance")?
                    .map(|value| finite_number(value, &format!("account[{id}].balance")))
                    .transpose()?;
                let raw_balance_updated_at: Option<String> = row.try_get("balance_updated_at")?;
                Ok(AccountRow {
                    id,
                    name: nonempty(row.try_get("name")?, &format!("account[{id}].name"))?,
                    kind: nonempty(row.try_get("kind")?, &format!("account[{id}].kind"))?,
                    balance,
                    balance_as_of_date: balance_as_of_date(
                        balance,
                        raw_balance_updated_at.as_deref(),
                    )?,
                    source_type: row.try_get("source_type")?,
                    source_ref: row.try_get("source_ref")?,
                    metadata: json_text(
                        row.try_get("metadata")?,
                        &format!("account[{id}].metadata"),
                    )?,
                    balance_updated_at: raw_balance_updated_at
                        .map(|value| {
                            canonical_timestamp(
                                &value,
                                &format!("account[{id}].balance_updated_at"),
                            )
                        })
                        .transpose()?,
                    is_primary: boolean_integer(
                        row.try_get("is_primary")?,
                        &format!("account[{id}].is_primary"),
                    )?,
                    is_active: boolean_integer(
                        row.try_get("is_active")?,
                        &format!("account[{id}].is_active"),
                    )?,
                    notes: row.try_get("notes")?,
                    created_at: canonical_timestamp(
                        row.try_get("created_at")?,
                        &format!("account[{id}].created_at"),
                    )?,
                    updated_at: canonical_timestamp(
                        row.try_get("updated_at")?,
                        &format!("account[{id}].updated_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_streams(&mut self, shape: StreamSourceShape) -> Result<Vec<StreamRow>> {
        let query = match shape {
            StreamSourceShape::Upgraded => {
                "SELECT id, name, type, kind, direction, amount_certainty, description,
                        default_account_id, configuration, parent_id,
                        is_active::INTEGER::BIGINT AS is_active, created_at, updated_at
                 FROM public.stream ORDER BY id"
            }
            StreamSourceShape::LegacySigned => {
                "SELECT id, name, type, kind, description, default_account_id,
                        configuration, parent_id,
                        is_active::INTEGER::BIGINT AS is_active, created_at, updated_at
                 FROM public.stream ORDER BY id"
            }
        };
        let rows = sqlx::query(query).fetch_all(&mut self.connection).await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let required = |value: Option<String>, name: &str| -> Result<String> {
                    nonempty(
                        value.with_context(|| format!("stream[{id}].{name} is NULL"))?,
                        &format!("stream[{id}].{name}"),
                    )
                };
                let stream_type = nonempty(row.try_get("type")?, &format!("stream[{id}].type"))?;
                let kind = required(row.try_get("kind")?, "kind")?;
                let (direction, amount_certainty) = match shape {
                    StreamSourceShape::Upgraded => (
                        one_of(
                            required(row.try_get("direction")?, "direction")?,
                            &["in", "out"],
                            &format!("stream[{id}].direction"),
                        )?,
                        one_of(
                            required(row.try_get("amount_certainty")?, "amount_certainty")?,
                            &["known", "estimated"],
                            &format!("stream[{id}].amount_certainty"),
                        )?,
                    ),
                    StreamSourceShape::LegacySigned => legacy_stream_semantics(&stream_type, &kind),
                };
                Ok(StreamRow {
                    id,
                    name: nonempty(row.try_get("name")?, &format!("stream[{id}].name"))?,
                    stream_type,
                    kind,
                    direction,
                    amount_certainty,
                    description: row.try_get("description")?,
                    default_account_id: row.try_get("default_account_id")?,
                    configuration: json_text(
                        row.try_get("configuration")?,
                        &format!("stream[{id}].configuration"),
                    )?,
                    parent_id: row.try_get("parent_id")?,
                    is_active: boolean_integer(
                        row.try_get("is_active")?,
                        &format!("stream[{id}].is_active"),
                    )?,
                    created_at: canonical_timestamp(
                        row.try_get("created_at")?,
                        &format!("stream[{id}].created_at"),
                    )?,
                    updated_at: canonical_timestamp(
                        row.try_get("updated_at")?,
                        &format!("stream[{id}].updated_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_stream_views(&mut self) -> Result<Vec<StreamViewRow>> {
        let rows = sqlx::query(
            "SELECT id, name, description, is_default::INTEGER::BIGINT AS is_default,
                    is_active::INTEGER::BIGINT AS is_active, created_at, updated_at
             FROM public.stream_view ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                Ok(StreamViewRow {
                    id,
                    name: nonempty(row.try_get("name")?, &format!("stream_view[{id}].name"))?,
                    description: row.try_get("description")?,
                    is_default: boolean_integer(
                        row.try_get("is_default")?,
                        &format!("stream_view[{id}].is_default"),
                    )?,
                    is_active: boolean_integer(
                        row.try_get("is_active")?,
                        &format!("stream_view[{id}].is_active"),
                    )?,
                    created_at: canonical_timestamp(
                        row.try_get("created_at")?,
                        &format!("stream_view[{id}].created_at"),
                    )?,
                    updated_at: canonical_timestamp(
                        row.try_get("updated_at")?,
                        &format!("stream_view[{id}].updated_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_stream_view_streams(&mut self) -> Result<Vec<StreamViewStreamRow>> {
        let rows = sqlx::query(
            "SELECT stream_view_id, stream_id, created_at
             FROM public.stream_view_stream ORDER BY stream_view_id, stream_id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let view_id: i64 = row.try_get("stream_view_id")?;
                let stream_id: i64 = row.try_get("stream_id")?;
                Ok(StreamViewStreamRow {
                    stream_view_id: view_id,
                    stream_id,
                    created_at: canonical_timestamp(
                        row.try_get("created_at")?,
                        &format!("stream_view_stream[{view_id},{stream_id}].created_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_stream_schedules(
        &mut self,
        shape: StreamSourceShape,
    ) -> Result<Vec<StreamScheduleRow>> {
        let rows = sqlx::query(
            "SELECT id, stream_id, account_id, label, amount, frequency, day_of_month,
                    start_date::text AS start_date, end_date::text AS end_date,
                    is_active::INTEGER::BIGINT AS is_active, metadata, created_at, updated_at
             FROM public.stream_schedule ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let day_of_month: Option<i32> = row.try_get("day_of_month")?;
                if let Some(day) = day_of_month
                    && !(1..=31).contains(&day)
                {
                    bail!("stream_schedule[{id}].day_of_month is outside 1..=31")
                }
                Ok(StreamScheduleRow {
                    id,
                    stream_id: row.try_get("stream_id")?,
                    account_id: row.try_get("account_id")?,
                    label: row.try_get("label")?,
                    amount: source_magnitude(
                        row.try_get("amount")?,
                        shape,
                        &format!("stream_schedule[{id}].amount"),
                    )?,
                    frequency: one_of(
                        row.try_get("frequency")?,
                        &[
                            "monthly",
                            "semimonthly",
                            "biweekly",
                            "weekly",
                            "annual",
                            "one_time",
                        ],
                        &format!("stream_schedule[{id}].frequency"),
                    )?,
                    day_of_month: day_of_month.map(i64::from),
                    start_date: iso_date(
                        row.try_get("start_date")?,
                        &format!("stream_schedule[{id}].start_date"),
                    )?,
                    end_date: row
                        .try_get::<Option<String>, _>("end_date")?
                        .map(|date| iso_date(&date, &format!("stream_schedule[{id}].end_date")))
                        .transpose()?,
                    is_active: boolean_integer(
                        row.try_get("is_active")?,
                        &format!("stream_schedule[{id}].is_active"),
                    )?,
                    metadata: json_text(
                        row.try_get("metadata")?,
                        &format!("stream_schedule[{id}].metadata"),
                    )?,
                    created_at: canonical_timestamp(
                        row.try_get("created_at")?,
                        &format!("stream_schedule[{id}].created_at"),
                    )?,
                    updated_at: canonical_timestamp(
                        row.try_get("updated_at")?,
                        &format!("stream_schedule[{id}].updated_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_stream_events(
        &mut self,
        streams: &[StreamRow],
        schedules: &[StreamScheduleRow],
        shape: StreamSourceShape,
    ) -> Result<Vec<StreamEventRow>> {
        let rows = sqlx::query(
            "SELECT id, stream_id, account_id, label, expected_date::text AS expected_date,
                    actual_date::text AS actual_date, amount, status, source_id, source_type,
                    metadata, notes, created_at, updated_at
             FROM public.stream_event ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        let events: Vec<StreamEventRow> = rows
            .into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let amount = source_magnitude(
                    row.try_get("amount")?,
                    shape,
                    &format!("stream_event[{id}].amount"),
                )?;
                let status = one_of(
                    row.try_get("status")?,
                    &["projected", "confirmed", "received"],
                    &format!("stream_event[{id}].status"),
                )?;
                let actual_date = row
                    .try_get::<Option<String>, _>("actual_date")?
                    .map(|date| iso_date(&date, &format!("stream_event[{id}].actual_date")))
                    .transpose()?;
                let event = LegacyEventRow {
                    id,
                    stream_id: row.try_get("stream_id")?,
                    account_id: row.try_get("account_id")?,
                    label: row.try_get("label")?,
                    expected_date: iso_date(
                        row.try_get("expected_date")?,
                        &format!("stream_event[{id}].expected_date"),
                    )?,
                    amount,
                    actual_date,
                    status,
                    source_id: row.try_get("source_id")?,
                    source_type: row.try_get("source_type")?,
                    metadata: json_text(
                        row.try_get("metadata")?,
                        &format!("stream_event[{id}].metadata"),
                    )?,
                    notes: row.try_get("notes")?,
                    created_at: canonical_timestamp(
                        row.try_get("created_at")?,
                        &format!("stream_event[{id}].created_at"),
                    )?,
                    updated_at: canonical_timestamp(
                        row.try_get("updated_at")?,
                        &format!("stream_event[{id}].updated_at"),
                    )?,
                };
                transform_event(event, streams, schedules)
            })
            .collect::<Result<_>>()?;

        validate_transformed_event_identities(&events)?;
        Ok(events)
    }

    async fn read_integration_connections(&mut self) -> Result<Vec<IntegrationConnectionRow>> {
        let rows = sqlx::query(
            "SELECT id, slug, name, provider, status, sync_cadence, last_synced_at,
                    last_error, metadata, next_scheduled_at, created_at, updated_at
             FROM intg.integration_connection ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                Ok(IntegrationConnectionRow {
                    id,
                    slug: nonempty(
                        row.try_get("slug")?,
                        &format!("integration_connection[{id}].slug"),
                    )?,
                    name: nonempty(
                        row.try_get("name")?,
                        &format!("integration_connection[{id}].name"),
                    )?,
                    provider: nonempty(
                        row.try_get("provider")?,
                        &format!("integration_connection[{id}].provider"),
                    )?,
                    status: one_of(
                        row.try_get("status")?,
                        &["active", "error"],
                        &format!("integration_connection[{id}].status"),
                    )?,
                    sync_cadence: nonempty(
                        row.try_get("sync_cadence")?,
                        &format!("integration_connection[{id}].sync_cadence"),
                    )?,
                    last_synced_at: optional_timestamp(
                        &row,
                        "last_synced_at",
                        &format!("integration_connection[{id}].last_synced_at"),
                    )?,
                    last_error: row.try_get("last_error")?,
                    metadata: json_text(
                        row.try_get("metadata")?,
                        &format!("integration_connection[{id}].metadata"),
                    )?,
                    next_scheduled_at: optional_timestamp(
                        &row,
                        "next_scheduled_at",
                        &format!("integration_connection[{id}].next_scheduled_at"),
                    )?,
                    created_at: required_timestamp(
                        &row,
                        "created_at",
                        &format!("integration_connection[{id}].created_at"),
                    )?,
                    updated_at: required_timestamp(
                        &row,
                        "updated_at",
                        &format!("integration_connection[{id}].updated_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_tmo_import_overviews(&mut self) -> Result<Vec<TmoImportOverviewRow>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, snapshot_date::text AS snapshot_date,
                    portfolio_value, portfolio_yield, portfolio_count::BIGINT AS portfolio_count,
                    ytd_interest, ytd_principal, trust_balance, outstanding_checks,
                    service_fees, processing_state, raw_payload, created_at, updated_at
             FROM intg.tmo_import_overview ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                Ok(TmoImportOverviewRow {
                    id,
                    connection_id: row.try_get("connection_id")?,
                    snapshot_date: iso_date(
                        row.try_get("snapshot_date")?,
                        &format!("tmo_import_overview[{id}].snapshot_date"),
                    )?,
                    portfolio_value: optional_finite(
                        &row,
                        "portfolio_value",
                        &format!("tmo_import_overview[{id}].portfolio_value"),
                    )?,
                    portfolio_yield: optional_finite(
                        &row,
                        "portfolio_yield",
                        &format!("tmo_import_overview[{id}].portfolio_yield"),
                    )?,
                    portfolio_count: row.try_get("portfolio_count")?,
                    ytd_interest: optional_finite(
                        &row,
                        "ytd_interest",
                        &format!("tmo_import_overview[{id}].ytd_interest"),
                    )?,
                    ytd_principal: optional_finite(
                        &row,
                        "ytd_principal",
                        &format!("tmo_import_overview[{id}].ytd_principal"),
                    )?,
                    trust_balance: optional_finite(
                        &row,
                        "trust_balance",
                        &format!("tmo_import_overview[{id}].trust_balance"),
                    )?,
                    outstanding_checks: optional_finite(
                        &row,
                        "outstanding_checks",
                        &format!("tmo_import_overview[{id}].outstanding_checks"),
                    )?,
                    service_fees: optional_finite(
                        &row,
                        "service_fees",
                        &format!("tmo_import_overview[{id}].service_fees"),
                    )?,
                    processing_state: one_of(
                        row.try_get("processing_state")?,
                        &["captured"],
                        &format!("tmo_import_overview[{id}].processing_state"),
                    )?,
                    raw_payload: json_text(
                        row.try_get("raw_payload")?,
                        &format!("tmo_import_overview[{id}].raw_payload"),
                    )?,
                    created_at: required_timestamp(
                        &row,
                        "created_at",
                        &format!("tmo_import_overview[{id}].created_at"),
                    )?,
                    updated_at: required_timestamp(
                        &row,
                        "updated_at",
                        &format!("tmo_import_overview[{id}].updated_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_tmo_import_loans(&mut self) -> Result<Vec<TmoImportLoanRow>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, stream_id, loan_account, borrower_name,
                    property_address, property_city, property_state, property_zip,
                    property_description, property_type, property_priority::BIGINT AS property_priority,
                    occupancy, appraised_value, ltv, percent_owned, priority::BIGINT AS priority,
                    loan_type::BIGINT AS loan_type, interest_rate, note_rate, original_balance,
                    loan_balance, principal_balance, regular_payment, payment_frequency,
                    maturity_date::text AS maturity_date, next_payment_date::text AS next_payment_date,
                    interest_paid_to::text AS interest_paid_to, billed_through::text AS billed_through,
                    term_left_months::BIGINT AS term_left_months,
                    is_delinquent::INTEGER::BIGINT AS is_delinquent,
                    is_active::INTEGER::BIGINT AS is_active,
                    raw_summary_payload, raw_detail_payload, summary_imported_at,
                    detail_imported_at, created_at, updated_at
             FROM intg.tmo_import_loan ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("tmo_import_loan[{id}].{name}");
                Ok(TmoImportLoanRow {
                    id,
                    connection_id: row.try_get("connection_id")?,
                    stream_id: row.try_get("stream_id")?,
                    loan_account: nonempty(row.try_get("loan_account")?, &field("loan_account"))?,
                    borrower_name: row.try_get("borrower_name")?,
                    property_address: row.try_get("property_address")?,
                    property_city: row.try_get("property_city")?,
                    property_state: row.try_get("property_state")?,
                    property_zip: row.try_get("property_zip")?,
                    property_description: row.try_get("property_description")?,
                    property_type: row.try_get("property_type")?,
                    property_priority: row.try_get("property_priority")?,
                    occupancy: row.try_get("occupancy")?,
                    appraised_value: optional_finite(
                        &row,
                        "appraised_value",
                        &field("appraised_value"),
                    )?,
                    ltv: optional_finite(&row, "ltv", &field("ltv"))?,
                    percent_owned: optional_finite(&row, "percent_owned", &field("percent_owned"))?,
                    priority: row.try_get("priority")?,
                    loan_type: row.try_get("loan_type")?,
                    interest_rate: optional_finite(&row, "interest_rate", &field("interest_rate"))?,
                    note_rate: optional_finite(&row, "note_rate", &field("note_rate"))?,
                    original_balance: optional_finite(
                        &row,
                        "original_balance",
                        &field("original_balance"),
                    )?,
                    loan_balance: optional_finite(&row, "loan_balance", &field("loan_balance"))?,
                    principal_balance: optional_finite(
                        &row,
                        "principal_balance",
                        &field("principal_balance"),
                    )?,
                    regular_payment: optional_finite(
                        &row,
                        "regular_payment",
                        &field("regular_payment"),
                    )?,
                    payment_frequency: row.try_get("payment_frequency")?,
                    maturity_date: optional_date(&row, "maturity_date", &field("maturity_date"))?,
                    next_payment_date: optional_date(
                        &row,
                        "next_payment_date",
                        &field("next_payment_date"),
                    )?,
                    interest_paid_to: optional_date(
                        &row,
                        "interest_paid_to",
                        &field("interest_paid_to"),
                    )?,
                    billed_through: optional_date(
                        &row,
                        "billed_through",
                        &field("billed_through"),
                    )?,
                    term_left_months: row.try_get("term_left_months")?,
                    is_delinquent: optional_boolean(
                        &row,
                        "is_delinquent",
                        &field("is_delinquent"),
                    )?,
                    is_active: optional_boolean(&row, "is_active", &field("is_active"))?,
                    raw_summary_payload: json_text(
                        row.try_get("raw_summary_payload")?,
                        &field("raw_summary_payload"),
                    )?,
                    raw_detail_payload: json_text(
                        row.try_get("raw_detail_payload")?,
                        &field("raw_detail_payload"),
                    )?,
                    summary_imported_at: optional_timestamp(
                        &row,
                        "summary_imported_at",
                        &field("summary_imported_at"),
                    )?,
                    detail_imported_at: optional_timestamp(
                        &row,
                        "detail_imported_at",
                        &field("detail_imported_at"),
                    )?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                    updated_at: required_timestamp(&row, "updated_at", &field("updated_at"))?,
                })
            })
            .collect()
    }

    async fn read_tmo_import_payments(&mut self) -> Result<Vec<TmoImportPaymentRow>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, external_id, loan_account, borrower_name,
                    property_name, check_number, check_date::text AS check_date, amount,
                    service_fee, interest, principal, charges, late_charges, other,
                    processing_state, normalized_event_source_id, raw_payload,
                    imported_at, updated_at
             FROM intg.tmo_import_payment ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("tmo_import_payment[{id}].{name}");
                Ok(TmoImportPaymentRow {
                    id,
                    connection_id: row.try_get("connection_id")?,
                    external_id: nonempty(row.try_get("external_id")?, &field("external_id"))?,
                    loan_account: nonempty(row.try_get("loan_account")?, &field("loan_account"))?,
                    borrower_name: nonempty(
                        row.try_get("borrower_name")?,
                        &field("borrower_name"),
                    )?,
                    property_name: nonempty(
                        row.try_get("property_name")?,
                        &field("property_name"),
                    )?,
                    check_number: row.try_get("check_number")?,
                    check_date: iso_date(row.try_get("check_date")?, &field("check_date"))?,
                    amount: required_finite(&row, "amount", &field("amount"))?,
                    service_fee: required_finite(&row, "service_fee", &field("service_fee"))?,
                    interest: required_finite(&row, "interest", &field("interest"))?,
                    principal: required_finite(&row, "principal", &field("principal"))?,
                    charges: required_finite(&row, "charges", &field("charges"))?,
                    late_charges: required_finite(&row, "late_charges", &field("late_charges"))?,
                    other: required_finite(&row, "other", &field("other"))?,
                    processing_state: one_of(
                        row.try_get("processing_state")?,
                        &["captured", "normalized"],
                        &field("processing_state"),
                    )?,
                    normalized_event_source_id: row.try_get("normalized_event_source_id")?,
                    raw_payload: json_text(row.try_get("raw_payload")?, &field("raw_payload"))?,
                    imported_at: required_timestamp(&row, "imported_at", &field("imported_at"))?,
                    updated_at: required_timestamp(&row, "updated_at", &field("updated_at"))?,
                })
            })
            .collect()
    }

    async fn read_tmo_accounts(&mut self) -> Result<Vec<TmoAccountRow>> {
        let rows = sqlx::query(
            "SELECT id, company_id, account_number, source_rec_id, display_name,
                    email, last_login_at, created_at, updated_at
             FROM intg.tmo_account ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                if id != 1 {
                    bail!("tmo_account id must be 1")
                }
                Ok(TmoAccountRow {
                    id,
                    company_id: nonempty(row.try_get("company_id")?, "tmo_account.company_id")?,
                    account_number: nonempty(
                        row.try_get("account_number")?,
                        "tmo_account.account_number",
                    )?,
                    source_rec_id: row.try_get("source_rec_id")?,
                    display_name: row.try_get("display_name")?,
                    email: row.try_get("email")?,
                    last_login_at: optional_timestamp(
                        &row,
                        "last_login_at",
                        "tmo_account.last_login_at",
                    )?,
                    created_at: required_timestamp(&row, "created_at", "tmo_account.created_at")?,
                    updated_at: required_timestamp(&row, "updated_at", "tmo_account.updated_at")?,
                })
            })
            .collect()
    }

    async fn read_tmo_credentials(&mut self) -> Result<Vec<TmoCredentialRow>> {
        let rows = sqlx::query(
            "SELECT connection_id, company_id, account_number, pin_ciphertext,
                    pin_nonce, key_version::BIGINT AS key_version, created_at, updated_at
             FROM intg.tmo_credential ORDER BY connection_id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let connection_id: i64 = row.try_get("connection_id")?;
                let field = |name: &str| format!("tmo_credential[{connection_id}].{name}");
                Ok(TmoCredentialRow {
                    connection_id,
                    company_id: nonempty(row.try_get("company_id")?, &field("company_id"))?,
                    account_number: nonempty(
                        row.try_get("account_number")?,
                        &field("account_number"),
                    )?,
                    pin_ciphertext: nonempty(
                        row.try_get("pin_ciphertext")?,
                        &field("pin_ciphertext"),
                    )?,
                    pin_nonce: nonempty(row.try_get("pin_nonce")?, &field("pin_nonce"))?,
                    key_version: row.try_get("key_version")?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                    updated_at: required_timestamp(&row, "updated_at", &field("updated_at"))?,
                })
            })
            .collect()
    }

    async fn read_monarch_credentials(&mut self) -> Result<Vec<MonarchCredentialRow>> {
        let rows = sqlx::query(
            "SELECT connection_id, access_token_ciphertext, access_token_nonce,
                    default_account_id, key_version::BIGINT AS key_version, created_at, updated_at
             FROM intg.monarch_credential ORDER BY connection_id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let connection_id: i64 = row.try_get("connection_id")?;
                let field = |name: &str| format!("monarch_credential[{connection_id}].{name}");
                Ok(MonarchCredentialRow {
                    connection_id,
                    access_token_ciphertext: nonempty(
                        row.try_get("access_token_ciphertext")?,
                        &field("access_token_ciphertext"),
                    )?,
                    access_token_nonce: nonempty(
                        row.try_get("access_token_nonce")?,
                        &field("access_token_nonce"),
                    )?,
                    default_account_id: nonempty(
                        row.try_get("default_account_id")?,
                        &field("default_account_id"),
                    )?,
                    key_version: row.try_get("key_version")?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                    updated_at: required_timestamp(&row, "updated_at", &field("updated_at"))?,
                })
            })
            .collect()
    }

    async fn read_tmo_payment_event_links(&mut self) -> Result<Vec<TmoPaymentEventLinkRow>> {
        let rows = sqlx::query(
            "SELECT tmo_payment_id, stream_event_id, created_at
             FROM intg.tmo_payment_event_link ORDER BY tmo_payment_id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let payment_id: i64 = row.try_get("tmo_payment_id")?;
                Ok(TmoPaymentEventLinkRow {
                    tmo_payment_id: payment_id,
                    stream_event_id: row.try_get("stream_event_id")?,
                    created_at: required_timestamp(
                        &row,
                        "created_at",
                        &format!("tmo_payment_event_link[{payment_id}].created_at"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_portfolio_snapshots(&mut self) -> Result<Vec<PortfolioSnapshotRow>> {
        let rows = sqlx::query(
            "SELECT id, snapshot_date::text AS snapshot_date, portfolio_value,
                    portfolio_yield, portfolio_count::BIGINT AS portfolio_count,
                    ytd_interest, ytd_principal, trust_balance, outstanding_checks,
                    service_fees, synced_at
             FROM public.portfolio_snapshot ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("portfolio_snapshot[{id}].{name}");
                Ok(PortfolioSnapshotRow {
                    id,
                    snapshot_date: iso_date(
                        row.try_get("snapshot_date")?,
                        &field("snapshot_date"),
                    )?,
                    portfolio_value: optional_finite(
                        &row,
                        "portfolio_value",
                        &field("portfolio_value"),
                    )?,
                    portfolio_yield: optional_finite(
                        &row,
                        "portfolio_yield",
                        &field("portfolio_yield"),
                    )?,
                    portfolio_count: row.try_get("portfolio_count")?,
                    ytd_interest: optional_finite(&row, "ytd_interest", &field("ytd_interest"))?,
                    ytd_principal: optional_finite(&row, "ytd_principal", &field("ytd_principal"))?,
                    trust_balance: optional_finite(&row, "trust_balance", &field("trust_balance"))?,
                    outstanding_checks: optional_finite(
                        &row,
                        "outstanding_checks",
                        &field("outstanding_checks"),
                    )?,
                    service_fees: optional_finite(&row, "service_fees", &field("service_fees"))?,
                    synced_at: required_timestamp(&row, "synced_at", &field("synced_at"))?,
                })
            })
            .collect()
    }

    async fn read_settings(&mut self) -> Result<Vec<SettingRow>> {
        let rows = sqlx::query("SELECT key, value, updated_at FROM public.settings ORDER BY key")
            .fetch_all(&mut self.connection)
            .await?;
        rows.into_iter()
            .map(|row| {
                let key: String = nonempty(row.try_get("key")?, "settings.key")?;
                Ok(SettingRow {
                    updated_at: required_timestamp(
                        &row,
                        "updated_at",
                        &format!("settings[{key}].updated_at"),
                    )?,
                    key,
                    value: row.try_get("value")?,
                })
            })
            .collect()
    }

    async fn read_sync_logs(
        &mut self,
        connections: &[IntegrationConnectionRow],
    ) -> Result<Vec<SyncLogRow>> {
        let rows = sqlx::query(
            "SELECT id, connection_slug, started_at, finished_at, status,
                    error_message, endpoints_hit, events_upserted::BIGINT AS events_upserted,
                    loans_upserted::BIGINT AS loans_upserted,
                    snapshots_created::BIGINT AS snapshots_created
             FROM public.sync_log ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("sync_log[{id}].{name}");
                let started_at = required_timestamp(&row, "started_at", &field("started_at"))?;
                let connection_slug = map_legacy_sync_slug(
                    row.try_get("connection_slug")?,
                    &started_at,
                    connections,
                    &field("connection_slug"),
                )?;
                let status: String = row.try_get("status")?;
                let finished_at = optional_timestamp(&row, "finished_at", &field("finished_at"))?;
                let (status, finished_at) =
                    validate_terminal_sync(status, &started_at, finished_at, &field("status"))?;
                Ok(SyncLogRow {
                    id,
                    connection_slug,
                    scheduled_for: None,
                    started_at,
                    finished_at: Some(finished_at),
                    status,
                    error_message: row.try_get("error_message")?,
                    endpoints_hit: row.try_get("endpoints_hit")?,
                    events_upserted: nonnegative_counter(
                        row.try_get("events_upserted")?,
                        &field("events_upserted"),
                    )?,
                    loans_upserted: nonnegative_counter(
                        row.try_get("loans_upserted")?,
                        &field("loans_upserted"),
                    )?,
                    snapshots_created: nonnegative_counter(
                        row.try_get("snapshots_created")?,
                        &field("snapshots_created"),
                    )?,
                })
            })
            .collect()
    }

    async fn read_loan_workspaces(&mut self) -> Result<Vec<LoanWorkspaceRow>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, loan_account, redfin_url, zillow_url,
                    decision_status, target_contribution, actual_contribution, notes,
                    created_at, updated_at
             FROM intg.loan_workspace ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("loan_workspace[{id}].{name}");
                Ok(LoanWorkspaceRow {
                    id,
                    connection_id: row.try_get("connection_id")?,
                    loan_account: nonempty(row.try_get("loan_account")?, &field("loan_account"))?,
                    redfin_url: row.try_get("redfin_url")?,
                    zillow_url: row.try_get("zillow_url")?,
                    decision_status: row
                        .try_get::<Option<String>, _>("decision_status")?
                        .map(|status| {
                            one_of(
                                status,
                                &["new", "reviewing", "committed", "funded", "passed"],
                                &field("decision_status"),
                            )
                        })
                        .transpose()?,
                    target_contribution: optional_finite(
                        &row,
                        "target_contribution",
                        &field("target_contribution"),
                    )?,
                    actual_contribution: optional_finite(
                        &row,
                        "actual_contribution",
                        &field("actual_contribution"),
                    )?,
                    notes: row.try_get("notes")?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                    updated_at: required_timestamp(&row, "updated_at", &field("updated_at"))?,
                })
            })
            .collect()
    }

    async fn read_loan_workspace_photos(&mut self) -> Result<Vec<LoanWorkspacePhotoRow>> {
        let rows = sqlx::query(
            "SELECT id, connection_id, loan_account, provider, caption, source_url,
                    image_url, sort_order::BIGINT AS sort_order,
                    is_featured::INTEGER::BIGINT AS is_featured, created_at
             FROM intg.loan_workspace_photo ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        let photos: Vec<LoanWorkspacePhotoRow> = rows
            .into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("loan_workspace_photo[{id}].{name}");
                Ok(LoanWorkspacePhotoRow {
                    id,
                    connection_id: row.try_get("connection_id")?,
                    loan_account: nonempty(row.try_get("loan_account")?, &field("loan_account"))?,
                    provider: nonempty(row.try_get("provider")?, &field("provider"))?,
                    caption: row.try_get("caption")?,
                    source_url: nonempty(row.try_get("source_url")?, &field("source_url"))?,
                    image_url: nonempty(row.try_get("image_url")?, &field("image_url"))?,
                    sort_order: row.try_get("sort_order")?,
                    is_featured: boolean_integer(
                        row.try_get("is_featured")?,
                        &field("is_featured"),
                    )?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                })
            })
            .collect::<Result<_>>()?;
        let mut featured = BTreeSet::new();
        for photo in photos.iter().filter(|photo| photo.is_featured == 1) {
            if !featured.insert((photo.connection_id, photo.loan_account.as_str())) {
                return Err(ManifestSafeBlocker::new(
                    "more than one featured photo exists for a loan workspace",
                )
                .into());
            }
        }
        Ok(photos)
    }

    async fn read_received_emails(&mut self) -> Result<Vec<ReceivedEmailRow>> {
        let rows = sqlx::query(
            "SELECT id, resend_email_id, from_address, to_addresses, subject,
                    received_at, body_s3_key, body_content_type, loan_account,
                    processing_state, error_message, raw_webhook_payload,
                    created_at, updated_at
             FROM intg.received_email ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("received_email[{id}].{name}");
                Ok(ReceivedEmailRow {
                    id,
                    resend_email_id: nonempty(
                        row.try_get("resend_email_id")?,
                        &field("resend_email_id"),
                    )?,
                    from_address: nonempty(row.try_get("from_address")?, &field("from_address"))?,
                    to_addresses: json_array_text(
                        row.try_get("to_addresses")?,
                        &field("to_addresses"),
                    )?,
                    subject: row.try_get("subject")?,
                    received_at: required_timestamp(&row, "received_at", &field("received_at"))?,
                    body_s3_key: row.try_get("body_s3_key")?,
                    body_content_type: row.try_get("body_content_type")?,
                    loan_account: row
                        .try_get::<Option<String>, _>("loan_account")?
                        .map(|account| nonempty(account, &field("loan_account")))
                        .transpose()?,
                    processing_state: one_of(
                        row.try_get("processing_state")?,
                        &["pending", "stored", "error"],
                        &field("processing_state"),
                    )?,
                    error_message: row.try_get("error_message")?,
                    raw_webhook_payload: json_text(
                        row.try_get("raw_webhook_payload")?,
                        &field("raw_webhook_payload"),
                    )?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                    updated_at: required_timestamp(&row, "updated_at", &field("updated_at"))?,
                })
            })
            .collect()
    }

    async fn read_received_email_attachments(
        &mut self,
        emails: &[ReceivedEmailRow],
    ) -> Result<Vec<ReceivedEmailAttachmentRow>> {
        let email_ids = emails
            .iter()
            .map(|email| (email.id, email.resend_email_id.as_str()))
            .collect::<BTreeMap<_, _>>();
        let rows = sqlx::query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type,
                    size_bytes::BIGINT AS size_bytes, s3_key, processing_state, created_at
             FROM intg.received_email_attachment ORDER BY id",
        )
        .fetch_all(&mut self.connection)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: i64 = row.try_get("id")?;
                let field = |name: &str| format!("received_email_attachment[{id}].{name}");
                let size_bytes: Option<i64> = row.try_get("size_bytes")?;
                if size_bytes.is_some_and(|size| size < 0) {
                    bail!("{} is negative", field("size_bytes"))
                }
                let email_id = row.try_get("email_id")?;
                let resend_attachment_id = nonempty(
                    row.try_get("resend_attachment_id")?,
                    &field("resend_attachment_id"),
                )?;
                let filename = nonempty(row.try_get("filename")?, &field("filename"))?;
                let source_s3_key: Option<String> = row.try_get("s3_key")?;
                let s3_key = transform_attachment_key(
                    email_ids.get(&email_id).copied(),
                    &resend_attachment_id,
                    &filename,
                    source_s3_key.as_deref(),
                )?;
                Ok(ReceivedEmailAttachmentRow {
                    id,
                    email_id,
                    resend_attachment_id,
                    filename,
                    content_type: nonempty(row.try_get("content_type")?, &field("content_type"))?,
                    size_bytes,
                    s3_key,
                    processing_state: one_of(
                        row.try_get("processing_state")?,
                        &["pending", "stored", "error"],
                        &field("processing_state"),
                    )?,
                    created_at: required_timestamp(&row, "created_at", &field("created_at"))?,
                })
            })
            .collect()
    }

    pub async fn read_sequence_states(
        &mut self,
        inventory: &[RelationInventory],
        dataset: &Dataset,
    ) -> Result<Vec<SequenceState>> {
        let maximums = BTreeMap::from([
            ("app_user", dataset.app_users.last().map(|row| row.id)),
            ("account", dataset.accounts.last().map(|row| row.id)),
            ("stream", dataset.streams.last().map(|row| row.id)),
            ("stream_view", dataset.stream_views.last().map(|row| row.id)),
            (
                "stream_schedule",
                dataset.stream_schedules.last().map(|row| row.id),
            ),
            (
                "stream_event",
                dataset.stream_events.last().map(|row| row.id),
            ),
            (
                "intg_integration_connection",
                dataset.integration_connections.last().map(|row| row.id),
            ),
            (
                "intg_tmo_import_overview",
                dataset.tmo_import_overviews.last().map(|row| row.id),
            ),
            (
                "intg_tmo_import_loan",
                dataset.tmo_import_loans.last().map(|row| row.id),
            ),
            (
                "intg_tmo_import_payment",
                dataset.tmo_import_payments.last().map(|row| row.id),
            ),
            (
                "portfolio_snapshot",
                dataset.portfolio_snapshots.last().map(|row| row.id),
            ),
            ("sync_log", dataset.sync_logs.last().map(|row| row.id)),
            (
                "intg_loan_workspace",
                dataset.loan_workspaces.last().map(|row| row.id),
            ),
            (
                "intg_loan_workspace_photo",
                dataset.loan_workspace_photos.last().map(|row| row.id),
            ),
            (
                "intg_received_email",
                dataset.received_emails.last().map(|row| row.id),
            ),
            (
                "intg_received_email_attachment",
                dataset.received_email_attachments.last().map(|row| row.id),
            ),
        ]);
        let mut states = Vec::new();
        for mapping in SERIAL_RELATIONS {
            let owner = format!("{}.{}", mapping.source_schema, mapping.source_table);
            let sequence = inventory
                .iter()
                .find(|relation| {
                    relation.kind == RelationKind::Sequence
                        && relation.owned_by.as_deref() == Some(owner.as_str())
                })
                .with_context(|| format!("{owner} has no owned sequence"))?;
            let query = format!(
                "SELECT last_value::BIGINT AS last_value, is_called FROM {}.{}",
                quote_identifier(&sequence.schema),
                quote_identifier(&sequence.name)
            );
            let row = sqlx::query(&query).fetch_one(&mut self.connection).await?;
            let parameters = sqlx::query(
                "SELECT increment_by::BIGINT AS increment_by, cycle,
                        min_value::BIGINT AS min_value, max_value::BIGINT AS max_value
                 FROM pg_sequences WHERE schemaname = $1 AND sequencename = $2",
            )
            .bind(&sequence.schema)
            .bind(&sequence.name)
            .fetch_one(&mut self.connection)
            .await?;
            let increment: i64 = parameters.try_get("increment_by")?;
            if increment != 1 {
                bail!(
                    "{}.{} has unsupported increment {increment}",
                    sequence.schema,
                    sequence.name
                )
            }
            let cycle: bool = parameters.try_get("cycle")?;
            if cycle {
                bail!(
                    "{}.{} is cyclic, which SQLite AUTOINCREMENT cannot preserve",
                    sequence.schema,
                    sequence.name
                )
            }
            let sequence_min: i64 = parameters.try_get("min_value")?;
            let sequence_max: i64 = parameters.try_get("max_value")?;
            if sequence_min < 1 || sequence_max != i64::MAX {
                bail!(
                    "{}.{} has unsupported bounds",
                    sequence.schema,
                    sequence.name
                )
            }
            let last_value: i64 = row.try_get("last_value")?;
            let is_called: bool = row.try_get("is_called")?;
            let source_effective_next = if is_called {
                last_value
                    .checked_add(1)
                    .context("source sequence overflow")?
            } else {
                last_value
            };
            let imported_max = maximums[mapping.target_table];
            let target_effective_next = source_effective_next.max(
                imported_max
                    .unwrap_or(0)
                    .checked_add(1)
                    .context("ID overflow")?,
            );
            if target_effective_next <= 0 {
                bail!("{owner} sequence would generate a non-positive ID")
            }
            states.push(SequenceState {
                table: mapping.target_table.to_owned(),
                source_sequence: format!("{}.{}", sequence.schema, sequence.name),
                source_effective_next,
                imported_max,
                target_effective_next,
            });
        }
        Ok(states)
    }

    pub async fn rollback(mut self) -> Result<()> {
        sqlx::query("ROLLBACK")
            .execute(&mut self.connection)
            .await
            .context("could not close source snapshot")?;
        Ok(())
    }
}

fn required_finite(row: &PgRow, column: &str, field: &str) -> Result<f64> {
    finite_number(row.try_get(column)?, field)
}

fn optional_finite(row: &PgRow, column: &str, field: &str) -> Result<Option<f64>> {
    row.try_get::<Option<f64>, _>(column)?
        .map(|value| finite_number(value, field))
        .transpose()
}

fn optional_boolean(row: &PgRow, column: &str, field: &str) -> Result<Option<i64>> {
    row.try_get::<Option<i64>, _>(column)?
        .map(|value| boolean_integer(value, field))
        .transpose()
}

fn optional_date(row: &PgRow, column: &str, field: &str) -> Result<Option<String>> {
    row.try_get::<Option<String>, _>(column)?
        .map(|value| iso_date(&value, field))
        .transpose()
}

fn required_timestamp(row: &PgRow, column: &str, field: &str) -> Result<String> {
    canonical_timestamp(row.try_get(column)?, field)
}

fn optional_timestamp(row: &PgRow, column: &str, field: &str) -> Result<Option<String>> {
    row.try_get::<Option<String>, _>(column)?
        .map(|value| canonical_timestamp(&value, field))
        .transpose()
}

fn nonnegative_counter(value: Option<i64>, field: &str) -> Result<i64> {
    let value = value.unwrap_or(0);
    if value < 0 {
        bail!("{field} is negative")
    }
    Ok(value)
}

fn map_legacy_sync_slug(
    value: Option<String>,
    started_at: &str,
    connections: &[IntegrationConnectionRow],
    field: &str,
) -> Result<String> {
    if let Some(slug) = value {
        return nonempty(slug, field);
    }
    let tmo_count = connections
        .iter()
        .filter(|connection| connection.slug == "tmo" && connection.provider == "mortgage_office")
        .count();
    if tmo_count != 1
        || connections.iter().any(|connection| {
            connection.slug != "tmo" && connection.created_at.as_str() <= started_at
        })
    {
        return Err(ManifestSafeBlocker::new(
            "a legacy sync_log NULL connection_slug cannot be proven to predate non-TMO connections and map uniquely to tmo",
        )
        .into());
    }
    Ok("tmo".to_owned())
}

fn validate_terminal_sync(
    status: String,
    started_at: &str,
    finished_at: Option<String>,
    field: &str,
) -> Result<(String, String)> {
    let status = one_of(status, &["running", "success", "error"], field)?;
    if status == "running" {
        return Err(ManifestSafeBlocker::new(
            "sync_log contains a running execution; quiesce provider work before export",
        )
        .into());
    }
    let finished_at = finished_at.ok_or_else(|| {
        ManifestSafeBlocker::new("a finished sync_log status has no finished_at timestamp")
    })?;
    if finished_at.as_str() < started_at {
        return Err(ManifestSafeBlocker::new("sync_log finished_at precedes started_at").into());
    }
    Ok((status, finished_at))
}

fn validate_transformed_event_identities(events: &[StreamEventRow]) -> Result<()> {
    let mut identities = BTreeMap::new();
    for event in events {
        let (Some(source_type), Some(source_id)) = (&event.source_type, &event.source_id) else {
            continue;
        };
        let key = (event.stream_id, source_type.clone(), source_id.clone());
        if let Some((previous_id, previous_received)) =
            identities.insert(key, (event.id, event.status == "received"))
        {
            let message = if previous_received || event.status == "received" {
                "legacy schedule events including immutable received history collapse to one target stable cadence slot; safe canonicalization requires reviewed historical schedule identity"
            } else {
                "two legacy schedule events collapse to one target stable cadence slot; safe canonicalization requires reviewed schedule identity"
            };
            return Err(anyhow::Error::new(ManifestSafeBlocker::new(message))).with_context(|| {
                format!(
                    "stream_event[{previous_id}] and stream_event[{}] have the same transformed source identity",
                    event.id
                )
            });
        }
    }
    Ok(())
}

fn transform_attachment_key(
    resend_email_id: Option<&str>,
    resend_attachment_id: &str,
    filename: &str,
    source_key: Option<&str>,
) -> Result<Option<String>> {
    let Some(source_key) = source_key else {
        return Ok(None);
    };
    let resend_email_id = resend_email_id.ok_or_else(|| {
        ManifestSafeBlocker::new("an attachment references an email row absent from the snapshot")
    })?;
    if !is_canonical_object_segment(resend_email_id)
        || !is_canonical_object_segment(resend_attachment_id)
    {
        return Err(ManifestSafeBlocker::new(
            "an attachment provider identifier cannot form a canonical target object key",
        )
        .into());
    }

    let legacy_filename = filename.replace(['/', '\\', '\0'], "_");
    let legacy_key = format!("emails/{resend_email_id}/attachments/{legacy_filename}");
    let target_key = format!("emails/{resend_email_id}/attachments/{resend_attachment_id}");
    if source_key != legacy_key && source_key != target_key {
        return Err(ManifestSafeBlocker::new(
            "an attachment object key does not match either the legacy or target writer contract",
        )
        .into());
    }
    Ok(Some(target_key))
}

fn is_canonical_object_segment(value: &str) -> bool {
    !value.is_empty()
        && value == value.trim()
        && !matches!(value, "." | "..")
        && !value.contains(['/', '\\', '%', '\0'])
        && !value.chars().any(char::is_control)
}

fn stream_source_shape(columns: &[ColumnInventory]) -> Option<StreamSourceShape> {
    let has_direction = columns.iter().any(|column| column.name == "direction");
    let has_certainty = columns
        .iter()
        .any(|column| column.name == "amount_certainty");
    match (has_direction, has_certainty) {
        (true, true) => Some(StreamSourceShape::Upgraded),
        (false, false) => Some(StreamSourceShape::LegacySigned),
        _ => None,
    }
}

fn legacy_stream_semantics(stream_type: &str, kind: &str) -> (String, String) {
    let direction = if matches!(kind, "manual_expense" | "credit_card")
        || matches!(stream_type, "manual_expense" | "credit_card_due")
    {
        "out"
    } else {
        "in"
    };
    let amount_certainty = if kind == "credit_card" {
        "estimated"
    } else {
        "known"
    };
    (direction.to_owned(), amount_certainty.to_owned())
}

fn source_magnitude(value: f64, shape: StreamSourceShape, field: &str) -> Result<f64> {
    match shape {
        StreamSourceShape::Upgraded => finite_magnitude(value, field),
        StreamSourceShape::LegacySigned => finite_number(value, field).map(f64::abs),
    }
}

pub fn classify_relation(
    schema: &str,
    name: &str,
    kind: &RelationKind,
    owned_by: Option<&str>,
) -> (RelationClassification, String) {
    if schema == "tower_sessions" && name == "session" && kind.has_rows() {
        return (
            RelationClassification::IntentionallyDiscarded,
            "legacy login sessions are not portable and app_session starts empty".to_owned(),
        );
    }
    if *kind == RelationKind::Sequence {
        return match owned_by {
            Some(owner)
                if SERIAL_RELATIONS.iter().any(|mapping| {
                    owner == format!("{}.{}", mapping.source_schema, mapping.source_table)
                }) =>
            {
                (
                    RelationClassification::SequenceMetadata,
                    format!("preserve effective next ID for {owner}"),
                )
            }
            _ => (
                RelationClassification::Unclassified,
                "sequence has no explicitly classified owner".to_owned(),
            ),
        };
    }
    if MAPPED_RELATIONS.contains(&(schema, name)) {
        if !matches!(kind, RelationKind::Table | RelationKind::PartitionedTable) {
            return (
                RelationClassification::Unclassified,
                "mapped source relation is not a table".to_owned(),
            );
        }
        return (
            RelationClassification::Transformed,
            "typed mapping into the implemented target schema".to_owned(),
        );
    }
    if is_legacy_public_remnant(schema, name) {
        return (
            RelationClassification::ForbiddenLegacyRemnant,
            "legacy public integration/TMO relation must be upgraded into intg before export"
                .to_owned(),
        );
    }
    (
        RelationClassification::Unclassified,
        "relation is absent from the checked cutover contract".to_owned(),
    )
}

fn is_legacy_public_remnant(schema: &str, name: &str) -> bool {
    schema == "public"
        && (name.starts_with("tmo_")
            || matches!(
                name,
                "integration_connection" | "received_email" | "received_email_attachment"
            ))
}

pub fn inventory_blockers(relations: &[RelationInventory]) -> Vec<String> {
    let mut blockers = BTreeSet::new();
    let names: BTreeSet<String> = relations
        .iter()
        .filter(|relation| relation.kind.has_rows())
        .map(RelationInventory::qualified_name)
        .collect();
    for (schema, table) in MAPPED_RELATIONS {
        let qualified = format!("{schema}.{table}");
        if !names.contains(&qualified) {
            blockers.insert(format!("required mapped relation {qualified} is missing"));
        }
    }
    for relation in relations {
        match relation.classification {
            RelationClassification::BlockedPendingTargetSchema
            | RelationClassification::ForbiddenLegacyRemnant
            | RelationClassification::Unclassified => {
                blockers.insert(format!(
                    "{}: {}",
                    relation.qualified_name(),
                    relation.reason
                ));
            }
            RelationClassification::Transformed => {
                if let Err(error) = validate_mapped_columns(relation) {
                    blockers.insert(format!("{}: {error:#}", relation.qualified_name()));
                }
            }
            _ => {}
        }
        if relation.schema == "public"
            && relation.name == "stream_event"
            && relation
                .columns
                .iter()
                .any(|column| column.name == "scheduled_date")
        {
            blockers.insert(
                "public.stream_event.scheduled_date is a forbidden legacy column".to_owned(),
            );
        }
    }
    blockers.into_iter().collect()
}

#[derive(Clone, Copy)]
struct ExpectedColumn {
    name: &'static str,
    pg_type: &'static str,
    nullable: bool,
}

const fn col(name: &'static str, pg_type: &'static str, nullable: bool) -> ExpectedColumn {
    ExpectedColumn {
        name,
        pg_type,
        nullable,
    }
}

fn expected_columns(table: &str) -> Option<Vec<ExpectedColumn>> {
    match table {
        "app_user" => Some(vec![
            col("id", "int8", false),
            col("email", "text", false),
            col("password_hash", "text", false),
            col("display_name", "text", true),
            col("is_active", "int4", false),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "account" => Some(vec![
            col("id", "int8", false),
            col("name", "text", false),
            col("kind", "text", false),
            col("balance", "float8", true),
            col("source_type", "text", true),
            col("source_ref", "text", true),
            col("metadata", "text", true),
            col("balance_updated_at", "text", true),
            col("is_primary", "int4", false),
            col("is_active", "int4", false),
            col("notes", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "stream" => Some(vec![
            col("id", "int8", false),
            col("name", "text", false),
            col("type", "text", false),
            col("description", "text", true),
            col("is_active", "int4", false),
            col("created_at", "text", false),
            col("updated_at", "text", false),
            col("kind", "text", true),
            col("default_account_id", "int8", true),
            col("configuration", "text", true),
            col("parent_id", "int8", true),
            col("direction", "text", true),
            col("amount_certainty", "text", true),
        ]),
        "stream_view" => Some(vec![
            col("id", "int8", false),
            col("name", "text", false),
            col("description", "text", true),
            col("is_default", "int4", false),
            col("is_active", "int4", false),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "stream_view_stream" => Some(vec![
            col("stream_view_id", "int8", false),
            col("stream_id", "int8", false),
            col("created_at", "text", false),
        ]),
        "stream_schedule" => Some(vec![
            col("id", "int8", false),
            col("stream_id", "int8", false),
            col("account_id", "int8", true),
            col("label", "text", true),
            col("amount", "float8", false),
            col("frequency", "text", false),
            col("day_of_month", "int4", true),
            col("start_date", "date", false),
            col("end_date", "date", true),
            col("is_active", "int4", false),
            col("metadata", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "stream_event" => Some(vec![
            col("id", "int8", false),
            col("stream_id", "int8", false),
            col("account_id", "int8", true),
            col("label", "text", true),
            col("expected_date", "date", false),
            col("actual_date", "date", true),
            col("amount", "float8", false),
            col("status", "text", false),
            col("source_id", "text", true),
            col("source_type", "text", true),
            col("metadata", "text", true),
            col("notes", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "integration_connection" => Some(vec![
            col("id", "int8", false),
            col("slug", "text", false),
            col("name", "text", false),
            col("provider", "text", false),
            col("status", "text", false),
            col("sync_cadence", "text", false),
            col("last_synced_at", "text", true),
            col("last_error", "text", true),
            col("metadata", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
            col("next_scheduled_at", "text", true),
        ]),
        "tmo_import_overview" => Some(vec![
            col("id", "int8", false),
            col("connection_id", "int8", false),
            col("snapshot_date", "date", false),
            col("portfolio_value", "float8", true),
            col("portfolio_yield", "float8", true),
            col("portfolio_count", "int4", true),
            col("ytd_interest", "float8", true),
            col("ytd_principal", "float8", true),
            col("trust_balance", "float8", true),
            col("outstanding_checks", "float8", true),
            col("service_fees", "float8", true),
            col("processing_state", "text", false),
            col("raw_payload", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "tmo_import_loan" => Some(vec![
            col("id", "int8", false),
            col("connection_id", "int8", false),
            col("stream_id", "int8", true),
            col("loan_account", "text", false),
            col("borrower_name", "text", true),
            col("property_address", "text", true),
            col("property_city", "text", true),
            col("property_state", "text", true),
            col("property_zip", "text", true),
            col("property_description", "text", true),
            col("property_type", "text", true),
            col("property_priority", "int4", true),
            col("occupancy", "text", true),
            col("appraised_value", "float8", true),
            col("ltv", "float8", true),
            col("percent_owned", "float8", true),
            col("priority", "int4", true),
            col("loan_type", "int4", true),
            col("interest_rate", "float8", true),
            col("note_rate", "float8", true),
            col("original_balance", "float8", true),
            col("principal_balance", "float8", true),
            col("regular_payment", "float8", true),
            col("payment_frequency", "text", true),
            col("maturity_date", "date", true),
            col("next_payment_date", "date", true),
            col("interest_paid_to", "date", true),
            col("billed_through", "date", true),
            col("term_left_months", "int4", true),
            col("is_delinquent", "int4", true),
            col("is_active", "int4", true),
            col("raw_summary_payload", "text", true),
            col("raw_detail_payload", "text", true),
            col("summary_imported_at", "text", true),
            col("detail_imported_at", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
            col("loan_balance", "float8", true),
        ]),
        "tmo_import_payment" => Some(vec![
            col("id", "int8", false),
            col("connection_id", "int8", false),
            col("external_id", "text", false),
            col("loan_account", "text", false),
            col("borrower_name", "text", false),
            col("property_name", "text", false),
            col("check_number", "text", true),
            col("check_date", "date", false),
            col("amount", "float8", false),
            col("service_fee", "float8", false),
            col("interest", "float8", false),
            col("principal", "float8", false),
            col("charges", "float8", false),
            col("late_charges", "float8", false),
            col("other", "float8", false),
            col("processing_state", "text", false),
            col("normalized_event_source_id", "text", true),
            col("raw_payload", "text", true),
            col("imported_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "tmo_account" => Some(vec![
            col("id", "int8", false),
            col("company_id", "text", false),
            col("account_number", "text", false),
            col("source_rec_id", "text", true),
            col("display_name", "text", true),
            col("email", "text", true),
            col("last_login_at", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "tmo_credential" => Some(vec![
            col("connection_id", "int8", false),
            col("company_id", "text", false),
            col("account_number", "text", false),
            col("pin_ciphertext", "text", false),
            col("pin_nonce", "text", false),
            col("key_version", "int4", false),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "monarch_credential" => Some(vec![
            col("connection_id", "int8", false),
            col("access_token_ciphertext", "text", false),
            col("access_token_nonce", "text", false),
            col("default_account_id", "text", false),
            col("key_version", "int4", false),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "tmo_payment_event_link" => Some(vec![
            col("tmo_payment_id", "int8", false),
            col("stream_event_id", "int8", false),
            col("created_at", "text", false),
        ]),
        "portfolio_snapshot" => Some(vec![
            col("id", "int8", false),
            col("snapshot_date", "date", false),
            col("portfolio_value", "float8", true),
            col("portfolio_yield", "float8", true),
            col("portfolio_count", "int4", true),
            col("ytd_interest", "float8", true),
            col("ytd_principal", "float8", true),
            col("trust_balance", "float8", true),
            col("outstanding_checks", "float8", true),
            col("service_fees", "float8", true),
            col("synced_at", "text", false),
        ]),
        "settings" => Some(vec![
            col("key", "text", false),
            col("value", "text", false),
            col("updated_at", "text", false),
        ]),
        "sync_log" => Some(vec![
            col("id", "int8", false),
            col("connection_slug", "text", true),
            col("started_at", "text", false),
            col("finished_at", "text", true),
            col("status", "text", false),
            col("error_message", "text", true),
            col("endpoints_hit", "text", true),
            col("events_upserted", "int4", true),
            col("loans_upserted", "int4", true),
            col("snapshots_created", "int4", true),
        ]),
        "loan_workspace" => Some(vec![
            col("id", "int8", false),
            col("connection_id", "int8", false),
            col("loan_account", "text", false),
            col("redfin_url", "text", true),
            col("zillow_url", "text", true),
            col("decision_status", "text", true),
            col("target_contribution", "float8", true),
            col("actual_contribution", "float8", true),
            col("notes", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "loan_workspace_photo" => Some(vec![
            col("id", "int8", false),
            col("connection_id", "int8", false),
            col("loan_account", "text", false),
            col("provider", "text", false),
            col("caption", "text", true),
            col("source_url", "text", false),
            col("image_url", "text", false),
            col("sort_order", "int4", false),
            col("created_at", "text", false),
            col("is_featured", "bool", false),
        ]),
        "received_email" => Some(vec![
            col("id", "int8", false),
            col("resend_email_id", "text", false),
            col("from_address", "text", false),
            col("to_addresses", "text", false),
            col("subject", "text", true),
            col("received_at", "text", false),
            col("body_s3_key", "text", true),
            col("body_content_type", "text", true),
            col("loan_account", "text", true),
            col("processing_state", "text", false),
            col("error_message", "text", true),
            col("raw_webhook_payload", "text", true),
            col("created_at", "text", false),
            col("updated_at", "text", false),
        ]),
        "received_email_attachment" => Some(vec![
            col("id", "int8", false),
            col("email_id", "int8", false),
            col("resend_attachment_id", "text", false),
            col("filename", "text", false),
            col("content_type", "text", false),
            col("size_bytes", "int4", true),
            col("s3_key", "text", true),
            col("processing_state", "text", false),
            col("created_at", "text", false),
        ]),
        _ => None,
    }
}

fn validate_mapped_columns(relation: &RelationInventory) -> Result<()> {
    let upgraded = expected_columns(&relation.name).context("no expected column contract")?;
    let actual: BTreeMap<&str, (&str, bool)> = relation
        .columns
        .iter()
        .map(|column| {
            (
                column.name.as_str(),
                (column.pg_type.as_str(), column.nullable),
            )
        })
        .collect();
    let upgraded_names: BTreeSet<&str> = upgraded.iter().map(|column| column.name).collect();
    let actual_names: BTreeSet<&str> = actual.keys().copied().collect();
    let legacy_stream = relation.schema == "public" && relation.name == "stream";
    let legacy = upgraded
        .iter()
        .copied()
        .filter(|column| !matches!(column.name, "direction" | "amount_certainty"))
        .collect::<Vec<_>>();
    let legacy_names: BTreeSet<&str> = legacy.iter().map(|column| column.name).collect();
    let expected = if actual_names == upgraded_names {
        &upgraded
    } else if legacy_stream && actual_names == legacy_names {
        &legacy
    } else {
        bail!(
            "column set differs from supported source contract (expected upgraded {upgraded_names:?}{}; got {actual_names:?})",
            if legacy_stream {
                format!(" or known legacy stream {legacy_names:?}")
            } else {
                String::new()
            }
        )
    };
    for column in expected {
        let (actual_type, actual_nullable) = actual[column.name];
        if actual_type != column.pg_type || actual_nullable != column.nullable {
            bail!(
                "column {} expected {} nullable={}, got {} nullable={}",
                column.name,
                column.pg_type,
                column.nullable,
                actual_type,
                actual_nullable
            )
        }
    }
    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_relation_without(modifier: &[&str]) -> RelationInventory {
        let mut relation = relation("public", "stream", RelationKind::Table);
        relation.columns = expected_columns("stream")
            .unwrap()
            .into_iter()
            .filter(|column| !modifier.contains(&column.name))
            .enumerate()
            .map(|(index, column)| ColumnInventory {
                name: column.name.to_owned(),
                pg_type: column.pg_type.to_owned(),
                nullable: column.nullable,
                ordinal: i32::try_from(index + 1).unwrap(),
            })
            .collect();
        relation
    }

    #[test]
    fn postgres_boolean_queries_cast_through_integer() {
        let source = include_str!("source.rs");
        for column in [
            "is_active",
            "is_primary",
            "is_default",
            "is_delinquent",
            "is_featured",
        ] {
            let invalid_direct_cast = [column, "::", "BIGINT"].concat();
            assert!(
                !source.contains(&invalid_direct_cast),
                "PostgreSQL cannot cast {column} directly from boolean to bigint"
            );
        }
    }

    #[test]
    fn known_legacy_stream_shape_has_explicit_lossless_backfill_rules() {
        let legacy = stream_relation_without(&["direction", "amount_certainty"]);
        assert_eq!(
            stream_source_shape(&legacy.columns),
            Some(StreamSourceShape::LegacySigned)
        );
        validate_mapped_columns(&legacy).unwrap();

        let unknown = stream_relation_without(&["direction"]);
        assert_eq!(stream_source_shape(&unknown.columns), None);
        assert!(validate_mapped_columns(&unknown).is_err());

        assert_eq!(
            legacy_stream_semantics("credit_card_due", "credit_card"),
            ("out".to_owned(), "estimated".to_owned())
        );
        assert_eq!(
            legacy_stream_semantics("manual_income", "manual_income"),
            ("in".to_owned(), "known".to_owned())
        );
        assert_eq!(
            source_magnitude(-125.5, StreamSourceShape::LegacySigned, "amount").unwrap(),
            125.5
        );
        assert!(source_magnitude(-1.0, StreamSourceShape::Upgraded, "amount").is_err());
    }

    fn transformed_event(id: i64, status: &str) -> StreamEventRow {
        StreamEventRow {
            id,
            stream_id: 1,
            account_id: None,
            label: Some("Seed".into()),
            expected_date: "2025-01-01".into(),
            amount: 1.0,
            override_label: None,
            has_label_override: 0,
            override_date: None,
            override_amount: None,
            override_account_id: None,
            has_account_override: 0,
            actual_date: (status == "received").then(|| "2025-01-01".into()),
            actual_amount: (status == "received").then_some(1.0),
            status: status.into(),
            is_excluded: 0,
            exclusion_reason: None,
            source_id: Some("stream_schedule:9:annual:2025".into()),
            source_type: Some("stream_schedule".into()),
            metadata: None,
            notes: None,
            created_at: "2025-01-01T00:00:00.000Z".into(),
            updated_at: "2025-01-01T00:00:00.000Z".into(),
        }
    }

    fn relation(schema: &str, name: &str, kind: RelationKind) -> RelationInventory {
        let (classification, reason) = classify_relation(schema, name, &kind, None);
        RelationInventory {
            schema: schema.into(),
            name: name.into(),
            kind,
            classification,
            reason,
            owned_by: None,
            source_count: Some(0),
            columns: Vec::new(),
            source_stats: None,
            destination_stats: None,
        }
    }

    #[test]
    fn relation_contract_is_explicit_and_fail_closed() {
        assert_eq!(
            relation("public", "account", RelationKind::Table).classification,
            RelationClassification::Transformed
        );
        assert_eq!(
            relation("tower_sessions", "session", RelationKind::Table).classification,
            RelationClassification::IntentionallyDiscarded
        );
        assert_eq!(
            relation("intg", "tmo_credential", RelationKind::Table).classification,
            RelationClassification::Transformed
        );
        assert_eq!(
            relation("public", "tmo_loan", RelationKind::Table).classification,
            RelationClassification::ForbiddenLegacyRemnant
        );
        assert_eq!(
            relation("public", "surprise", RelationKind::Table).classification,
            RelationClassification::Unclassified
        );
    }

    #[test]
    fn unknown_and_invalid_mapped_contracts_are_export_blockers() {
        let relations = vec![
            relation("public", "portfolio_snapshot", RelationKind::Table),
            relation("public", "surprise", RelationKind::Table),
        ];
        let blockers = inventory_blockers(&relations);
        assert!(
            blockers
                .iter()
                .any(|item| item.contains("portfolio_snapshot"))
        );
        assert!(blockers.iter().any(|item| item.contains("surprise")));
        assert!(
            blockers
                .iter()
                .any(|item| item.contains("required mapped relation"))
        );
    }

    #[test]
    fn received_history_stable_slot_collision_is_a_precise_blocker() {
        let events = [
            transformed_event(1, "received"),
            transformed_event(2, "projected"),
        ];
        let error = validate_transformed_event_identities(&events).unwrap_err();
        let blocker = error.downcast_ref::<ManifestSafeBlocker>().unwrap();
        assert!(blocker.message().contains("immutable received history"));
        assert!(error.to_string().contains("stream_event[1]"));
    }

    #[test]
    fn legacy_sync_transform_is_explicit_and_fails_closed() {
        let connection =
            |id, slug: &str, provider: &str, created_at: &str| IntegrationConnectionRow {
                id,
                slug: slug.into(),
                name: slug.into(),
                provider: provider.into(),
                status: "active".into(),
                sync_cadence: "manual".into(),
                last_synced_at: None,
                last_error: None,
                metadata: None,
                next_scheduled_at: None,
                created_at: created_at.into(),
                updated_at: created_at.into(),
            };
        let started_at = "2025-01-02T00:00:00.000Z";
        let tmo = connection(1, "tmo", "mortgage_office", "2024-01-01T00:00:00.000Z");
        let later_monarch = connection(2, "monarch", "monarch", "2025-02-01T00:00:00.000Z");
        assert_eq!(
            map_legacy_sync_slug(
                None,
                started_at,
                &[tmo.clone(), later_monarch],
                "sync_log.connection_slug"
            )
            .unwrap(),
            "tmo"
        );
        let earlier_monarch = connection(2, "monarch", "monarch", "2024-02-01T00:00:00.000Z");
        let error = map_legacy_sync_slug(
            None,
            started_at,
            &[tmo, earlier_monarch],
            "sync_log.connection_slug",
        )
        .unwrap_err();
        assert!(error.downcast_ref::<ManifestSafeBlocker>().is_some());

        assert_eq!(nonnegative_counter(None, "counter").unwrap(), 0);
        assert!(nonnegative_counter(Some(-1), "counter").is_err());
        assert!(
            validate_terminal_sync("running".into(), started_at, None, "sync_log.status")
                .unwrap_err()
                .downcast_ref::<ManifestSafeBlocker>()
                .is_some()
        );
        assert!(
            validate_terminal_sync(
                "success".into(),
                started_at,
                Some("2025-01-01T00:00:00.000Z".into()),
                "sync_log.status"
            )
            .unwrap_err()
            .downcast_ref::<ManifestSafeBlocker>()
            .is_some()
        );
    }

    #[test]
    fn legacy_attachment_filename_key_becomes_provider_id_key() {
        assert_eq!(
            transform_attachment_key(
                Some("email-1"),
                "attachment-2",
                "100% statement.pdf",
                Some("emails/email-1/attachments/100% statement.pdf"),
            )
            .unwrap(),
            Some("emails/email-1/attachments/attachment-2".to_owned())
        );
    }

    #[test]
    fn unknown_attachment_key_contract_blocks_without_echoing_the_key() {
        let error = transform_attachment_key(
            Some("email-1"),
            "attachment-2",
            "statement.pdf",
            Some("somewhere/private.pdf"),
        )
        .unwrap_err();
        let blocker = error.downcast_ref::<ManifestSafeBlocker>().unwrap();
        assert!(!blocker.to_string().contains("private"));
    }
}
