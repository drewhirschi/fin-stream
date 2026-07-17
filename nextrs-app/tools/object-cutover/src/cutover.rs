use std::collections::{BTreeMap, BTreeSet};

use bytes::Bytes;
use sha2::{Digest, Sha256};

use crate::{
    CutoverError,
    manifest::{
        BackfillDisposition, Blocker, ManifestCounts, ObjectManifest, ObjectReference, Presence,
        RunMode,
    },
    source_db::{DatabaseInventory, DatabaseReference},
    storage::{ByteObjectStore, PutDisposition, StoreError, StoredObject},
};

pub async fn inventory(
    database: DatabaseInventory,
    store: &dyn ByteObjectStore,
    mode: RunMode,
) -> Result<ObjectManifest, CutoverError> {
    let mut manifest = ObjectManifest::new(mode);
    manifest.blockers = database.blockers;
    manifest.counts.empty_email_body_keys_skipped = database.empty_email_body_keys_skipped;
    manifest.counts.database_references = database.references.len() as u64;

    let source_keys = store.list_keys().await.map_err(map_inventory_store_error)?;
    manifest.counts.source_objects = source_keys.len() as u64;

    let key_references = references_by_key(&database.references);
    let source_key_references = references_by_source_key(&database.references);
    for (key, references) in &key_references {
        if references.len() > 1 {
            manifest.counts.duplicate_keys += 1;
            manifest.counts.duplicate_references += (references.len() - 1) as u64;
            for reference in references {
                manifest.blockers.push(Blocker::reference(
                    "duplicate_canonical_key",
                    reference.kind,
                    reference.db_id,
                ));
            }
        }
        let _ = key;
    }
    for (source_key, references) in &source_key_references {
        if !source_keys.contains(source_key) {
            for reference in references {
                manifest.blockers.push(Blocker::reference(
                    "referenced_object_missing",
                    reference.kind,
                    reference.db_id,
                ));
            }
        }
    }

    let referenced_keys = source_key_references
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    manifest.counts.orphan_objects = source_keys.difference(&referenced_keys).count() as u64;
    if manifest.counts.orphan_objects > 0 {
        manifest
            .blockers
            .push(Blocker::global("unreferenced_source_objects"));
    }

    for reference in database.references {
        manifest
            .references
            .push(inspect_reference(reference, store).await?);
    }
    manifest
        .references
        .sort_by_key(|reference| (reference.reference_kind, reference.db_id));
    populate_counts(&mut manifest);
    manifest.valid = manifest.blockers.is_empty();
    Ok(manifest)
}

pub async fn backfill_local_to_s3(
    database: DatabaseInventory,
    local: &dyn ByteObjectStore,
    destination: &dyn ByteObjectStore,
) -> Result<ObjectManifest, CutoverError> {
    transfer_to_canonical_destination(database, local, destination, RunMode::BackfillLocalToS3)
        .await
}

pub async fn promote_s3_to_s3(
    database: DatabaseInventory,
    source: &dyn ByteObjectStore,
    destination: &dyn ByteObjectStore,
) -> Result<ObjectManifest, CutoverError> {
    transfer_to_canonical_destination(database, source, destination, RunMode::PromoteS3ToS3).await
}

async fn transfer_to_canonical_destination(
    database: DatabaseInventory,
    source: &dyn ByteObjectStore,
    destination: &dyn ByteObjectStore,
    mode: RunMode,
) -> Result<ObjectManifest, CutoverError> {
    let source_keys = database
        .references
        .iter()
        .filter_map(|reference| {
            Some((
                (reference.kind, reference.db_id),
                reference.source_key.clone()?,
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut manifest = inventory(database, source, mode).await?;
    if !manifest.valid {
        return Ok(manifest);
    }

    for reference in &mut manifest.references {
        let Some(key) = reference.canonical_key.as_deref() else {
            continue;
        };
        let source_key = source_keys
            .get(&(reference.reference_kind, reference.db_id))
            .ok_or(CutoverError::Backfill)?;
        let source = source
            .get(source_key)
            .await
            .map_err(|_| CutoverError::TransferSourceRead)?
            .ok_or(CutoverError::Backfill)?;
        let source_hash = sha256(&source.bytes);
        let desired_content_type = reference
            .content_type
            .clone()
            .or(source.content_type.clone())
            .or_else(|| mime_guess::from_path(key).first_raw().map(str::to_owned));
        let upload = StoredObject {
            bytes: source.bytes,
            content_type: desired_content_type.clone(),
        };
        let disposition = destination
            .put_create(key, upload, &source_hash)
            .await
            .map_err(map_destination_write_error)?;

        let Some(remote) = destination
            .get(key)
            .await
            .map_err(|_| CutoverError::DestinationVerificationUnavailable)?
        else {
            manifest.blockers.push(Blocker::reference(
                "destination_object_missing_after_put",
                reference.reference_kind,
                reference.db_id,
            ));
            reference.presence = Presence::Missing;
            continue;
        };
        let remote_hash = sha256(&remote.bytes);
        let remote_content_type = remote.content_type.clone();
        reference.size_bytes = Some(remote.bytes.len() as u64);
        reference.sha256 = Some(remote_hash.clone());
        reference.content_type = remote_content_type.clone().or(desired_content_type.clone());
        reference.presence = Presence::Present;

        if remote_hash != source_hash {
            manifest.counts.mismatched_existing_objects += 1;
            manifest.blockers.push(Blocker::reference(
                "destination_hash_mismatch",
                reference.reference_kind,
                reference.db_id,
            ));
            continue;
        }
        if desired_content_type.is_some() && remote_content_type != desired_content_type {
            manifest.blockers.push(Blocker::reference(
                "destination_content_type_mismatch",
                reference.reference_kind,
                reference.db_id,
            ));
            continue;
        }

        reference.backfill = Some(match disposition {
            PutDisposition::Created => {
                manifest.counts.uploaded_objects += 1;
                BackfillDisposition::Uploaded
            }
            PutDisposition::AlreadyExists => {
                manifest.counts.existing_verified_objects += 1;
                BackfillDisposition::ExistingVerified
            }
        });
    }

    let destination_keys = destination
        .list_keys()
        .await
        .map_err(|_| CutoverError::DestinationVerificationUnavailable)?;
    let referenced_keys = manifest
        .references
        .iter()
        .filter_map(|reference| reference.canonical_key.clone())
        .collect::<BTreeSet<_>>();
    manifest.counts.source_objects = destination_keys.len() as u64;
    manifest.counts.orphan_objects = destination_keys.difference(&referenced_keys).count() as u64;
    if manifest.counts.orphan_objects > 0 {
        manifest
            .blockers
            .push(Blocker::global("unreferenced_destination_objects"));
    }

    // Recalculate ordinary presence counts while retaining backfill counters.
    let backfill_counts = (
        manifest.counts.uploaded_objects,
        manifest.counts.existing_verified_objects,
        manifest.counts.mismatched_existing_objects,
    );
    populate_counts(&mut manifest);
    manifest.counts.uploaded_objects = backfill_counts.0;
    manifest.counts.existing_verified_objects = backfill_counts.1;
    manifest.counts.mismatched_existing_objects = backfill_counts.2;
    manifest.valid = manifest.blockers.is_empty();
    Ok(manifest)
}

fn references_by_key(
    references: &[DatabaseReference],
) -> BTreeMap<String, Vec<&DatabaseReference>> {
    let mut by_key: BTreeMap<String, Vec<&DatabaseReference>> = BTreeMap::new();
    for reference in references {
        if let Some(key) = &reference.canonical_key {
            by_key.entry(key.clone()).or_default().push(reference);
        }
    }
    by_key
}

fn references_by_source_key(
    references: &[DatabaseReference],
) -> BTreeMap<String, Vec<&DatabaseReference>> {
    let mut by_key: BTreeMap<String, Vec<&DatabaseReference>> = BTreeMap::new();
    for reference in references {
        if let Some(key) = &reference.source_key {
            by_key.entry(key.clone()).or_default().push(reference);
        }
    }
    by_key
}

async fn inspect_reference(
    reference: DatabaseReference,
    store: &dyn ByteObjectStore,
) -> Result<ObjectReference, CutoverError> {
    let mut result = ObjectReference {
        reference_kind: reference.kind,
        db_id: reference.db_id,
        canonical_key: reference.canonical_key.clone(),
        size_bytes: None,
        sha256: None,
        content_type: reference.content_type,
        presence: reference.initial_presence,
        backfill: None,
    };
    let Some(key) = reference.canonical_key else {
        return Ok(result);
    };
    let source_key = reference.source_key.ok_or(CutoverError::Inventory)?;
    if let Some(object) = store
        .get(&source_key)
        .await
        .map_err(map_inventory_store_error)?
    {
        result.size_bytes = Some(object.bytes.len() as u64);
        result.sha256 = Some(sha256(&object.bytes));
        result.content_type = result
            .content_type
            .or(object.content_type)
            .or_else(|| mime_guess::from_path(&key).first_raw().map(str::to_owned));
        result.presence = Presence::Present;
    } else {
        result.presence = Presence::Missing;
    }
    Ok(result)
}

fn populate_counts(manifest: &mut ObjectManifest) {
    let preserved = (
        manifest.counts.empty_email_body_keys_skipped,
        manifest.counts.duplicate_keys,
        manifest.counts.duplicate_references,
        manifest.counts.source_objects,
        manifest.counts.orphan_objects,
        manifest.counts.uploaded_objects,
        manifest.counts.existing_verified_objects,
        manifest.counts.mismatched_existing_objects,
    );
    manifest.counts = ManifestCounts::default();
    manifest.counts.database_references = manifest.references.len() as u64;
    manifest.counts.empty_email_body_keys_skipped = preserved.0;
    manifest.counts.duplicate_keys = preserved.1;
    manifest.counts.duplicate_references = preserved.2;
    manifest.counts.source_objects = preserved.3;
    manifest.counts.orphan_objects = preserved.4;
    manifest.counts.uploaded_objects = preserved.5;
    manifest.counts.existing_verified_objects = preserved.6;
    manifest.counts.mismatched_existing_objects = preserved.7;
    for reference in &manifest.references {
        match reference.presence {
            Presence::Present => manifest.counts.present_objects += 1,
            Presence::Missing => manifest.counts.missing_objects += 1,
            Presence::ExternalOnly => manifest.counts.external_only_photos += 1,
            Presence::Invalid => manifest.counts.invalid_references += 1,
        }
        if reference.canonical_key.is_some() {
            manifest.counts.referenced_objects += 1;
        }
    }
}

fn sha256(bytes: &Bytes) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn map_inventory_store_error(error: StoreError) -> CutoverError {
    match error {
        StoreError::Unavailable => CutoverError::ObjectSourceUnavailable,
        StoreError::InvalidKey => CutoverError::ObjectSourceInvalidKey,
        StoreError::AmbiguousEncoding => CutoverError::ObjectSourceAmbiguousEncoding,
        StoreError::CanonicalCollision => CutoverError::ObjectSourceCanonicalCollision,
        StoreError::CreateUnsupported | StoreError::WriteDenied => {
            CutoverError::ObjectSourceUnavailable
        }
    }
}

fn map_destination_write_error(error: StoreError) -> CutoverError {
    match error {
        StoreError::CreateUnsupported => CutoverError::DestinationCreateUnsupported,
        StoreError::WriteDenied => CutoverError::DestinationWriteDenied,
        _ => CutoverError::DestinationWriteUnavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        sync::Mutex,
    };

    use async_trait::async_trait;

    use super::*;
    use crate::{
        manifest::ReferenceKind,
        source_db::DatabaseInventory,
        storage::{ByteObjectStore, PutDisposition, StoreError, StoredObject},
    };

    #[derive(Default)]
    struct MemoryStore {
        objects: Mutex<BTreeMap<String, StoredObject>>,
        created: Mutex<u64>,
    }

    impl MemoryStore {
        fn with(entries: &[(&str, &[u8])]) -> Self {
            Self {
                objects: Mutex::new(
                    entries
                        .iter()
                        .map(|(key, value)| {
                            (
                                (*key).to_owned(),
                                StoredObject {
                                    bytes: Bytes::copy_from_slice(value),
                                    content_type: None,
                                },
                            )
                        })
                        .collect(),
                ),
                created: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl ByteObjectStore for MemoryStore {
        async fn get(&self, key: &str) -> Result<Option<StoredObject>, StoreError> {
            Ok(self.objects.lock().unwrap().get(key).cloned())
        }

        async fn list_keys(&self) -> Result<BTreeSet<String>, StoreError> {
            Ok(self.objects.lock().unwrap().keys().cloned().collect())
        }

        async fn put_create(
            &self,
            key: &str,
            object: StoredObject,
            _sha256: &str,
        ) -> Result<PutDisposition, StoreError> {
            let mut objects = self.objects.lock().unwrap();
            if objects.contains_key(key) {
                Ok(PutDisposition::AlreadyExists)
            } else {
                objects.insert(key.to_owned(), object);
                *self.created.lock().unwrap() += 1;
                Ok(PutDisposition::Created)
            }
        }
    }

    fn reference(kind: ReferenceKind, id: i64, key: &str) -> DatabaseReference {
        DatabaseReference {
            kind,
            db_id: id,
            canonical_key: Some(key.to_owned()),
            source_key: Some(key.to_owned()),
            content_type: None,
            initial_presence: Presence::Missing,
        }
    }

    #[tokio::test]
    async fn missing_orphan_and_duplicate_references_block_inventory() {
        let store = MemoryStore::with(&[("objects/present", b"present"), ("orphan", b"orphan")]);
        let database = DatabaseInventory {
            references: vec![
                reference(ReferenceKind::ReceivedEmailBody, 1, "objects/present"),
                reference(ReferenceKind::ReceivedEmailAttachment, 2, "objects/present"),
                reference(ReferenceKind::LoanWorkspacePhoto, 3, "objects/missing"),
            ],
            ..DatabaseInventory::default()
        };

        let manifest = inventory(database, &store, RunMode::InventoryLocal)
            .await
            .unwrap();
        assert!(!manifest.valid);
        assert_eq!(manifest.counts.duplicate_keys, 1);
        assert_eq!(manifest.counts.duplicate_references, 1);
        assert_eq!(manifest.counts.missing_objects, 1);
        assert_eq!(manifest.counts.orphan_objects, 1);
    }

    #[tokio::test]
    async fn backfill_is_idempotent_and_verifies_by_downloading() {
        let local = MemoryStore::with(&[("objects/one", b"one")]);
        let destination = MemoryStore::default();
        let make_database = || DatabaseInventory {
            references: vec![reference(
                ReferenceKind::ReceivedEmailAttachment,
                1,
                "objects/one",
            )],
            ..DatabaseInventory::default()
        };

        let first = backfill_local_to_s3(make_database(), &local, &destination)
            .await
            .unwrap();
        assert!(first.valid);
        assert_eq!(first.counts.uploaded_objects, 1);
        assert_eq!(*destination.created.lock().unwrap(), 1);

        let second = backfill_local_to_s3(make_database(), &local, &destination)
            .await
            .unwrap();
        assert!(second.valid);
        assert_eq!(second.counts.existing_verified_objects, 1);
        assert_eq!(*destination.created.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn an_existing_mismatch_is_never_overwritten() {
        let local = MemoryStore::with(&[("objects/one", b"expected")]);
        let destination = MemoryStore::with(&[("objects/one", b"different")]);
        let database = DatabaseInventory {
            references: vec![reference(
                ReferenceKind::ReceivedEmailAttachment,
                1,
                "objects/one",
            )],
            ..DatabaseInventory::default()
        };

        let manifest = backfill_local_to_s3(database, &local, &destination)
            .await
            .unwrap();
        assert!(!manifest.valid);
        assert_eq!(manifest.counts.mismatched_existing_objects, 1);
        assert_eq!(
            destination.get("objects/one").await.unwrap().unwrap().bytes,
            Bytes::from_static(b"different")
        );
    }

    #[tokio::test]
    async fn matching_bytes_with_wrong_content_type_are_not_accepted() {
        let local = MemoryStore {
            objects: Mutex::new(BTreeMap::from([(
                "objects/one.html".to_owned(),
                StoredObject {
                    bytes: Bytes::from_static(b"same"),
                    content_type: Some("text/html".to_owned()),
                },
            )])),
            created: Mutex::new(0),
        };
        let destination = MemoryStore {
            objects: Mutex::new(BTreeMap::from([(
                "objects/one.html".to_owned(),
                StoredObject {
                    bytes: Bytes::from_static(b"same"),
                    content_type: Some("application/octet-stream".to_owned()),
                },
            )])),
            created: Mutex::new(0),
        };
        let database = DatabaseInventory {
            references: vec![reference(
                ReferenceKind::ReceivedEmailBody,
                1,
                "objects/one.html",
            )],
            ..DatabaseInventory::default()
        };

        let manifest = backfill_local_to_s3(database, &local, &destination)
            .await
            .unwrap();
        assert!(!manifest.valid);
        assert!(
            manifest
                .blockers
                .iter()
                .any(|blocker| blocker.code == "destination_content_type_mismatch")
        );
    }
}
