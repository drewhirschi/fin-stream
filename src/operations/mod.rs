mod error;
mod models;
mod repository;

pub use error::{OperationError, OperationResult};
pub use models::{
    ClaimOutcome, OperationControl, OperationMode, SyncCompletion, SyncRun, SyncRunStatus,
};
pub use repository::{MAX_SCHEDULED_ATTEMPTS, OperationRepository};

/// Canonical timestamp for durable operation-control transitions.
pub fn utc_now_millis() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
        now.millisecond(),
    )
}

#[cfg(all(test, feature = "local-db"))]
mod tests;
