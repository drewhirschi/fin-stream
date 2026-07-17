use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use bytes::Bytes;
use futures::TryStreamExt;
use object_store::{
    Attribute, AttributeValue, Attributes, ObjectStore as RemoteObjectStore, PutMode, PutOptions,
    PutPayload, aws::AmazonS3Builder, path::Path as ObjectPath,
};
use walkdir::WalkDir;

use crate::{
    CutoverError,
    key::{KeyError, canonical_key, legacy_physical_key},
    require_absolute,
};

const REMOTE_ATTEMPTS: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub bytes: Bytes,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutDisposition {
    Created,
    AlreadyExists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    Unavailable,
    InvalidKey,
    AmbiguousEncoding,
    CanonicalCollision,
    CreateUnsupported,
    WriteDenied,
}

/// Physical key layout used by an object store.
///
/// The legacy app stored a photo as `<loan-account>/<file>` below its storage
/// root while exposing it at `/media/loan-workspace/<loan-account>/<file>`.
/// Email objects already carried their `emails/` namespace. New destinations
/// store the canonical database key verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectKeyLayout {
    LegacySource,
    Canonical,
}

#[async_trait]
pub trait ByteObjectStore: Send + Sync {
    async fn get(&self, canonical_key: &str) -> Result<Option<StoredObject>, StoreError>;
    async fn list_keys(&self) -> Result<BTreeSet<String>, StoreError>;
    async fn put_create(
        &self,
        canonical_key: &str,
        object: StoredObject,
        sha256: &str,
    ) -> Result<PutDisposition, StoreError>;
}

#[derive(Debug, Clone)]
pub struct LocalDirectoryStore {
    root: PathBuf,
}

impl LocalDirectoryStore {
    pub fn open(path: &Path) -> Result<Self, CutoverError> {
        require_absolute(path, CutoverError::RelativeSourcePath)?;
        let root = path.canonicalize().map_err(|_| CutoverError::LocalSource)?;
        if !root.is_dir() {
            return Err(CutoverError::LocalSource);
        }
        Ok(Self { root })
    }

    fn resolved_file(&self, key: &str) -> Result<Option<PathBuf>, StoreError> {
        legacy_physical_key(key).map_err(map_key_error)?;
        let mut resolved = None;
        for physical_key in physical_candidates(key, ObjectKeyLayout::LegacySource) {
            let candidate = self.root.join(&physical_key);
            if !candidate.exists() {
                continue;
            }

            let mut cursor = self.root.clone();
            for segment in physical_key.split('/') {
                cursor.push(segment);
                let metadata =
                    std::fs::symlink_metadata(&cursor).map_err(|_| StoreError::Unavailable)?;
                if metadata.file_type().is_symlink() {
                    return Err(StoreError::InvalidKey);
                }
            }
            let candidate = candidate
                .canonicalize()
                .map_err(|_| StoreError::Unavailable)?;
            if !candidate.starts_with(&self.root) || !candidate.is_file() || resolved.is_some() {
                return Err(StoreError::CanonicalCollision);
            }
            resolved = Some(candidate);
        }
        Ok(resolved)
    }
}

#[async_trait]
impl ByteObjectStore for LocalDirectoryStore {
    async fn get(&self, canonical_key: &str) -> Result<Option<StoredObject>, StoreError> {
        let Some(path) = self.resolved_file(canonical_key)? else {
            return Ok(None);
        };
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|_| StoreError::Unavailable)?;
        Ok(Some(StoredObject {
            bytes: Bytes::from(bytes),
            content_type: mime_guess::from_path(canonical_key)
                .first_raw()
                .map(str::to_owned),
        }))
    }

    async fn list_keys(&self) -> Result<BTreeSet<String>, StoreError> {
        let mut keys = BTreeSet::new();
        for entry in WalkDir::new(&self.root).follow_links(false) {
            let entry = entry.map_err(|_| StoreError::Unavailable)?;
            if entry.path() == self.root {
                continue;
            }
            if entry.file_type().is_symlink() {
                return Err(StoreError::InvalidKey);
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&self.root)
                .map_err(|_| StoreError::InvalidKey)?;
            let key = relative
                .components()
                .map(|component| component.as_os_str().to_str().ok_or(StoreError::InvalidKey))
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            let key = canonicalize_physical_key(&key, ObjectKeyLayout::LegacySource)?;
            if !keys.insert(key) {
                return Err(StoreError::CanonicalCollision);
            }
        }
        Ok(keys)
    }

    async fn put_create(
        &self,
        _canonical_key: &str,
        _object: StoredObject,
        _sha256: &str,
    ) -> Result<PutDisposition, StoreError> {
        // Local directories are sources only. Keeping this unsupported prevents
        // an accidental reverse-copy from mutating the retained source corpus.
        Err(StoreError::Unavailable)
    }
}

pub struct S3CompatibleStore {
    store: Arc<dyn RemoteObjectStore>,
    prefix: String,
    layout: ObjectKeyLayout,
}

impl S3CompatibleStore {
    pub fn from_environment(layout: ObjectKeyLayout) -> Result<Self, CutoverError> {
        Self::from_environment_with_prefix(layout, "OBJECT_CUTOVER_S3_PREFIX")
    }

    pub fn from_environment_with_prefix(
        layout: ObjectKeyLayout,
        prefix_environment_name: &str,
    ) -> Result<Self, CutoverError> {
        let endpoint = required_env("OBJECT_CUTOVER_S3_ENDPOINT")?;
        let region = required_env("OBJECT_CUTOVER_S3_REGION")?;
        let bucket = required_env("OBJECT_CUTOVER_S3_BUCKET")?;
        let access_key = required_env("OBJECT_CUTOVER_S3_ACCESS_KEY_ID")?;
        let secret_key = required_env("OBJECT_CUTOVER_S3_SECRET_ACCESS_KEY")?;
        let raw_prefix = env::var(prefix_environment_name).unwrap_or_default();
        let prefix = normalize_prefix(&raw_prefix)?;
        let allow_http = match env::var("OBJECT_CUTOVER_S3_ALLOW_HTTP") {
            Ok(value) if value == "1" || value.eq_ignore_ascii_case("true") => true,
            Ok(value) if value == "0" || value.eq_ignore_ascii_case("false") => false,
            Ok(_) => return Err(CutoverError::S3Configuration),
            Err(env::VarError::NotPresent) => false,
            Err(env::VarError::NotUnicode(_)) => return Err(CutoverError::S3Configuration),
        };

        let parsed_endpoint =
            url::Url::parse(&endpoint).map_err(|_| CutoverError::S3Configuration)?;
        if parsed_endpoint.username() != ""
            || parsed_endpoint.password().is_some()
            || parsed_endpoint.query().is_some()
            || parsed_endpoint.fragment().is_some()
            || (!allow_http && parsed_endpoint.scheme() != "https")
            || (allow_http && !matches!(parsed_endpoint.scheme(), "http" | "https"))
        {
            return Err(CutoverError::S3Configuration);
        }

        let store = AmazonS3Builder::new()
            .with_endpoint(endpoint)
            .with_region(region)
            .with_bucket_name(bucket)
            .with_access_key_id(access_key)
            .with_secret_access_key(secret_key)
            .with_allow_http(allow_http)
            .build()
            .map_err(|_| CutoverError::S3Configuration)?;

        Ok(Self {
            store: Arc::new(store),
            prefix,
            layout,
        })
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    fn remote_key(&self, key: &str) -> Result<ObjectPath, StoreError> {
        match self.layout {
            ObjectKeyLayout::LegacySource => legacy_physical_key(key),
            ObjectKeyLayout::Canonical => canonical_key(key),
        }
        .map_err(map_key_error)?;
        let key = if self.prefix.is_empty() {
            key.to_owned()
        } else {
            format!("{}/{key}", self.prefix)
        };
        ObjectPath::parse(key).map_err(|_| StoreError::InvalidKey)
    }

    fn list_prefix(&self) -> Option<ObjectPath> {
        if self.prefix.is_empty() {
            None
        } else {
            Some(ObjectPath::from(self.prefix.clone()))
        }
    }

    fn canonical_from_remote(&self, remote: &str) -> Result<Option<String>, StoreError> {
        let key = if self.prefix.is_empty() {
            remote
        } else {
            let Some(key) = remote
                .strip_prefix(&self.prefix)
                .and_then(|suffix| suffix.strip_prefix('/'))
            else {
                // S3 prefix matching is byte-oriented. Listing `foo` can also
                // return `foobar`; those sibling namespaces are not ours.
                return Ok(None);
            };
            key
        };
        canonicalize_physical_key(key, self.layout).map(Some)
    }

    async fn get_physical(&self, physical_key: &str) -> Result<Option<StoredObject>, StoreError> {
        let key = self.remote_key(physical_key)?;
        for attempt in 0..REMOTE_ATTEMPTS {
            let result = match self.store.get(&key).await {
                Ok(result) => result,
                Err(object_store::Error::NotFound { .. }) => return Ok(None),
                Err(_) if attempt + 1 < REMOTE_ATTEMPTS => {
                    remote_retry_delay(attempt).await;
                    continue;
                }
                Err(_) => return Err(StoreError::Unavailable),
            };
            let content_type = result
                .attributes
                .get(&Attribute::ContentType)
                .map(|value| value.as_ref().to_owned());
            match result.bytes().await {
                Ok(bytes) => {
                    return Ok(Some(StoredObject {
                        bytes,
                        content_type,
                    }));
                }
                Err(_) if attempt + 1 < REMOTE_ATTEMPTS => {
                    remote_retry_delay(attempt).await;
                }
                Err(_) => return Err(StoreError::Unavailable),
            }
        }
        Err(StoreError::Unavailable)
    }
}

#[async_trait]
impl ByteObjectStore for S3CompatibleStore {
    async fn get(&self, requested_key: &str) -> Result<Option<StoredObject>, StoreError> {
        match self.layout {
            ObjectKeyLayout::LegacySource => legacy_physical_key(requested_key),
            ObjectKeyLayout::Canonical => canonical_key(requested_key),
        }
        .map_err(map_key_error)?;
        let mut found = None;
        for physical_key in physical_candidates(requested_key, self.layout) {
            if let Some(object) = self.get_physical(&physical_key).await? {
                if found.is_some() {
                    return Err(StoreError::CanonicalCollision);
                }
                found = Some(object);
            }
        }
        Ok(found)
    }

    async fn list_keys(&self) -> Result<BTreeSet<String>, StoreError> {
        let prefix = self.list_prefix();
        let mut objects = None;
        for attempt in 0..REMOTE_ATTEMPTS {
            match self
                .store
                .list(prefix.as_ref())
                .try_collect::<Vec<_>>()
                .await
            {
                Ok(listed) => {
                    objects = Some(listed);
                    break;
                }
                Err(_) if attempt + 1 < REMOTE_ATTEMPTS => remote_retry_delay(attempt).await,
                Err(_) => return Err(StoreError::Unavailable),
            }
        }
        let objects = objects.ok_or(StoreError::Unavailable)?;
        let mut keys = BTreeSet::new();
        for object in objects {
            if let Some(key) = self.canonical_from_remote(object.location.as_ref())? {
                if !keys.insert(key) {
                    return Err(StoreError::CanonicalCollision);
                }
            }
        }
        Ok(keys)
    }

    async fn put_create(
        &self,
        canonical_key: &str,
        object: StoredObject,
        sha256: &str,
    ) -> Result<PutDisposition, StoreError> {
        if self.layout != ObjectKeyLayout::Canonical {
            return Err(StoreError::Unavailable);
        }
        let key = self.remote_key(canonical_key)?;
        for attempt in 0..REMOTE_ATTEMPTS {
            let mut attributes = Attributes::new();
            if let Some(content_type) = &object.content_type {
                attributes.insert(
                    Attribute::ContentType,
                    AttributeValue::from(content_type.clone()),
                );
            }
            attributes.insert(
                Attribute::Metadata("sha256".into()),
                AttributeValue::from(sha256.to_owned()),
            );
            let options = PutOptions {
                mode: PutMode::Create,
                attributes,
                ..PutOptions::default()
            };
            match self
                .store
                .put_opts(&key, PutPayload::from(object.bytes.clone()), options)
                .await
            {
                Ok(_) => return Ok(PutDisposition::Created),
                Err(
                    object_store::Error::AlreadyExists { .. }
                    | object_store::Error::Precondition { .. },
                ) => return Ok(PutDisposition::AlreadyExists),
                Err(
                    object_store::Error::NotSupported { .. } | object_store::Error::NotImplemented,
                ) => return Err(StoreError::CreateUnsupported),
                Err(
                    object_store::Error::PermissionDenied { .. }
                    | object_store::Error::Unauthenticated { .. },
                ) => return Err(StoreError::WriteDenied),
                Err(_) if attempt + 1 < REMOTE_ATTEMPTS => remote_retry_delay(attempt).await,
                Err(_) => return Err(StoreError::Unavailable),
            }
        }
        Err(StoreError::Unavailable)
    }
}

async fn remote_retry_delay(attempt: u32) {
    tokio::time::sleep(std::time::Duration::from_millis(100 * 2_u64.pow(attempt))).await;
}

fn physical_candidates(canonical: &str, layout: ObjectKeyLayout) -> Vec<String> {
    let mut candidates = vec![canonical.to_owned()];
    if layout == ObjectKeyLayout::LegacySource {
        if let Some(legacy) = canonical.strip_prefix("loan-workspace/") {
            if !legacy.is_empty() {
                candidates.push(legacy.to_owned());
            }
        }
    }
    candidates
}

fn canonicalize_physical_key(
    physical: &str,
    layout: ObjectKeyLayout,
) -> Result<String, StoreError> {
    let physical = match layout {
        ObjectKeyLayout::LegacySource => legacy_physical_key(physical),
        ObjectKeyLayout::Canonical => canonical_key(physical),
    }
    .map_err(map_key_error)?;
    if layout == ObjectKeyLayout::Canonical
        || physical.starts_with("loan-workspace/")
        || physical.starts_with("emails/")
    {
        return Ok(physical);
    }
    legacy_physical_key(&format!("loan-workspace/{physical}")).map_err(map_key_error)
}

fn map_key_error(error: KeyError) -> StoreError {
    match error {
        KeyError::AmbiguousEncoding => StoreError::AmbiguousEncoding,
        _ => StoreError::InvalidKey,
    }
}

fn required_env(name: &str) -> Result<String, CutoverError> {
    let value = env::var(name).map_err(|_| CutoverError::S3Configuration)?;
    if value.trim().is_empty()
        || value != value.trim()
        || value.chars().any(|character| character.is_control())
    {
        Err(CutoverError::S3Configuration)
    } else {
        Ok(value)
    }
}

fn normalize_prefix(raw: &str) -> Result<String, CutoverError> {
    if raw.is_empty() {
        return Ok(String::new());
    }
    if raw != raw.trim() || raw.starts_with('/') || raw.ends_with('/') {
        return Err(CutoverError::S3Configuration);
    }
    canonical_key(raw).map_err(|_| CutoverError::S3Configuration)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fs, os::unix::fs::symlink};

    use tempfile::tempdir;

    use super::*;

    #[tokio::test]
    async fn local_store_lists_and_reads_canonical_files() {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join("loan-workspace/one")).unwrap();
        fs::write(
            directory.path().join("loan-workspace/one/front.jpg"),
            b"photo",
        )
        .unwrap();
        let store = LocalDirectoryStore::open(directory.path()).unwrap();

        assert_eq!(
            store.list_keys().await.unwrap(),
            BTreeSet::from(["loan-workspace/one/front.jpg".to_owned()])
        );
        assert_eq!(
            store
                .get("loan-workspace/one/front.jpg")
                .await
                .unwrap()
                .unwrap()
                .bytes,
            Bytes::from_static(b"photo")
        );
    }

    #[tokio::test]
    async fn local_store_maps_the_real_legacy_photo_layout() {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join("loan-100")).unwrap();
        fs::create_dir_all(directory.path().join("emails/message-1")).unwrap();
        fs::write(directory.path().join("loan-100/front.jpg"), b"photo").unwrap();
        fs::write(directory.path().join("emails/message-1/body.html"), b"body").unwrap();
        let store = LocalDirectoryStore::open(directory.path()).unwrap();

        assert_eq!(
            store.list_keys().await.unwrap(),
            BTreeSet::from([
                "emails/message-1/body.html".to_owned(),
                "loan-workspace/loan-100/front.jpg".to_owned(),
            ])
        );
        assert_eq!(
            store
                .get("loan-workspace/loan-100/front.jpg")
                .await
                .unwrap()
                .unwrap()
                .bytes,
            Bytes::from_static(b"photo")
        );
    }

    #[tokio::test]
    async fn local_store_rejects_legacy_and_canonical_collisions() {
        let directory = tempdir().unwrap();
        fs::create_dir_all(directory.path().join("loan-100")).unwrap();
        fs::create_dir_all(directory.path().join("loan-workspace/loan-100")).unwrap();
        fs::write(directory.path().join("loan-100/front.jpg"), b"old").unwrap();
        fs::write(
            directory.path().join("loan-workspace/loan-100/front.jpg"),
            b"new",
        )
        .unwrap();
        let store = LocalDirectoryStore::open(directory.path()).unwrap();

        assert_eq!(store.list_keys().await, Err(StoreError::CanonicalCollision));
        assert_eq!(
            store.get("loan-workspace/loan-100/front.jpg").await,
            Err(StoreError::CanonicalCollision)
        );
    }

    #[tokio::test]
    async fn local_store_rejects_symlinks() {
        let directory = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("secret"), b"secret").unwrap();
        symlink(
            outside.path().join("secret"),
            directory.path().join("photo"),
        )
        .unwrap();
        let store = LocalDirectoryStore::open(directory.path()).unwrap();
        assert_eq!(store.list_keys().await, Err(StoreError::InvalidKey));
        assert_eq!(store.get("photo").await, Err(StoreError::InvalidKey));
    }

    #[test]
    fn prefix_is_canonical_or_empty() {
        assert_eq!(normalize_prefix("").unwrap(), "");
        assert_eq!(
            normalize_prefix("trust-deeds/prod").unwrap(),
            "trust-deeds/prod"
        );
        assert!(normalize_prefix("/trust-deeds").is_err());
        assert!(normalize_prefix("trust-deeds/../other").is_err());
    }

    #[test]
    fn physical_key_mapping_is_layout_explicit() {
        assert_eq!(
            canonicalize_physical_key("loan-100/front.jpg", ObjectKeyLayout::LegacySource),
            Ok("loan-workspace/loan-100/front.jpg".to_owned())
        );
        assert_eq!(
            canonicalize_physical_key("emails/one/body.html", ObjectKeyLayout::LegacySource),
            Ok("emails/one/body.html".to_owned())
        );
        assert_eq!(
            canonicalize_physical_key("loan-100/front.jpg", ObjectKeyLayout::Canonical),
            Ok("loan-100/front.jpg".to_owned())
        );
    }
}
