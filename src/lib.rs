pub mod config;
pub mod db;
pub mod filters;
pub mod models;
pub mod monarch;
pub mod routes;
pub mod templates;
pub mod tmo;

use tokio::sync::Mutex;

pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub sync_status: Mutex<Option<models::SyncStatus>>,
}
