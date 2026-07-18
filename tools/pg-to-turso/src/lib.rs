mod blocker;
mod convert;
mod credential_crypto;
pub mod manifest;
pub mod model;
mod schedule_transform;
mod source;
mod sqlite;
mod stats;

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use blocker::ManifestSafeBlocker;
use convert::verify_login_password;
use credential_crypto::CredentialCipher;
use manifest::{
    CredentialCanaryValidation, LoginCanaryValidation, MigrationRecord, RelationClassification,
    RunStatus, ValidationManifest,
};
use model::Dataset;
use source::{SourceSnapshot, inventory_blockers};
use stats::dataset_stats;
use zeroize::Zeroizing;

pub const MIGRATION_1: &str = include_str!("../../../migrations/0001_auth.sql");
pub const MIGRATION_2: &str = include_str!("../../../migrations/0002_streams_forecast.sql");
pub const MIGRATION_3: &str = include_str!("../../../migrations/0003_integrations_operations.sql");
pub const MIGRATION_4: &str = include_str!("../../../migrations/0004_workspaces_inbox.sql");
pub const MIGRATION_5: &str = include_str!("../../../migrations/0005_resend_inbound_leases.sql");

pub struct InventoryRequest {
    pub source_url: String,
    pub manifest_path: PathBuf,
}

pub struct ExportRequest {
    pub source_url: String,
    pub output_path: PathBuf,
    pub manifest_path: PathBuf,
    pub login_canary: LoginCanary,
    pub credential_key: Option<CredentialKey>,
    pub legacy_credential_key: Option<CredentialKey>,
}

/// A known-good cutover credential supplied from environment variables. This
/// type is deliberately not Debug or Serialize so its contents cannot drift
/// into diagnostics or the validation manifest.
pub struct LoginCanary {
    pub email: String,
    pub password: String,
}

/// Trimmed, zeroizing key material. It deliberately implements neither Debug
/// nor Serialize.
pub struct CredentialKey(Zeroizing<String>);

impl CredentialKey {
    pub fn new(value: String) -> Option<Self> {
        let value = Zeroizing::new(value);
        let trimmed = value.trim();
        (!trimmed.is_empty()).then(|| Self(Zeroizing::new(trimmed.to_owned())))
    }

    fn expose(&self) -> &str {
        self.0.as_str()
    }
}

pub async fn inventory(request: InventoryRequest) -> Result<ValidationManifest> {
    validate_private_output_path(&request.manifest_path, "manifest")?;
    let mut snapshot = SourceSnapshot::open(&request.source_url).await?;
    let relations = snapshot.inventory().await?;
    let mut manifest = ValidationManifest::new(&snapshot.snapshot_id, relations);
    manifest.blockers = inventory_blockers(&manifest.relations);
    if !manifest.blockers.is_empty() {
        manifest.status = RunStatus::Blocked;
    }
    snapshot.rollback().await?;
    write_manifest(&request.manifest_path, &manifest)?;
    Ok(manifest)
}

pub async fn export(request: ExportRequest) -> Result<ValidationManifest> {
    validate_private_output_path(&request.output_path, "SQLite artifact")?;
    validate_private_output_path(&request.manifest_path, "manifest")?;
    ensure!(
        request.output_path != request.manifest_path,
        "artifact and manifest paths must differ"
    );

    let mut snapshot = SourceSnapshot::open(&request.source_url).await?;
    let relations = snapshot.inventory().await?;
    let mut manifest = ValidationManifest::new(&snapshot.snapshot_id, relations);
    manifest.blockers = inventory_blockers(&manifest.relations);
    if !manifest.blockers.is_empty() {
        manifest.status = RunStatus::Blocked;
        snapshot.rollback().await?;
        write_manifest(&request.manifest_path, &manifest)?;
        bail!(
            "source inventory has {} cutover blocker(s); no SQLite artifact was created",
            manifest.blockers.len()
        );
    }
    manifest.login_canary = Some(LoginCanaryValidation {
        expected: "one environment-only credential verifies against an active source user and the SQLite artifact readback".to_owned(),
        source_password_verified: false,
        artifact_password_verified: false,
    });
    manifest.credential_canary = Some(CredentialCanaryValidation {
        source_tmo_count: 0,
        source_monarch_count: 0,
        artifact_tmo_count: 0,
        artifact_monarch_count: 0,
        source_verified: false,
        artifact_verified: false,
        key_fingerprint: None,
        legacy_key_fingerprint: None,
        rewrapped_tmo_count: 0,
        rewrapped_monarch_count: 0,
    });
    let mut dataset = match snapshot.read_dataset().await {
        Ok(dataset) => dataset,
        Err(error) => {
            let blocker = manifest_blocker(
                &error,
                "mapped row conversion failed; details were intentionally omitted from the manifest",
            );
            abort_export(
                snapshot,
                &mut manifest,
                &request.manifest_path,
                None,
                &blocker,
            )
            .await?;
            return Err(error).context("typed source export failed");
        }
    };
    let rewrap = match rewrap_legacy_credentials(
        &mut dataset,
        request.credential_key.as_ref(),
        request.legacy_credential_key.as_ref(),
    ) {
        Ok(rewrap) => rewrap,
        Err(error) => {
            let blocker = manifest_blocker(&error, "source credential rewrap failed");
            abort_export(
                snapshot,
                &mut manifest,
                &request.manifest_path,
                None,
                &blocker,
            )
            .await?;
            return Err(error).context("source credential rewrap failed");
        }
    };
    {
        let validation = manifest
            .credential_canary
            .as_mut()
            .expect("credential canary manifest entry exists");
        validation.rewrapped_tmo_count = rewrap.tmo_count;
        validation.rewrapped_monarch_count = rewrap.monarch_count;
        validation.legacy_key_fingerprint = rewrap.legacy_key_fingerprint;
    }
    if let Err(error) = validate_login_canary(&dataset, &request.login_canary) {
        let blocker = manifest_blocker(&error, "source login canary failed");
        abort_export(
            snapshot,
            &mut manifest,
            &request.manifest_path,
            None,
            &blocker,
        )
        .await?;
        return Err(error).context("source login canary failed");
    }
    manifest
        .login_canary
        .as_mut()
        .expect("login canary manifest entry exists")
        .source_password_verified = true;
    let source_credentials =
        match validate_credential_canary(&dataset, request.credential_key.as_ref()) {
            Ok(validation) => validation,
            Err(error) => {
                let blocker = manifest_blocker(&error, "source credential canary failed");
                abort_export(
                    snapshot,
                    &mut manifest,
                    &request.manifest_path,
                    None,
                    &blocker,
                )
                .await?;
                return Err(error).context("source credential canary failed");
            }
        };
    {
        let validation = manifest
            .credential_canary
            .as_mut()
            .expect("credential canary manifest entry exists");
        validation.source_tmo_count = source_credentials.tmo_count;
        validation.source_monarch_count = source_credentials.monarch_count;
        validation.source_verified = true;
        validation.key_fingerprint = source_credentials.key_fingerprint.clone();
    }
    let source_stats = dataset_stats(&dataset);
    if let Err(error) = attach_source_stats(&mut manifest, &source_stats) {
        abort_export(
            snapshot,
            &mut manifest,
            &request.manifest_path,
            None,
            "source count/stat validation failed",
        )
        .await?;
        return Err(error);
    }
    let sequences = match snapshot
        .read_sequence_states(&manifest.relations, &dataset)
        .await
    {
        Ok(sequences) => sequences,
        Err(error) => {
            abort_export(
                snapshot,
                &mut manifest,
                &request.manifest_path,
                None,
                "source sequence validation failed",
            )
            .await?;
            return Err(error).context("source sequence export failed");
        }
    };
    manifest.sequences = sequences.clone();

    let artifact = match sqlite::build_artifact(&request.output_path, &dataset, &sequences) {
        Ok(artifact) => artifact,
        Err(error) => {
            abort_export(
                snapshot,
                &mut manifest,
                &request.manifest_path,
                None,
                "target artifact construction or validation failed",
            )
            .await?;
            return Err(error).context("SQLite artifact validation failed");
        }
    };
    if let Err(error) = validate_login_canary(&artifact.destination, &request.login_canary) {
        let blocker = manifest_blocker(&error, "artifact login canary failed");
        abort_export(
            snapshot,
            &mut manifest,
            &request.manifest_path,
            Some(&request.output_path),
            &blocker,
        )
        .await?;
        return Err(error).context("artifact login canary failed");
    }
    manifest
        .login_canary
        .as_mut()
        .expect("login canary manifest entry exists")
        .artifact_password_verified = true;
    let artifact_credentials =
        match validate_credential_canary(&artifact.destination, request.credential_key.as_ref()) {
            Ok(validation) => validation,
            Err(error) => {
                let blocker = manifest_blocker(&error, "artifact credential canary failed");
                abort_export(
                    snapshot,
                    &mut manifest,
                    &request.manifest_path,
                    Some(&request.output_path),
                    &blocker,
                )
                .await?;
                return Err(error).context("artifact credential canary failed");
            }
        };
    if source_credentials != artifact_credentials {
        abort_export(
            snapshot,
            &mut manifest,
            &request.manifest_path,
            Some(&request.output_path),
            "source and artifact credential-canary counts/fingerprint differ",
        )
        .await?;
        bail!("source and artifact credential canaries differ");
    }
    {
        let validation = manifest
            .credential_canary
            .as_mut()
            .expect("credential canary manifest entry exists");
        validation.artifact_tmo_count = artifact_credentials.tmo_count;
        validation.artifact_monarch_count = artifact_credentials.monarch_count;
        validation.artifact_verified = true;
    }
    let destination_stats = dataset_stats(&artifact.destination);
    if let Err(error) = attach_destination_stats(&mut manifest, &destination_stats) {
        abort_export(
            snapshot,
            &mut manifest,
            &request.manifest_path,
            Some(&request.output_path),
            "destination stat attachment failed",
        )
        .await?;
        return Err(error);
    }
    manifest.sqlite_integrity_check = Some(artifact.integrity_check);
    manifest.sqlite_foreign_key_violations = Some(artifact.foreign_key_violations);
    manifest.artifact_blake3 = Some(artifact.artifact_blake3);
    for (table, observed) in artifact.target_only {
        if let Some(validation) = manifest
            .target_only
            .iter_mut()
            .find(|validation| validation.table == table)
        {
            validation.observed = Some(observed);
        }
    }
    manifest.status = RunStatus::Complete;

    if let Err(error) = snapshot.rollback().await {
        remove_incomplete_output(&request.output_path);
        return Err(error);
    }
    if let Err(error) = write_manifest(&request.manifest_path, &manifest) {
        remove_incomplete_output(&request.output_path);
        return Err(error);
    }
    Ok(manifest)
}

fn validate_login_canary(dataset: &Dataset, canary: &LoginCanary) -> Result<()> {
    let email = canary.email.trim().to_lowercase();
    if email.is_empty() || canary.password.is_empty() {
        return Err(ManifestSafeBlocker::new(
            "the environment-only login canary email or password is empty",
        )
        .into());
    }
    let user = dataset
        .app_users
        .iter()
        .find(|user| user.is_active == 1 && user.email == email)
        .ok_or_else(|| {
            ManifestSafeBlocker::new(
                "the environment-only login canary does not identify an active source user",
            )
        })?;
    if !verify_login_password(
        &canary.password,
        &user.password_hash,
        "active app_user.password_hash",
    )? {
        return Err(ManifestSafeBlocker::new(
            "the environment-only login canary password does not verify",
        )
        .into());
    }
    Ok(())
}

#[derive(Debug, Default)]
struct CredentialRewrapResult {
    tmo_count: u64,
    monarch_count: u64,
    legacy_key_fingerprint: Option<String>,
}

fn rewrap_legacy_credentials(
    dataset: &mut Dataset,
    current_key: Option<&CredentialKey>,
    legacy_key: Option<&CredentialKey>,
) -> Result<CredentialRewrapResult> {
    if dataset.tmo_credentials.is_empty() && dataset.monarch_credentials.is_empty() {
        return Ok(CredentialRewrapResult::default());
    }
    let Some(current_key) = current_key else {
        return Ok(CredentialRewrapResult::default());
    };
    let current = CredentialCipher::new(current_key.expose()).map_err(|_| {
        ManifestSafeBlocker::new("APP_ENCRYPTION_KEY is empty or invalid for credential rewrap")
    })?;
    let legacy = legacy_key
        .map(|key| CredentialCipher::new(key.expose()))
        .transpose()
        .map_err(|_| {
            ManifestSafeBlocker::new(
                "LEGACY_APP_ENCRYPTION_KEY is empty or invalid for credential rewrap",
            )
        })?;
    let mut result = CredentialRewrapResult::default();

    for row in &mut dataset.tmo_credentials {
        if current
            .decrypt_canary(&row.pin_ciphertext, &row.pin_nonce, row.key_version)
            .is_ok()
        {
            continue;
        }
        let legacy = legacy.as_ref().ok_or_else(|| {
            ManifestSafeBlocker::new(
                "a TMO credential needs LEGACY_APP_ENCRYPTION_KEY before it can be rewrapped",
            )
        })?;
        let plaintext = legacy
            .decrypt_parts(&row.pin_ciphertext, &row.pin_nonce, row.key_version)
            .map_err(|_| {
                ManifestSafeBlocker::new(
                    "a TMO credential cannot be decrypted with either supplied encryption key",
                )
            })?;
        let (ciphertext, nonce) = current.encrypt_parts(&plaintext).map_err(|_| {
            ManifestSafeBlocker::new("a TMO credential could not be rewrapped safely")
        })?;
        drop(plaintext);
        row.pin_ciphertext = ciphertext;
        row.pin_nonce = nonce;
        result.tmo_count += 1;
    }

    for row in &mut dataset.monarch_credentials {
        if current
            .decrypt_canary(
                &row.access_token_ciphertext,
                &row.access_token_nonce,
                row.key_version,
            )
            .is_ok()
        {
            continue;
        }
        let legacy = legacy.as_ref().ok_or_else(|| {
            ManifestSafeBlocker::new(
                "a Monarch credential needs LEGACY_APP_ENCRYPTION_KEY before it can be rewrapped",
            )
        })?;
        let plaintext = legacy
            .decrypt_parts(
                &row.access_token_ciphertext,
                &row.access_token_nonce,
                row.key_version,
            )
            .map_err(|_| {
                ManifestSafeBlocker::new(
                    "a Monarch credential cannot be decrypted with either supplied encryption key",
                )
            })?;
        let (ciphertext, nonce) = current.encrypt_parts(&plaintext).map_err(|_| {
            ManifestSafeBlocker::new("a Monarch credential could not be rewrapped safely")
        })?;
        drop(plaintext);
        row.access_token_ciphertext = ciphertext;
        row.access_token_nonce = nonce;
        result.monarch_count += 1;
    }

    if result.tmo_count > 0 || result.monarch_count > 0 {
        result.legacy_key_fingerprint = legacy.map(|cipher| cipher.key_fingerprint());
    }
    Ok(result)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CredentialCanaryResult {
    tmo_count: u64,
    monarch_count: u64,
    key_fingerprint: Option<String>,
}

fn validate_credential_canary(
    dataset: &Dataset,
    key: Option<&CredentialKey>,
) -> Result<CredentialCanaryResult> {
    validate_active_provider_credentials(dataset)?;
    let tmo_count = dataset.tmo_credentials.len() as u64;
    let monarch_count = dataset.monarch_credentials.len() as u64;
    if tmo_count == 0 && monarch_count == 0 {
        return Ok(CredentialCanaryResult {
            tmo_count,
            monarch_count,
            key_fingerprint: None,
        });
    }
    let key = key.ok_or_else(|| {
        ManifestSafeBlocker::new(
            "APP_ENCRYPTION_KEY is required because provider credential rows exist",
        )
    })?;
    let cipher = CredentialCipher::new(key.expose()).map_err(|_| {
        ManifestSafeBlocker::new("APP_ENCRYPTION_KEY is empty or invalid for credential canaries")
    })?;
    for row in &dataset.tmo_credentials {
        cipher
            .decrypt_canary(&row.pin_ciphertext, &row.pin_nonce, row.key_version)
            .map_err(|_| {
                ManifestSafeBlocker::new(
                    "a TMO credential cannot be decrypted with APP_ENCRYPTION_KEY",
                )
            })?;
    }
    for row in &dataset.monarch_credentials {
        cipher
            .decrypt_canary(
                &row.access_token_ciphertext,
                &row.access_token_nonce,
                row.key_version,
            )
            .map_err(|_| {
                ManifestSafeBlocker::new(
                    "a Monarch credential cannot be decrypted with APP_ENCRYPTION_KEY",
                )
            })?;
    }
    Ok(CredentialCanaryResult {
        tmo_count,
        monarch_count,
        key_fingerprint: Some(cipher.key_fingerprint()),
    })
}

fn validate_active_provider_credentials(dataset: &Dataset) -> Result<()> {
    for connection in &dataset.integration_connections {
        if connection.status != "active" {
            continue;
        }
        match connection.provider.as_str() {
            "mortgage_office" => {
                if connection.slug != "tmo"
                    || !dataset
                        .tmo_credentials
                        .iter()
                        .any(|credential| credential.connection_id == connection.id)
                {
                    return Err(ManifestSafeBlocker::new(
                        "an active TMO provider connection is misidentified or missing its required credential",
                    )
                    .into());
                }
            }
            "monarch" => {
                if connection.slug != "monarch"
                    || !dataset
                        .monarch_credentials
                        .iter()
                        .any(|credential| credential.connection_id == connection.id)
                {
                    return Err(ManifestSafeBlocker::new(
                        "an active Monarch provider connection is misidentified or missing its required credential",
                    )
                    .into());
                }
            }
            _ if connection
                .metadata
                .as_deref()
                .and_then(|metadata| serde_json::from_str::<serde_json::Value>(metadata).ok())
                .and_then(|metadata| {
                    metadata.get("syncable").and_then(|value| value.as_bool())
                })
                == Some(false) => {}
            _ => {
                return Err(ManifestSafeBlocker::new(
                    "an active provider connection has no supported credential contract and is not explicitly marked metadata.syncable=false",
                )
                .into());
            }
        }
    }
    for credential in &dataset.tmo_credentials {
        let matches_provider = dataset.integration_connections.iter().any(|connection| {
            connection.id == credential.connection_id
                && connection.provider == "mortgage_office"
                && connection.slug == "tmo"
        });
        ensure!(
            matches_provider,
            "TMO credential belongs to a non-TMO connection"
        );
    }
    for credential in &dataset.monarch_credentials {
        let matches_provider = dataset.integration_connections.iter().any(|connection| {
            connection.id == credential.connection_id
                && connection.provider == "monarch"
                && connection.slug == "monarch"
        });
        ensure!(
            matches_provider,
            "Monarch credential belongs to a non-Monarch connection"
        );
    }
    Ok(())
}

fn manifest_blocker(error: &anyhow::Error, fallback: &str) -> String {
    error
        .downcast_ref::<ManifestSafeBlocker>()
        .map(|blocker| blocker.message().to_owned())
        .unwrap_or_else(|| fallback.to_owned())
}

async fn abort_export(
    snapshot: SourceSnapshot,
    manifest: &mut ValidationManifest,
    manifest_path: &Path,
    output_path: Option<&Path>,
    blocker: &str,
) -> Result<()> {
    if let Some(output_path) = output_path {
        remove_incomplete_output(output_path);
    }
    manifest.status = RunStatus::Blocked;
    manifest.blockers.push(blocker.to_owned());
    snapshot.rollback().await?;
    write_manifest(manifest_path, manifest)?;
    Ok(())
}

pub fn target_migrations() -> Vec<MigrationRecord> {
    vec![
        MigrationRecord {
            version: 1,
            name: "auth".to_owned(),
            blake3: blake3::hash(MIGRATION_1.as_bytes()).to_hex().to_string(),
        },
        MigrationRecord {
            version: 2,
            name: "streams_forecast".to_owned(),
            blake3: blake3::hash(MIGRATION_2.as_bytes()).to_hex().to_string(),
        },
        MigrationRecord {
            version: 3,
            name: "integrations_operations".to_owned(),
            blake3: blake3::hash(MIGRATION_3.as_bytes()).to_hex().to_string(),
        },
        MigrationRecord {
            version: 4,
            name: "workspaces_inbox".to_owned(),
            blake3: blake3::hash(MIGRATION_4.as_bytes()).to_hex().to_string(),
        },
        MigrationRecord {
            version: 5,
            name: "resend_inbound_leases".to_owned(),
            blake3: blake3::hash(MIGRATION_5.as_bytes()).to_hex().to_string(),
        },
    ]
}

fn attach_source_stats(
    manifest: &mut ValidationManifest,
    stats: &std::collections::BTreeMap<&str, manifest::TableStats>,
) -> Result<()> {
    for relation in &mut manifest.relations {
        if relation.classification == RelationClassification::Transformed {
            let table_stats = stats.get(relation.name.as_str()).with_context(|| {
                format!("missing source stats for {}", relation.qualified_name())
            })?;
            ensure!(
                relation.source_count == Some(table_stats.row_count as i64),
                "{} inventory/data counts differ inside one snapshot",
                relation.qualified_name()
            );
            relation.source_stats = Some((*table_stats).clone());
        }
    }
    Ok(())
}

fn attach_destination_stats(
    manifest: &mut ValidationManifest,
    stats: &std::collections::BTreeMap<&str, manifest::TableStats>,
) -> Result<()> {
    for relation in &mut manifest.relations {
        if relation.classification == RelationClassification::Transformed {
            relation.destination_stats = Some(
                stats
                    .get(relation.name.as_str())
                    .with_context(|| {
                        format!(
                            "missing destination stats for {}",
                            relation.qualified_name()
                        )
                    })?
                    .clone(),
            );
        }
    }
    Ok(())
}

fn validate_private_output_path(path: &Path, label: &str) -> Result<()> {
    ensure!(path.is_absolute(), "{label} path must be absolute");
    ensure!(!path.exists(), "refusing to overwrite existing {label}");
    ensure!(path.file_name().is_some(), "{label} path has no file name");
    let parent = path.parent().context("output path has no parent")?;
    let canonical_parent = parent
        .canonicalize()
        .with_context(|| format!("{label} parent directory does not exist"))?;
    ensure!(
        canonical_parent.is_dir(),
        "{label} parent is not a directory"
    );

    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .context("could not determine repository root")?
        .canonicalize()?;
    ensure!(
        !canonical_parent.starts_with(&repository),
        "{label} must be created outside the repository"
    );
    Ok(())
}

fn write_manifest(path: &Path, manifest: &ValidationManifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest)?;
    let parent = path.parent().context("manifest path has no parent")?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".trust-deeds-manifest-")
        .suffix(".json.partial")
        .tempfile_in(parent)?;
    set_private_permissions(temporary.path())?;
    temporary.write_all(&bytes)?;
    temporary.write_all(b"\n")?;
    temporary.as_file_mut().sync_all()?;
    let persisted = temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)
        .context("could not atomically publish validation manifest")?;
    persisted.sync_all()?;
    set_private_permissions(path)?;
    sync_parent_directory(parent)?;
    Ok(())
}

fn remove_incomplete_output(path: &Path) {
    let _ = fs::remove_file(path);
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[cfg(unix)]
fn set_private_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let mode = fs::metadata(path)?.permissions().mode() & 0o777;
    ensure!(
        mode == 0o600,
        "private output mode is {mode:o}, expected 600"
    );
    Ok(())
}

#[cfg(not(unix))]
fn set_private_permissions(_path: &Path) -> Result<()> {
    bail!("the exporter currently requires Unix mode-0600 file semantics")
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use argon2::password_hash::{PasswordHasher, SaltString};

    use super::*;
    use crate::model::{
        AccountRow, AppUserRow, Dataset, IntegrationConnectionRow, LoanWorkspacePhotoRow,
        LoanWorkspaceRow, MonarchCredentialRow, PortfolioSnapshotRow, ReceivedEmailAttachmentRow,
        ReceivedEmailRow, SequenceState, SettingRow, StreamEventRow, StreamRow, StreamScheduleRow,
        StreamViewRow, StreamViewStreamRow, SyncLogRow, TmoAccountRow, TmoCredentialRow,
        TmoImportLoanRow, TmoImportOverviewRow, TmoImportPaymentRow, TmoPaymentEventLinkRow,
    };
    use crate::schedule_transform::{LegacyEventRow, transform_event};

    const FIXTURE_PASSWORD: &str = "cutover password";

    fn fixture_password_hash() -> String {
        let salt = SaltString::encode_b64(b"0123456789abcdef").unwrap();
        argon2::Argon2::default()
            .hash_password(FIXTURE_PASSWORD.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    fn fixture() -> Dataset {
        let timestamp = "2025-01-02T03:04:05.000Z".to_owned();
        Dataset {
            app_users: vec![AppUserRow {
                id: 1,
                email: "admin@example.com".into(),
                password_hash: fixture_password_hash(),
                display_name: Some("Admin".into()),
                is_active: 1,
                created_at: 1_735_786_800,
                updated_at: 1_735_786_800,
            }],
            accounts: vec![AccountRow {
                id: 2,
                name: "Primary Cash".into(),
                kind: "cash".into(),
                balance: Some(12_345.67),
                balance_as_of_date: Some("2025-01-02".into()),
                source_type: Some("manual".into()),
                source_ref: None,
                metadata: Some("{\"fixture\":true}".into()),
                balance_updated_at: Some(timestamp.clone()),
                is_primary: 1,
                is_active: 1,
                notes: None,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            streams: vec![
                StreamRow {
                    id: 3,
                    name: "Salary".into(),
                    stream_type: "manual_income".into(),
                    kind: "manual_income".into(),
                    direction: "in".into(),
                    amount_certainty: "known".into(),
                    description: None,
                    default_account_id: Some(2),
                    configuration: Some("{}".into()),
                    // Deliberately references a larger ID to exercise the
                    // two-phase self-FK load.
                    parent_id: Some(10),
                    is_active: 1,
                    created_at: timestamp.clone(),
                    updated_at: timestamp.clone(),
                },
                StreamRow {
                    id: 10,
                    name: "Income".into(),
                    stream_type: "group".into(),
                    kind: "manual_income".into(),
                    direction: "in".into(),
                    amount_certainty: "known".into(),
                    description: None,
                    default_account_id: Some(2),
                    configuration: None,
                    parent_id: None,
                    is_active: 1,
                    created_at: timestamp.clone(),
                    updated_at: timestamp.clone(),
                },
            ],
            stream_views: vec![StreamViewRow {
                id: 4,
                name: "Everything".into(),
                description: None,
                is_default: 1,
                is_active: 1,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            stream_view_streams: vec![StreamViewStreamRow {
                stream_view_id: 4,
                stream_id: 3,
                created_at: timestamp.clone(),
            }],
            stream_schedules: vec![StreamScheduleRow {
                id: 5,
                stream_id: 3,
                account_id: Some(2),
                label: Some("Payday".into()),
                amount: 2_000.0,
                frequency: "monthly".into(),
                day_of_month: Some(15),
                start_date: "2025-01-15".into(),
                end_date: None,
                is_active: 1,
                metadata: Some("{}".into()),
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            stream_events: vec![StreamEventRow {
                id: 6,
                stream_id: 3,
                account_id: Some(2),
                label: Some("January payday".into()),
                expected_date: "2025-01-15".into(),
                amount: 2_000.0,
                override_label: None,
                has_label_override: 0,
                override_date: None,
                override_amount: None,
                override_account_id: None,
                has_account_override: 0,
                actual_date: Some("2025-01-15".into()),
                actual_amount: Some(2_000.0),
                status: "received".into(),
                is_excluded: 0,
                exclusion_reason: None,
                source_id: Some("fixture:1".into()),
                source_type: Some("manual".into()),
                metadata: Some("{\"fixture\":true}".into()),
                notes: None,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            integration_connections: vec![
                IntegrationConnectionRow {
                    id: 7,
                    slug: "tmo".into(),
                    name: "The Mortgage Office".into(),
                    provider: "mortgage_office".into(),
                    status: "active".into(),
                    sync_cadence: "manual".into(),
                    last_synced_at: Some(timestamp.clone()),
                    last_error: None,
                    metadata: Some("{\"fixture\":true}".into()),
                    next_scheduled_at: None,
                    created_at: timestamp.clone(),
                    updated_at: timestamp.clone(),
                },
                IntegrationConnectionRow {
                    id: 8,
                    slug: "monarch".into(),
                    name: "Monarch".into(),
                    provider: "monarch".into(),
                    status: "active".into(),
                    sync_cadence: "manual".into(),
                    last_synced_at: None,
                    last_error: None,
                    metadata: None,
                    next_scheduled_at: Some("2025-01-03T03:04:05.000Z".into()),
                    created_at: timestamp.clone(),
                    updated_at: timestamp.clone(),
                },
            ],
            tmo_import_overviews: vec![TmoImportOverviewRow {
                id: 9,
                connection_id: 7,
                snapshot_date: "2025-01-02".into(),
                portfolio_value: Some(500_000.25),
                portfolio_yield: Some(8.125),
                portfolio_count: Some(12),
                ytd_interest: Some(1_234.5),
                ytd_principal: Some(987.25),
                trust_balance: Some(4_321.0),
                outstanding_checks: Some(123.45),
                service_fees: Some(12.34),
                processing_state: "captured".into(),
                raw_payload: Some("{\"overview\":true}".into()),
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            tmo_import_loans: vec![TmoImportLoanRow {
                id: 10,
                connection_id: 7,
                stream_id: Some(3),
                loan_account: "LN-100".into(),
                borrower_name: Some("Fixture Borrower".into()),
                property_address: Some("1 Fixture Way".into()),
                property_city: Some("Denver".into()),
                property_state: Some("CO".into()),
                property_zip: Some("80202".into()),
                property_description: Some("Fixture property".into()),
                property_type: Some("SFR".into()),
                property_priority: Some(1),
                occupancy: Some("Owner".into()),
                appraised_value: Some(400_000.0),
                ltv: Some(65.0),
                percent_owned: Some(25.0),
                priority: Some(1),
                loan_type: Some(2),
                interest_rate: Some(8.25),
                note_rate: Some(8.5),
                original_balance: Some(250_000.0),
                loan_balance: Some(240_000.0),
                principal_balance: Some(239_500.0),
                regular_payment: Some(2_100.0),
                payment_frequency: Some("Monthly".into()),
                maturity_date: Some("2030-01-02".into()),
                next_payment_date: Some("2025-02-02".into()),
                interest_paid_to: Some("2025-01-02".into()),
                billed_through: Some("2025-01-31".into()),
                term_left_months: Some(60),
                is_delinquent: Some(0),
                is_active: Some(1),
                raw_summary_payload: Some("{\"summary\":true}".into()),
                raw_detail_payload: Some("{\"detail\":true}".into()),
                summary_imported_at: Some(timestamp.clone()),
                detail_imported_at: Some(timestamp.clone()),
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            tmo_import_payments: vec![TmoImportPaymentRow {
                id: 11,
                connection_id: 7,
                external_id: "PAY-100".into(),
                loan_account: "LN-100".into(),
                borrower_name: "Fixture Borrower".into(),
                property_name: "1 Fixture Way".into(),
                check_number: Some("1001".into()),
                check_date: "2025-01-02".into(),
                amount: 2_100.0,
                service_fee: 10.0,
                interest: 1_500.0,
                principal: 590.0,
                charges: 0.0,
                late_charges: 0.0,
                other: 0.0,
                processing_state: "normalized".into(),
                normalized_event_source_id: Some("fixture:1".into()),
                raw_payload: Some("{\"payment\":true}".into()),
                imported_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            tmo_accounts: vec![TmoAccountRow {
                id: 1,
                company_id: "vci".into(),
                account_number: "fixture-account".into(),
                source_rec_id: Some("REC-1".into()),
                display_name: Some("Fixture TMO".into()),
                email: Some("tmo@example.com".into()),
                last_login_at: Some(timestamp.clone()),
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            tmo_credentials: vec![TmoCredentialRow {
                connection_id: 7,
                company_id: "vci".into(),
                account_number: "fixture-account".into(),
                pin_ciphertext: "QsRRZq1wtthAKK37nWsnen/0q535QJ3h1SkkOW8EOLerrg==".into(),
                pin_nonce: "AAECAwQFBgcICQoL".into(),
                key_version: 1,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            monarch_credentials: vec![MonarchCredentialRow {
                connection_id: 8,
                access_token_ciphertext: "QsRRZq1wtthAKK37nWsnen/0q535QJ3h1SkkOW8EOLerrg==".into(),
                access_token_nonce: "AAECAwQFBgcICQoL".into(),
                default_account_id: "monarch-account".into(),
                key_version: 1,
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            tmo_payment_event_links: vec![TmoPaymentEventLinkRow {
                tmo_payment_id: 11,
                stream_event_id: 6,
                created_at: timestamp.clone(),
            }],
            portfolio_snapshots: vec![PortfolioSnapshotRow {
                id: 12,
                snapshot_date: "2025-01-02".into(),
                portfolio_value: Some(500_000.25),
                portfolio_yield: Some(8.125),
                portfolio_count: Some(12),
                ytd_interest: Some(1_234.5),
                ytd_principal: Some(987.25),
                trust_balance: Some(4_321.0),
                outstanding_checks: Some(123.45),
                service_fees: Some(12.34),
                synced_at: timestamp.clone(),
            }],
            settings: vec![SettingRow {
                key: "fixture.setting".into(),
                value: "preserved exactly".into(),
                updated_at: timestamp.clone(),
            }],
            sync_logs: vec![SyncLogRow {
                id: 13,
                connection_slug: "tmo".into(),
                scheduled_for: None,
                started_at: timestamp.clone(),
                finished_at: Some("2025-01-02T03:05:05.000Z".into()),
                status: "success".into(),
                error_message: None,
                endpoints_hit: Some("overview,loans,payments".into()),
                events_upserted: 1,
                loans_upserted: 1,
                snapshots_created: 1,
            }],
            loan_workspaces: vec![LoanWorkspaceRow {
                id: 14,
                connection_id: 7,
                loan_account: "LN-100".into(),
                redfin_url: Some("https://www.redfin.com/fixture".into()),
                zillow_url: Some("https://www.zillow.com/fixture".into()),
                decision_status: Some("reviewing".into()),
                target_contribution: Some(50_000.0),
                actual_contribution: Some(49_500.0),
                notes: Some("Fixture workspace".into()),
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            loan_workspace_photos: vec![LoanWorkspacePhotoRow {
                id: 15,
                connection_id: 7,
                loan_account: "LN-100".into(),
                provider: "redfin".into(),
                caption: Some("Front".into()),
                source_url: "https://www.redfin.com/fixture".into(),
                image_url: "https://images.example.com/fixture.jpg".into(),
                sort_order: 0,
                is_featured: 1,
                created_at: timestamp.clone(),
            }],
            received_emails: vec![ReceivedEmailRow {
                id: 16,
                resend_email_id: "resend-email-1".into(),
                from_address: "sender@example.com".into(),
                to_addresses: "[\"inbox@example.com\"]".into(),
                subject: Some("Loan LN-100".into()),
                received_at: timestamp.clone(),
                body_s3_key: Some("email/fixture/body".into()),
                body_content_type: Some("text/plain".into()),
                loan_account: Some("LN-100".into()),
                processing_state: "stored".into(),
                error_message: None,
                raw_webhook_payload: Some("{\"email\":true}".into()),
                created_at: timestamp.clone(),
                updated_at: timestamp.clone(),
            }],
            received_email_attachments: vec![ReceivedEmailAttachmentRow {
                id: 17,
                email_id: 16,
                resend_attachment_id: "resend-attachment-1".into(),
                filename: "statement.pdf".into(),
                content_type: "application/pdf".into(),
                size_bytes: Some(1_024),
                s3_key: Some("email/fixture/statement.pdf".into()),
                processing_state: "stored".into(),
                created_at: timestamp,
            }],
        }
    }

    fn sequences() -> Vec<SequenceState> {
        [
            ("app_user", "public.app_user_id_seq", 1, 10),
            ("account", "public.account_id_seq", 2, 20),
            ("stream", "public.stream_id_seq", 10, 30),
            ("stream_view", "public.stream_view_id_seq", 4, 40),
            ("stream_schedule", "public.stream_schedule_id_seq", 5, 50),
            ("stream_event", "public.stream_event_id_seq", 6, 60),
            (
                "intg_integration_connection",
                "intg.integration_connection_id_seq",
                8,
                80,
            ),
            (
                "intg_tmo_import_overview",
                "intg.tmo_import_overview_id_seq",
                9,
                90,
            ),
            (
                "intg_tmo_import_loan",
                "intg.tmo_import_loan_id_seq",
                10,
                100,
            ),
            (
                "intg_tmo_import_payment",
                "intg.tmo_import_payment_id_seq",
                11,
                110,
            ),
            (
                "portfolio_snapshot",
                "public.portfolio_snapshot_id_seq",
                12,
                120,
            ),
            ("sync_log", "public.sync_log_id_seq", 13, 130),
            ("intg_loan_workspace", "intg.loan_workspace_id_seq", 14, 140),
            (
                "intg_loan_workspace_photo",
                "intg.loan_workspace_photo_id_seq",
                15,
                150,
            ),
            ("intg_received_email", "intg.received_email_id_seq", 16, 160),
            (
                "intg_received_email_attachment",
                "intg.received_email_attachment_id_seq",
                17,
                170,
            ),
        ]
        .into_iter()
        .map(|(table, source_sequence, max, next)| SequenceState {
            table: table.into(),
            source_sequence: source_sequence.into(),
            source_effective_next: next,
            imported_max: Some(max),
            target_effective_next: next,
        })
        .collect()
    }

    #[test]
    fn exact_migrations_load_and_validate_a_deterministic_fixture() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("export.sqlite");
        let result = sqlite::build_artifact(&output, &fixture(), &sequences()).unwrap();
        assert_eq!(result.destination, fixture());
        assert_eq!(result.integrity_check, "ok");
        assert_eq!(result.foreign_key_violations, 0);
        assert!(result.target_only.iter().any(|(table, observed)| {
            table == "intg_received_email_processing_lease" && observed == "0 rows"
        }));
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!PathBuf::from(format!("{}-wal", output.display())).exists());
        assert!(!PathBuf::from(format!("{}-shm", output.display())).exists());
        let connection = rusqlite::Connection::open(&output).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap()
                .to_ascii_lowercase(),
            "wal"
        );
        assert_eq!(
            connection
                .query_row("PRAGMA page_size", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            4096
        );
        assert_eq!(
            connection
                .query_row("PRAGMA auto_vacuum", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row("PRAGMA encoding", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "UTF-8"
        );
    }

    #[test]
    fn transformed_schedule_events_survive_first_target_refresh() {
        let mut dataset = fixture();
        let timestamp = "2025-01-02T03:04:05.000Z".to_owned();
        let legacy_event = |id, occurrence: &str| LegacyEventRow {
            id,
            stream_id: 3,
            account_id: Some(2),
            label: Some("Payday".into()),
            expected_date: occurrence.into(),
            actual_date: None,
            amount: 2_000.0,
            status: "projected".into(),
            source_id: Some(format!("stream_schedule:5:{occurrence}")),
            source_type: Some("stream_schedule".into()),
            metadata: Some("{\"schedule_id\":5}".into()),
            notes: None,
            created_at: timestamp.clone(),
            updated_at: timestamp.clone(),
        };

        let mut moved = legacy_event(6, "2025-01-15");
        moved.expected_date = "2025-01-20".into();
        moved.amount = 2_100.0;
        moved.label = Some("Moved payday".into());
        moved.account_id = None;
        let moved = transform_event(moved, &dataset.streams, &dataset.stream_schedules).unwrap();

        let mut received = legacy_event(7, "2025-02-15");
        received.expected_date = "2025-02-22".into();
        received.actual_date = Some("2025-02-22".into());
        received.amount = 2_200.0;
        received.status = "received".into();
        let received =
            transform_event(received, &dataset.streams, &dataset.stream_schedules).unwrap();
        dataset.stream_events = vec![moved, received];

        let mut sequence_states = sequences();
        let event_sequence = sequence_states
            .iter_mut()
            .find(|state| state.table == "stream_event")
            .unwrap();
        event_sequence.imported_max = Some(7);

        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("schedule-export.sqlite");
        sqlite::build_artifact(&output, &dataset, &sequence_states).unwrap();
        let connection = rusqlite::Connection::open(&output).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON")
            .unwrap();

        let refresh = |source_id: &str, expected_date: &str, status: &str| {
            connection
                .execute(
                    "INSERT INTO stream_event (stream_id, account_id, label, expected_date,
                         amount, status, source_id, source_type, metadata, is_excluded,
                         exclusion_reason)
                     VALUES (3, 2, 'Payday', ?1, 2000.0, ?2, ?3,
                         'stream_schedule', '{\"schedule_id\":5}', 0, NULL)
                     ON CONFLICT(stream_id, source_type, source_id) DO UPDATE SET
                        account_id = excluded.account_id, label = excluded.label,
                        expected_date = excluded.expected_date, amount = excluded.amount,
                        metadata = excluded.metadata,
                        is_excluded = CASE
                            WHEN stream_event.exclusion_reason = 'user' THEN 1 ELSE 0 END,
                        exclusion_reason = CASE
                            WHEN stream_event.exclusion_reason = 'user' THEN 'user' ELSE NULL END
                     WHERE stream_event.status IN ('projected', 'confirmed')",
                    rusqlite::params![expected_date, status, source_id],
                )
                .unwrap()
        };
        assert_eq!(
            refresh(
                "stream_schedule:5:monthly:2025-01",
                "2025-01-10",
                "projected"
            ),
            1
        );
        assert_eq!(
            refresh(
                "stream_schedule:5:monthly:2025-02",
                "2025-02-10",
                "projected"
            ),
            0,
            "received occurrence must be immutable during refresh"
        );

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM stream_event", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2, "stable cadence slots must not duplicate rows");
        type RefreshedEvent = (
            i64,
            String,
            f64,
            Option<String>,
            Option<f64>,
            Option<String>,
            i64,
            Option<i64>,
        );
        let moved: RefreshedEvent = connection
            .query_row(
                "SELECT id, expected_date, amount, override_date, override_amount,
                        override_label, has_account_override, override_account_id
                 FROM stream_event WHERE id = 6",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(moved.0, 6);
        assert_eq!(moved.1, "2025-01-10");
        assert_eq!(moved.2, 2_000.0);
        assert_eq!(moved.3.as_deref(), Some("2025-01-20"));
        assert_eq!(moved.4, Some(2_100.0));
        assert_eq!(moved.5.as_deref(), Some("Moved payday"));
        assert_eq!((moved.6, moved.7), (1, None));

        let received: (String, f64, Option<String>, Option<f64>, Option<f64>) = connection
            .query_row(
                "SELECT expected_date, amount, override_date, override_amount, actual_amount
                 FROM stream_event WHERE id = 7",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(received.0, "2025-02-15");
        assert_eq!(received.1, 2_000.0);
        assert_eq!(received.2.as_deref(), Some("2025-02-22"));
        assert_eq!(received.3, Some(2_200.0));
        assert_eq!(received.4, Some(2_200.0));
    }

    #[test]
    fn manifest_has_no_field_for_urls_paths_or_rows() {
        let mut manifest = ValidationManifest::new("1:2:", Vec::new());
        manifest.login_canary = Some(LoginCanaryValidation {
            expected: "environment-only credential verifies twice".into(),
            source_password_verified: true,
            artifact_password_verified: true,
        });
        manifest.credential_canary = Some(CredentialCanaryValidation {
            source_tmo_count: 1,
            source_monarch_count: 1,
            artifact_tmo_count: 1,
            artifact_monarch_count: 1,
            source_verified: true,
            artifact_verified: true,
            key_fingerprint: Some("63581ea5".into()),
            legacy_key_fingerprint: None,
            rewrapped_tmo_count: 0,
            rewrapped_monarch_count: 0,
        });
        let serialized = serde_json::to_string(&manifest).unwrap();
        assert!(!serialized.contains("postgres://"));
        assert!(!serialized.contains("source_url"));
        assert!(!serialized.contains("output_path"));
        assert!(!serialized.contains("password_hash"));
        assert!(!serialized.contains(FIXTURE_PASSWORD));
        assert!(!serialized.contains("legacy-fixture-key"));
        assert!(!serialized.contains("QsRRZq1wtthAKK37nWsnen"));
        assert!(!serialized.contains("AAECAwQFBgcICQoL"));
        assert!(serialized.contains("source_password_verified"));
        assert!(serialized.contains("63581ea5"));
    }

    #[test]
    fn credential_canary_decrypts_every_provider_before_and_after_copy() {
        let dataset = fixture();
        let key = CredentialKey::new(" legacy-fixture-key ".into()).unwrap();
        let validation = validate_credential_canary(&dataset, Some(&key)).unwrap();
        assert_eq!(validation.tmo_count, 1);
        assert_eq!(validation.monarch_count, 1);
        assert_eq!(validation.key_fingerprint.as_deref(), Some("63581ea5"));

        let missing = validate_credential_canary(&dataset, None).unwrap_err();
        assert!(missing.downcast_ref::<ManifestSafeBlocker>().is_some());
        let wrong = CredentialKey::new("wrong-key".into()).unwrap();
        let error = validate_credential_canary(&dataset, Some(&wrong)).unwrap_err();
        assert!(error.downcast_ref::<ManifestSafeBlocker>().is_some());
        let rendered = format!("{error:#}");
        assert!(!rendered.contains("wrong-key"));
        assert!(!rendered.contains("QsRRZq1wtthAKK37nWsnen"));
    }

    #[test]
    fn explicit_legacy_key_rewraps_credentials_under_the_current_key() {
        let mut dataset = fixture();
        let current = CredentialKey::new("current-production-key".into()).unwrap();
        let legacy = CredentialKey::new("legacy-fixture-key".into()).unwrap();
        let rewrap =
            rewrap_legacy_credentials(&mut dataset, Some(&current), Some(&legacy)).unwrap();
        assert_eq!(rewrap.tmo_count, 1);
        assert_eq!(rewrap.monarch_count, 1);
        assert_eq!(rewrap.legacy_key_fingerprint.as_deref(), Some("63581ea5"));

        validate_credential_canary(&dataset, Some(&current)).unwrap();
        assert!(validate_credential_canary(&dataset, Some(&legacy)).is_err());

        let mut missing_legacy = fixture();
        let error = rewrap_legacy_credentials(&mut missing_legacy, Some(&current), None)
            .expect_err("a mismatched current key must not silently discard credentials");
        assert!(error.downcast_ref::<ManifestSafeBlocker>().is_some());
        let rendered = format!("{error:#}");
        assert!(!rendered.contains("current-production-key"));
        assert!(!rendered.contains("legacy-fixture-key"));
        assert!(!rendered.contains("QsRRZq1wtthAKK37nWsnen"));
    }

    #[test]
    fn active_provider_contract_fails_closed_without_exact_credentials() {
        let mut dataset = fixture();
        dataset.tmo_credentials.clear();
        let error = validate_active_provider_credentials(&dataset).unwrap_err();
        assert!(error.downcast_ref::<ManifestSafeBlocker>().is_some());

        let mut dataset = fixture();
        dataset.integration_connections[0].provider = "surprise".into();
        dataset.integration_connections[0].metadata = Some("{\"syncable\":false}".into());
        let error = validate_active_provider_credentials(&dataset).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("credential belongs to a non-TMO connection")
        );
    }

    #[test]
    fn login_canary_matches_the_real_normalization_and_verifier() {
        let canary = LoginCanary {
            email: "  ADMIN@EXAMPLE.COM  ".into(),
            password: FIXTURE_PASSWORD.into(),
        };
        validate_login_canary(&fixture(), &canary).unwrap();

        let wrong = LoginCanary {
            email: canary.email,
            password: "wrong".into(),
        };
        let error = validate_login_canary(&fixture(), &wrong).unwrap_err();
        assert!(error.downcast_ref::<ManifestSafeBlocker>().is_some());
    }

    #[test]
    fn target_migration_checksums_are_deterministic() {
        assert_eq!(target_migrations(), target_migrations());
        assert_eq!(target_migrations().len(), 5);
        assert!(
            target_migrations()
                .iter()
                .all(|migration| migration.blake3.len() == 64)
        );
    }
}
