use std::{fmt, str::FromStr};

use serde::Serialize;

use super::OperationError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationMode {
    ReadOnly,
    Enabled,
}

impl OperationMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Enabled => "enabled",
        }
    }
}

impl fmt::Display for OperationMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for OperationMode {
    type Err = OperationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "read_only" => Ok(Self::ReadOnly),
            "enabled" => Ok(Self::Enabled),
            _ => Err(OperationError::coordination(format!(
                "database contains unsupported operation mode {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OperationControl {
    pub mode: OperationMode,
    pub scheduler_enabled: bool,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncRunStatus {
    Running,
    Success,
    Error,
}

impl SyncRunStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

impl FromStr for SyncRunStatus {
    type Err = OperationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "running" => Ok(Self::Running),
            "success" => Ok(Self::Success),
            "error" => Ok(Self::Error),
            _ => Err(OperationError::coordination(format!(
                "database contains unsupported sync status {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SyncRun {
    pub id: i64,
    pub connection_slug: String,
    pub scheduled_for: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub status: SyncRunStatus,
    pub error_message: Option<String>,
    pub endpoints_hit: Option<String>,
    pub events_upserted: i64,
    pub loans_upserted: i64,
    pub snapshots_created: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimOutcome {
    Claimed(SyncRun),
    AlreadyRunning(SyncRun),
    AlreadyScheduled(SyncRun),
    CoveredBySuccess(SyncRun),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyncCompletion {
    pub endpoints_hit: Option<String>,
    pub events_upserted: i64,
    pub loans_upserted: i64,
    pub snapshots_created: i64,
}
