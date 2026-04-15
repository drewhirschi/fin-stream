pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod filters;
pub mod media_storage;
pub mod models;
pub mod monarch;
pub mod property_media;
pub mod resend;
pub mod routes;
pub mod scheduler;
pub mod templates;
pub mod tmo;

use tokio::sync::Mutex;

pub struct AppState {
    pub db: sqlx::PgPool,
    pub sync_status: Mutex<Option<models::SyncStatus>>,
}
