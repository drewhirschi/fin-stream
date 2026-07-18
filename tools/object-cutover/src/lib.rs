pub mod cutover;
pub mod key;
pub mod manifest;
pub mod source_db;
pub mod storage;

use std::path::Path;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CutoverError {
    #[error("the SQLite artifact path must be absolute")]
    RelativeDatabasePath,
    #[error("the manifest output path must be absolute")]
    RelativeManifestPath,
    #[error("the local object source path must be absolute")]
    RelativeSourcePath,
    #[error("the SQLite artifact could not be opened read-only")]
    DatabaseOpen,
    #[error("the SQLite artifact does not have the expected object-reference schema")]
    DatabaseSchema,
    #[error("the SQLite artifact could not be read")]
    DatabaseRead,
    #[error("the local object source is unavailable or unsafe")]
    LocalSource,
    #[error("the S3-compatible object store configuration is incomplete or invalid")]
    S3Configuration,
    #[error("the source and destination object prefixes must be distinct and non-empty")]
    S3PrefixesMustDiffer,
    #[error("the configured object source is unavailable")]
    ObjectSourceUnavailable,
    #[error("the configured object source contains an invalid or unsafe key")]
    ObjectSourceInvalidKey,
    #[error("the configured object source contains a key with ambiguous percent encoding")]
    ObjectSourceAmbiguousEncoding,
    #[error("multiple physical objects map to the same canonical key")]
    ObjectSourceCanonicalCollision,
    #[error("the object inventory could not be completed")]
    Inventory,
    #[error("the backfill could not be completed")]
    Backfill,
    #[error("the verified source object could not be read during transfer")]
    TransferSourceRead,
    #[error("the destination create-only write was unavailable")]
    DestinationWriteUnavailable,
    #[error("the destination object could not be read back after transfer")]
    DestinationVerificationUnavailable,
    #[error("the destination object store does not support the required create-only write")]
    DestinationCreateUnsupported,
    #[error("the destination object store denied the create-only write")]
    DestinationWriteDenied,
    #[error("the manifest parent directory is unavailable")]
    ManifestParent,
    #[error("the manifest output already exists; refusing to overwrite it")]
    ManifestExists,
    #[error("the manifest output must be outside the source repository")]
    ManifestInsideRepository,
    #[error("the manifest could not be published atomically")]
    ManifestPublish,
    #[error("cutover validation found blockers; inspect the manifest")]
    ValidationBlocked,
}

pub fn require_absolute(path: &Path, error: CutoverError) -> Result<(), CutoverError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(error)
    }
}
