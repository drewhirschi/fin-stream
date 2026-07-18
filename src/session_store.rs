use std::fmt;

use async_trait::async_trait;
use libsql::params;
use time::OffsetDateTime;
use tower_sessions::{
    SessionStore,
    session::{Id, Record},
    session_store,
};

use crate::db::AppContext;

#[derive(Clone)]
pub struct LibsqlSessionStore {
    context: AppContext,
}

impl LibsqlSessionStore {
    pub fn new(context: AppContext) -> Self {
        Self { context }
    }
}

impl fmt::Debug for LibsqlSessionStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LibsqlSessionStore")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl SessionStore for LibsqlSessionStore {
    async fn create(&self, record: &mut Record) -> session_store::Result<()> {
        let connection = self.context.connection().await.map_err(backend)?;
        for _ in 0..8 {
            let data = encode(record)?;
            let changed = connection
                .execute(
                    "INSERT INTO app_session (id, data, expires_at_unix_s) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(id) DO NOTHING",
                    params![
                        record.id.to_string(),
                        data,
                        record.expiry_date.unix_timestamp()
                    ],
                )
                .await
                .map_err(backend)?;
            if changed == 1 {
                return Ok(());
            }
            record.id = Id::default();
        }
        Err(session_store::Error::Backend(
            "could not allocate a unique session id after 8 attempts".into(),
        ))
    }

    async fn save(&self, record: &Record) -> session_store::Result<()> {
        let connection = self.context.connection().await.map_err(backend)?;
        let data = encode(record)?;
        let changed = connection
            .execute(
                "UPDATE app_session SET data = ?2, expires_at_unix_s = ?3 WHERE id = ?1",
                params![
                    record.id.to_string(),
                    data,
                    record.expiry_date.unix_timestamp()
                ],
            )
            .await
            .map_err(backend)?;
        if changed != 1 {
            return Err(session_store::Error::Backend(
                "session disappeared before it could be saved".into(),
            ));
        }
        Ok(())
    }

    async fn load(&self, session_id: &Id) -> session_store::Result<Option<Record>> {
        let connection = self.context.connection().await.map_err(backend)?;
        let mut rows = connection
            .query(
                "SELECT data, expires_at_unix_s FROM app_session \
                 WHERE id = ?1 AND expires_at_unix_s > ?2 LIMIT 1",
                params![
                    session_id.to_string(),
                    OffsetDateTime::now_utc().unix_timestamp()
                ],
            )
            .await
            .map_err(backend)?;
        let Some(row) = rows.next().await.map_err(backend)? else {
            return Ok(None);
        };
        let data = row.get::<Vec<u8>>(0).map_err(backend)?;
        let stored_expiry = row.get::<i64>(1).map_err(backend)?;
        let record: Record = serde_json::from_slice(&data)
            .map_err(|error| session_store::Error::Decode(error.to_string()))?;
        if record.id != *session_id || record.expiry_date.unix_timestamp() != stored_expiry {
            return Err(session_store::Error::Decode(
                "session record metadata does not match its database row".into(),
            ));
        }
        if record.expiry_date <= OffsetDateTime::now_utc() {
            return Ok(None);
        }
        Ok(Some(record))
    }

    async fn delete(&self, session_id: &Id) -> session_store::Result<()> {
        self.context
            .connection()
            .await
            .map_err(backend)?
            .execute(
                "DELETE FROM app_session WHERE id = ?1",
                params![session_id.to_string()],
            )
            .await
            .map_err(backend)?;
        Ok(())
    }
}

fn encode(record: &Record) -> session_store::Result<Vec<u8>> {
    serde_json::to_vec(record).map_err(|error| session_store::Error::Encode(error.to_string()))
}

fn backend(error: impl fmt::Display) -> session_store::Error {
    session_store::Error::Backend(error.to_string())
}
