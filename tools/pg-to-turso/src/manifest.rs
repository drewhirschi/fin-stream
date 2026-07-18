use serde::{Deserialize, Serialize};

use crate::model::SequenceState;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    InventoryReady,
    Blocked,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationKind {
    Table,
    PartitionedTable,
    View,
    MaterializedView,
    Sequence,
    ForeignTable,
    Unknown,
}

impl RelationKind {
    pub fn from_pg_relkind(value: &str) -> Self {
        match value {
            "r" => Self::Table,
            "p" => Self::PartitionedTable,
            "v" => Self::View,
            "m" => Self::MaterializedView,
            "S" => Self::Sequence,
            "f" => Self::ForeignTable,
            _ => Self::Unknown,
        }
    }

    pub fn has_rows(&self) -> bool {
        matches!(
            self,
            Self::Table | Self::PartitionedTable | Self::View | Self::MaterializedView
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationClassification {
    Mapped,
    Transformed,
    IntentionallyDiscarded,
    TargetOnly,
    SequenceMetadata,
    BlockedPendingTargetSchema,
    ForbiddenLegacyRemnant,
    Unclassified,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ColumnInventory {
    pub name: String,
    pub pg_type: String,
    pub nullable: bool,
    pub ordinal: i32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RelationInventory {
    pub schema: String,
    pub name: String,
    pub kind: RelationKind,
    pub classification: RelationClassification,
    pub reason: String,
    pub owned_by: Option<String>,
    pub source_count: Option<i64>,
    pub columns: Vec<ColumnInventory>,
    pub source_stats: Option<TableStats>,
    pub destination_stats: Option<TableStats>,
}

impl RelationInventory {
    pub fn qualified_name(&self) -> String {
        format!("{}.{}", self.schema, self.name)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct FinancialStats {
    pub field: String,
    pub non_null_count: u64,
    /// BLAKE3 over the ordered IEEE-754 bit patterns, not display decimals.
    pub bits_blake3: String,
    /// Deterministic f64 accumulation rendered losslessly for business review.
    pub sum: String,
    pub sum_bits: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TableStats {
    pub row_count: u64,
    pub key_min: Option<String>,
    pub key_max: Option<String>,
    pub canonical_rows_blake3: String,
    pub financial: Vec<FinancialStats>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MigrationRecord {
    pub version: i64,
    pub name: String,
    pub blake3: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TargetOnlyValidation {
    pub table: String,
    pub classification: RelationClassification,
    pub expected: String,
    pub observed: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoginCanaryValidation {
    pub expected: String,
    pub source_password_verified: bool,
    pub artifact_password_verified: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialCanaryValidation {
    pub source_tmo_count: u64,
    pub source_monarch_count: u64,
    pub artifact_tmo_count: u64,
    pub artifact_monarch_count: u64,
    pub source_verified: bool,
    pub artifact_verified: bool,
    pub key_fingerprint: Option<String>,
    pub legacy_key_fingerprint: Option<String>,
    pub rewrapped_tmo_count: u64,
    pub rewrapped_monarch_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ValidationManifest {
    pub format_version: u32,
    pub tool_version: String,
    pub status: RunStatus,
    /// Hash of PostgreSQL's snapshot identifier. The identifier itself is omitted.
    pub source_snapshot_blake3: String,
    pub source_isolation: String,
    pub source_read_only: bool,
    pub migrations: Vec<MigrationRecord>,
    pub relations: Vec<RelationInventory>,
    pub sequences: Vec<SequenceState>,
    pub target_only: Vec<TargetOnlyValidation>,
    /// Records only cutover assertions. The email, password, and hash are
    /// deliberately never serialized.
    pub login_canary: Option<LoginCanaryValidation>,
    /// Counts, booleans, and an approved non-reversible key fingerprint only.
    pub credential_canary: Option<CredentialCanaryValidation>,
    pub blockers: Vec<String>,
    pub sqlite_integrity_check: Option<String>,
    pub sqlite_foreign_key_violations: Option<u64>,
    pub artifact_blake3: Option<String>,
}

impl ValidationManifest {
    pub fn new(snapshot: &str, relations: Vec<RelationInventory>) -> Self {
        Self {
            format_version: 1,
            tool_version: env!("CARGO_PKG_VERSION").to_owned(),
            status: RunStatus::InventoryReady,
            source_snapshot_blake3: blake3::hash(snapshot.as_bytes()).to_hex().to_string(),
            source_isolation: "repeatable read".to_owned(),
            source_read_only: true,
            migrations: crate::target_migrations(),
            relations,
            sequences: Vec::new(),
            target_only: vec![
                TargetOnlyValidation {
                    table: "_schema_migrations".to_owned(),
                    classification: RelationClassification::TargetOnly,
                    expected: "exactly the rows installed by checked-in target migrations"
                        .to_owned(),
                    observed: None,
                },
                TargetOnlyValidation {
                    table: "app_session".to_owned(),
                    classification: RelationClassification::TargetOnly,
                    expected: "empty; PostgreSQL sessions are intentionally discarded".to_owned(),
                    observed: None,
                },
                TargetOnlyValidation {
                    table: "intg_received_email_processing_lease".to_owned(),
                    classification: RelationClassification::TargetOnly,
                    expected: "empty; transient webhook processing claims are never imported"
                        .to_owned(),
                    observed: None,
                },
                TargetOnlyValidation {
                    table: "operation_control".to_owned(),
                    classification: RelationClassification::TargetOnly,
                    expected: "exactly id=1, mode=read_only, scheduler_enabled=0".to_owned(),
                    observed: None,
                },
            ],
            login_canary: None,
            credential_canary: None,
            blockers: Vec::new(),
            sqlite_integrity_check: None,
            sqlite_foreign_key_violations: None,
            artifact_blake3: None,
        }
    }
}
