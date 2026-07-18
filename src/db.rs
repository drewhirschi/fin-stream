use std::{fmt, sync::Arc};

use anyhow::Context;
#[cfg(feature = "local-db")]
use libsql::TransactionBehavior;
use libsql::{Builder, Connection, Database, params};

use crate::{auth, config::AppConfig};

const AUTH_MIGRATION: &str = include_str!("../migrations/0001_auth.sql");
const STREAMS_FORECAST_MIGRATION: &str = include_str!("../migrations/0002_streams_forecast.sql");
const INTEGRATIONS_OPERATIONS_MIGRATION: &str =
    include_str!("../migrations/0003_integrations_operations.sql");
const WORKSPACES_INBOX_MIGRATION: &str = include_str!("../migrations/0004_workspaces_inbox.sql");
const RESEND_INBOUND_LEASES_MIGRATION: &str =
    include_str!("../migrations/0005_resend_inbound_leases.sql");

#[derive(Clone, Copy)]
struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "auth",
        sql: AUTH_MIGRATION,
    },
    Migration {
        version: 2,
        name: "streams_forecast",
        sql: STREAMS_FORECAST_MIGRATION,
    },
    Migration {
        version: 3,
        name: "integrations_operations",
        sql: INTEGRATIONS_OPERATIONS_MIGRATION,
    },
    Migration {
        version: 4,
        name: "workspaces_inbox",
        sql: WORKSPACES_INBOX_MIGRATION,
    },
    Migration {
        version: 5,
        name: "resend_inbound_leases",
        sql: RESEND_INBOUND_LEASES_MIGRATION,
    },
];

#[derive(Clone)]
pub struct AppContext {
    database: Arc<Database>,
    /// libSQL's plain `:memory:` database is isolated per connection. The
    /// public test constructor retains one handle so tests exercise the same
    /// ephemeral database; configured file/remote runtimes always leave this
    /// empty and vend a new connection per operation.
    test_connection: Option<Connection>,
}

impl fmt::Debug for AppContext {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("AppContext").finish_non_exhaustive()
    }
}

impl AppContext {
    #[cfg(feature = "local-db")]
    pub async fn connect(config: &AppConfig) -> anyhow::Result<Self> {
        if config.local_database_path.as_os_str() != ":memory:"
            && let Some(parent) = config.local_database_path.parent()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create database directory {}", parent.display()))?;
        }

        let database = Builder::new_local(&config.local_database_path)
            .build()
            .await
            .context("open local libSQL database")?;
        Self::from_database_with_migrations(database, false).await
    }

    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    pub async fn connect(config: &AppConfig) -> anyhow::Result<Self> {
        let database = Builder::new_remote(
            config.turso_database_url.clone(),
            config.turso_auth_token.clone(),
        )
        .build()
        .await
        .context("connect to Turso")?;
        Self::from_database_checked(database).await
    }

    /// Connect the narrow operator CLI without requiring unrelated cookie,
    /// admin, or credential-encryption configuration. The local flavor may
    /// apply package migrations; the remote flavor remains verification-only.
    #[cfg(feature = "local-db")]
    pub(crate) async fn connect_for_operations_from_env() -> anyhow::Result<Self> {
        let path = std::env::var("LIBSQL_LOCAL_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("data/trust-deeds.db"));
        if path.as_os_str() != ":memory:"
            && let Some(parent) = path.parent()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("create database directory {}", parent.display()))?;
        }
        let database = Builder::new_local(path)
            .build()
            .await
            .context("open local libSQL database")?;
        Self::from_database_with_migrations(database, false).await
    }

    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    pub(crate) async fn connect_for_operations_from_env() -> anyhow::Result<Self> {
        let database_url = std::env::var("TURSO_DATABASE_URL")
            .context("TURSO_DATABASE_URL is required for remote-db")?;
        let auth_token = std::env::var("TURSO_AUTH_TOKEN")
            .context("TURSO_AUTH_TOKEN is required for remote-db")?;
        let database = Builder::new_remote(database_url, auth_token)
            .build()
            .await
            .context("connect to Turso")?;
        Self::from_database_checked(database).await
    }

    /// Build a local/test context and apply the package migrations.
    ///
    /// Production Vercel initialization uses `from_database_checked` instead:
    /// cold starts must never mutate schema or bootstrap application data.
    #[cfg(feature = "local-db")]
    pub async fn from_database(database: Database) -> anyhow::Result<Self> {
        Self::from_database_with_migrations(database, true).await
    }

    #[cfg(feature = "local-db")]
    async fn from_database_with_migrations(
        database: Database,
        retain_test_connection: bool,
    ) -> anyhow::Result<Self> {
        let test_connection = if retain_test_connection {
            Some(database.connect().context("open test libSQL connection")?)
        } else {
            None
        };
        let context = Self {
            database: Arc::new(database),
            test_connection,
        };
        context.migrate().await?;
        Ok(context)
    }

    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    async fn from_database_checked(database: Database) -> anyhow::Result<Self> {
        let context = Self {
            database: Arc::new(database),
            test_connection: None,
        };
        context.ensure_schema_current().await?;
        Ok(context)
    }

    /// Vend a configured connection for one request/repository operation.
    /// SQLite foreign-key enforcement is connection-local, so every handle is
    /// enabled and verified before it leaves this factory.
    pub async fn connection(&self) -> anyhow::Result<Connection> {
        let connection = if let Some(connection) = &self.test_connection {
            connection.clone()
        } else {
            self.database.connect().context("open libSQL connection")?
        };
        connection
            .execute("PRAGMA foreign_keys = ON", ())
            .await
            .context("enable SQLite foreign keys")?;
        let mut rows = connection
            .query("PRAGMA foreign_keys", ())
            .await
            .context("verify SQLite foreign keys")?;
        let enabled = rows
            .next()
            .await
            .context("read SQLite foreign-key setting")?
            .context("SQLite did not return its foreign-key setting")?
            .get::<i64>(0)
            .context("decode SQLite foreign-key setting")?;
        if enabled != 1 {
            anyhow::bail!("SQLite foreign-key enforcement is disabled");
        }
        Ok(connection)
    }

    #[cfg(feature = "local-db")]
    async fn migrate(&self) -> anyhow::Result<()> {
        let connection = self.connection().await?;
        ensure_migration_table(&connection).await?;
        for migration in MIGRATIONS {
            apply_migration(&connection, *migration).await?;
        }
        Ok(())
    }

    #[cfg(all(feature = "remote-db", not(feature = "local-db")))]
    async fn ensure_schema_current(&self) -> anyhow::Result<()> {
        let connection = self.connection().await?;
        verify_migrations(&connection).await
    }

    pub async fn bootstrap_admin(
        &self,
        email: Option<&str>,
        password: Option<&str>,
    ) -> anyhow::Result<()> {
        let (Some(email), Some(password)) = (email, password) else {
            return Ok(());
        };
        let email = email.trim().to_lowercase();
        let connection = self.connection().await?;

        let mut rows = connection
            .query(
                "SELECT 1 FROM app_user WHERE email = ?1 LIMIT 1",
                params![email.clone()],
            )
            .await
            .context("check bootstrap user")?;
        if rows.next().await.context("read bootstrap user")?.is_some() {
            return Ok(());
        }

        let password = password.to_owned();
        let password_hash = tokio::task::spawn_blocking(move || auth::hash_password(&password))
            .await
            .context("join password hashing task")??;
        connection
            .execute(
                "INSERT OR IGNORE INTO app_user (email, password_hash, display_name) VALUES (?1, ?2, 'Admin')",
                params![email, password_hash],
            )
            .await
            .context("insert bootstrap user")?;
        Ok(())
    }

    pub async fn active_user_for_login(
        &self,
        email: &str,
    ) -> anyhow::Result<Option<(i64, String)>> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id, password_hash FROM app_user WHERE email = ?1 AND is_active = 1 LIMIT 1",
                params![email],
            )
            .await
            .context("query login user")?;
        let Some(row) = rows.next().await.context("read login user")? else {
            return Ok(None);
        };
        Ok(Some((
            row.get::<i64>(0).context("decode user id")?,
            row.get::<String>(1).context("decode password hash")?,
        )))
    }

    pub async fn ensure_usable_login(&self) -> anyhow::Result<()> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT id, password_hash FROM app_user WHERE is_active = 1 ORDER BY id",
                (),
            )
            .await
            .context("check for an active login user")?;
        let mut found = false;
        while let Some(row) = rows.next().await.context("read active login user check")? {
            found = true;
            let user_id = row.get::<i64>(0).context("decode active login user id")?;
            let hash = row
                .get::<String>(1)
                .context("decode active login password hash")?;
            auth::validate_password_hash(&hash)
                .with_context(|| format!("active login user {user_id} has an unusable hash"))?;
        }
        if !found {
            anyhow::bail!(
                "no active login user exists; run the explicit local bootstrap or import production users"
            );
        }
        Ok(())
    }

    pub async fn user_is_active(&self, user_id: i64) -> anyhow::Result<bool> {
        let connection = self.connection().await?;
        let mut rows = connection
            .query(
                "SELECT 1 FROM app_user WHERE id = ?1 AND is_active = 1 LIMIT 1",
                params![user_id],
            )
            .await
            .context("query active user")?;
        Ok(rows.next().await.context("read active user")?.is_some())
    }
}

#[cfg(feature = "local-db")]
async fn ensure_migration_table(connection: &Connection) -> anyhow::Result<()> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS _schema_migrations (\
                version INTEGER PRIMARY KEY, \
                name TEXT NOT NULL UNIQUE, \
                checksum TEXT NOT NULL CHECK (length(checksum) = 64), \
                applied_at TEXT NOT NULL \
                    DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))\
             ) STRICT;",
        )
        .await
        .context("create schema migration ledger")?;
    Ok(())
}

#[cfg(feature = "local-db")]
async fn apply_migration(connection: &Connection, migration: Migration) -> anyhow::Result<()> {
    let checksum = migration_checksum(migration);
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .await
        .with_context(|| format!("begin migration {} transaction", migration.version))?;

    if let Some((stored_name, stored_checksum)) =
        migration_record(&transaction, migration.version).await?
    {
        verify_migration_record(migration, &checksum, &stored_name, &stored_checksum)?;
        transaction
            .commit()
            .await
            .with_context(|| format!("finish migration {} verification", migration.version))?;
        return Ok(());
    }

    transaction
        .execute_batch(migration.sql)
        .await
        .with_context(|| format!("apply migration {} ({})", migration.version, migration.name))?;
    transaction
        .execute(
            "INSERT INTO _schema_migrations (version, name, checksum) VALUES (?1, ?2, ?3)",
            params![migration.version, migration.name, checksum],
        )
        .await
        .with_context(|| format!("record migration {}", migration.version))?;
    transaction
        .commit()
        .await
        .with_context(|| format!("commit migration {}", migration.version))?;
    Ok(())
}

#[cfg(any(
    all(test, feature = "local-db"),
    all(feature = "remote-db", not(feature = "local-db"))
))]
async fn verify_migrations(connection: &Connection) -> anyhow::Result<()> {
    let mut rows = connection
        .query(
            "SELECT version, name, checksum FROM _schema_migrations ORDER BY version ASC",
            (),
        )
        .await
        .context("read deployed schema migration ledger")?;
    let mut index = 0;
    while let Some(row) = rows.next().await.context("read deployed migration row")? {
        let version = row.get::<i64>(0).context("decode migration version")?;
        let name = row.get::<String>(1).context("decode migration name")?;
        let checksum = row.get::<String>(2).context("decode migration checksum")?;
        let Some(expected) = MIGRATIONS.get(index).copied() else {
            anyhow::bail!("database contains unexpected migration version {version} ({name})");
        };
        if version != expected.version {
            anyhow::bail!(
                "database migration sequence mismatch: expected version {}, found {version}",
                expected.version
            );
        }
        verify_migration_record(expected, &migration_checksum(expected), &name, &checksum)?;
        index += 1;
    }
    if index != MIGRATIONS.len() {
        let missing = MIGRATIONS[index];
        anyhow::bail!(
            "database schema is behind: missing migration {} ({})",
            missing.version,
            missing.name
        );
    }
    Ok(())
}

#[cfg(feature = "local-db")]
async fn migration_record(
    connection: &Connection,
    version: i64,
) -> anyhow::Result<Option<(String, String)>> {
    let mut rows = connection
        .query(
            "SELECT name, checksum FROM _schema_migrations WHERE version = ?1",
            params![version],
        )
        .await
        .with_context(|| format!("query migration {version}"))?;
    let Some(row) = rows
        .next()
        .await
        .with_context(|| format!("read migration {version}"))?
    else {
        return Ok(None);
    };
    Ok(Some((
        row.get(0).context("decode stored migration name")?,
        row.get(1).context("decode stored migration checksum")?,
    )))
}

fn migration_checksum(migration: Migration) -> String {
    blake3::hash(migration.sql.as_bytes()).to_hex().to_string()
}

fn verify_migration_record(
    migration: Migration,
    expected_checksum: &str,
    stored_name: &str,
    stored_checksum: &str,
) -> anyhow::Result<()> {
    if stored_name != migration.name {
        anyhow::bail!(
            "migration {} name mismatch: expected {}, found {stored_name}",
            migration.version,
            migration.name
        );
    }
    if stored_checksum != expected_checksum {
        anyhow::bail!(
            "migration {} ({}) checksum mismatch",
            migration.version,
            migration.name
        );
    }
    Ok(())
}

#[cfg(all(test, feature = "local-db"))]
mod tests {
    use libsql::Builder;

    use super::{AppContext, MIGRATIONS, migration_checksum, verify_migrations};

    #[tokio::test]
    async fn migration_ledger_is_complete_and_idempotent() {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        let context = AppContext::from_database(database).await.unwrap();
        context.migrate().await.unwrap();
        let connection = context.connection().await.unwrap();
        verify_migrations(&connection).await.unwrap();

        let mut rows = connection
            .query(
                "SELECT version, name, checksum FROM _schema_migrations ORDER BY version",
                (),
            )
            .await
            .unwrap();
        for migration in MIGRATIONS {
            let row = rows.next().await.unwrap().unwrap();
            assert_eq!(row.get::<i64>(0).unwrap(), migration.version);
            assert_eq!(row.get::<String>(1).unwrap(), migration.name);
            assert_eq!(
                row.get::<String>(2).unwrap(),
                migration_checksum(*migration)
            );
        }
        assert!(rows.next().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn migration_checksum_tampering_fails_closed() {
        let database = Builder::new_local(":memory:").build().await.unwrap();
        let context = AppContext::from_database(database).await.unwrap();
        let connection = context.connection().await.unwrap();
        connection
            .execute(
                "UPDATE _schema_migrations SET checksum = ?2 WHERE version = ?1",
                libsql::params![2_i64, "0".repeat(64)],
            )
            .await
            .unwrap();

        let error = verify_migrations(&connection).await.unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"));
        assert!(context.migrate().await.is_err());
    }
}
