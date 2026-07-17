use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tempfile::{Builder, NamedTempFile};

use crate::{CutoverError, require_absolute};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    InventoryLocal,
    InventoryS3,
    BackfillLocalToS3,
    PromoteS3ToS3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    LoanWorkspacePhoto,
    ReceivedEmailBody,
    ReceivedEmailAttachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    Present,
    Missing,
    ExternalOnly,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackfillDisposition {
    Uploaded,
    ExistingVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectReference {
    pub reference_kind: ReferenceKind,
    pub db_id: i64,
    pub canonical_key: Option<String>,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub content_type: Option<String>,
    pub presence: Presence,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backfill: Option<BackfillDisposition>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestCounts {
    pub database_references: u64,
    pub referenced_objects: u64,
    pub present_objects: u64,
    pub missing_objects: u64,
    pub invalid_references: u64,
    pub external_only_photos: u64,
    pub empty_email_body_keys_skipped: u64,
    pub duplicate_keys: u64,
    pub duplicate_references: u64,
    pub source_objects: u64,
    pub orphan_objects: u64,
    pub uploaded_objects: u64,
    pub existing_verified_objects: u64,
    pub mismatched_existing_objects: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blocker {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_kind: Option<ReferenceKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_id: Option<i64>,
}

impl Blocker {
    pub fn reference(code: &str, kind: ReferenceKind, db_id: i64) -> Self {
        Self {
            code: code.to_owned(),
            reference_kind: Some(kind),
            db_id: Some(db_id),
        }
    }

    pub fn global(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            reference_kind: None,
            db_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectManifest {
    pub format_version: u32,
    pub generated_at: DateTime<Utc>,
    pub mode: RunMode,
    pub valid: bool,
    pub counts: ManifestCounts,
    pub references: Vec<ObjectReference>,
    pub blockers: Vec<Blocker>,
}

impl ObjectManifest {
    pub fn new(mode: RunMode) -> Self {
        Self {
            format_version: 1,
            generated_at: Utc::now(),
            mode,
            valid: false,
            counts: ManifestCounts::default(),
            references: Vec::new(),
            blockers: Vec::new(),
        }
    }
}

pub struct ManifestPublisher {
    path: PathBuf,
    temporary: NamedTempFile,
}

impl ManifestPublisher {
    pub fn reserve(path: &Path) -> Result<Self, CutoverError> {
        validate_manifest_destination(path)?;
        let parent = path.parent().ok_or(CutoverError::ManifestParent)?;
        let temporary = Builder::new()
            .prefix(".object-cutover-manifest-")
            .tempfile_in(parent)
            .map_err(|_| CutoverError::ManifestPublish)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|_| CutoverError::ManifestPublish)?;
        }

        Ok(Self {
            path: path.to_owned(),
            temporary,
        })
    }

    pub fn publish(mut self, manifest: &ObjectManifest) -> Result<(), CutoverError> {
        let parent = self.path.parent().ok_or(CutoverError::ManifestParent)?;
        {
            let mut writer = BufWriter::new(self.temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, manifest)
                .map_err(|_| CutoverError::ManifestPublish)?;
            writer
                .write_all(b"\n")
                .map_err(|_| CutoverError::ManifestPublish)?;
            writer.flush().map_err(|_| CutoverError::ManifestPublish)?;
        }
        self.temporary
            .as_file()
            .sync_all()
            .map_err(|_| CutoverError::ManifestPublish)?;

        self.temporary
            .persist_noclobber(&self.path)
            .map_err(|error| {
                if error.error.kind() == std::io::ErrorKind::AlreadyExists {
                    CutoverError::ManifestExists
                } else {
                    CutoverError::ManifestPublish
                }
            })?;

        #[cfg(unix)]
        OpenOptions::new()
            .read(true)
            .open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| CutoverError::ManifestPublish)?;

        Ok(())
    }
}

pub fn publish_manifest(path: &Path, manifest: &ObjectManifest) -> Result<(), CutoverError> {
    ManifestPublisher::reserve(path)?.publish(manifest)
}

pub fn validate_manifest_destination(path: &Path) -> Result<(), CutoverError> {
    require_absolute(path, CutoverError::RelativeManifestPath)?;
    if path.exists() {
        return Err(CutoverError::ManifestExists);
    }

    let parent = path.parent().ok_or(CutoverError::ManifestParent)?;
    if !parent.is_dir() {
        return Err(CutoverError::ManifestParent);
    }
    let parent = parent
        .canonicalize()
        .map_err(|_| CutoverError::ManifestParent)?;
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or(CutoverError::ManifestParent)?
        .canonicalize()
        .map_err(|_| CutoverError::ManifestParent)?;
    if parent.starts_with(repository) {
        return Err(CutoverError::ManifestInsideRepository);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn manifest_is_private_and_never_overwritten() {
        let directory = tempdir().unwrap();
        let output = directory.path().join("manifest.json");
        let manifest = ObjectManifest::new(RunMode::InventoryLocal);

        publish_manifest(&output, &manifest).unwrap();
        assert_eq!(
            fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(matches!(
            publish_manifest(&output, &manifest),
            Err(CutoverError::ManifestExists)
        ));
    }

    #[test]
    fn manifest_inside_repository_is_rejected() {
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("must-not-exist.json");
        assert!(matches!(
            validate_manifest_destination(&output),
            Err(CutoverError::ManifestInsideRepository)
        ));
    }
}
