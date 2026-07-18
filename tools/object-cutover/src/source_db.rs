use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::{
    CutoverError,
    key::{PhotoLocation, canonical_database_key, classify_photo, legacy_physical_key},
    manifest::{Blocker, Presence, ReferenceKind},
    require_absolute,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseReference {
    pub kind: ReferenceKind,
    pub db_id: i64,
    pub canonical_key: Option<String>,
    pub source_key: Option<String>,
    pub content_type: Option<String>,
    pub initial_presence: Presence,
}

#[derive(Debug, Default)]
pub struct DatabaseInventory {
    pub references: Vec<DatabaseReference>,
    pub blockers: Vec<Blocker>,
    pub empty_email_body_keys_skipped: u64,
}

pub fn read_database(path: &Path) -> Result<DatabaseInventory, CutoverError> {
    require_absolute(path, CutoverError::RelativeDatabasePath)?;
    if !path.is_file() {
        return Err(CutoverError::DatabaseOpen);
    }

    let mut uri = url::Url::from_file_path(path).map_err(|_| CutoverError::DatabaseOpen)?;
    uri.query_pairs_mut().append_pair("immutable", "1");
    let connection = Connection::open_with_flags(
        uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(|_| CutoverError::DatabaseOpen)?;
    connection
        .execute_batch("PRAGMA query_only = ON; PRAGMA foreign_keys = ON; BEGIN;")
        .map_err(|_| CutoverError::DatabaseOpen)?;

    verify_schema(&connection)?;
    let mut inventory = DatabaseInventory::default();
    read_photos(&connection, &mut inventory)?;
    read_email_bodies(&connection, &mut inventory)?;
    read_attachments(&connection, &mut inventory)?;
    connection
        .execute_batch("COMMIT;")
        .map_err(|_| CutoverError::DatabaseRead)?;
    Ok(inventory)
}

fn verify_schema(connection: &Connection) -> Result<(), CutoverError> {
    for (table, required_columns) in [
        ("intg_loan_workspace_photo", &["id", "image_url"] as &[&str]),
        (
            "intg_received_email",
            &["id", "resend_email_id", "body_s3_key", "body_content_type"],
        ),
        (
            "intg_received_email_attachment",
            &[
                "id",
                "email_id",
                "resend_attachment_id",
                "filename",
                "s3_key",
                "content_type",
            ],
        ),
    ] {
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(|_| CutoverError::DatabaseSchema)?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|_| CutoverError::DatabaseSchema)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| CutoverError::DatabaseSchema)?;
        if !required_columns
            .iter()
            .all(|required| columns.iter().any(|column| column == required))
        {
            return Err(CutoverError::DatabaseSchema);
        }
    }
    Ok(())
}

fn read_photos(
    connection: &Connection,
    inventory: &mut DatabaseInventory,
) -> Result<(), CutoverError> {
    let mut statement = connection
        .prepare("SELECT id, image_url FROM intg_loan_workspace_photo ORDER BY id")
        .map_err(|_| CutoverError::DatabaseRead)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| CutoverError::DatabaseRead)?;

    for row in rows {
        let (id, image_url) = row.map_err(|_| CutoverError::DatabaseRead)?;
        match classify_photo(&image_url) {
            Ok(PhotoLocation::Stored(key)) => inventory.references.push(DatabaseReference {
                kind: ReferenceKind::LoanWorkspacePhoto,
                db_id: id,
                source_key: Some(key.clone()),
                canonical_key: Some(key),
                content_type: None,
                initial_presence: Presence::Missing,
            }),
            Ok(PhotoLocation::ExternalOnly) => inventory.references.push(DatabaseReference {
                kind: ReferenceKind::LoanWorkspacePhoto,
                db_id: id,
                canonical_key: None,
                source_key: None,
                content_type: None,
                initial_presence: Presence::ExternalOnly,
            }),
            Err(_) => {
                inventory.references.push(DatabaseReference {
                    kind: ReferenceKind::LoanWorkspacePhoto,
                    db_id: id,
                    canonical_key: None,
                    source_key: None,
                    content_type: None,
                    initial_presence: Presence::Invalid,
                });
                inventory.blockers.push(Blocker::reference(
                    "invalid_photo_location",
                    ReferenceKind::LoanWorkspacePhoto,
                    id,
                ));
            }
        }
    }
    Ok(())
}

fn read_email_bodies(
    connection: &Connection,
    inventory: &mut DatabaseInventory,
) -> Result<(), CutoverError> {
    let mut statement = connection
        .prepare("SELECT id, body_s3_key, body_content_type FROM intg_received_email ORDER BY id")
        .map_err(|_| CutoverError::DatabaseRead)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|_| CutoverError::DatabaseRead)?;

    for row in rows {
        let (id, key, content_type) = row.map_err(|_| CutoverError::DatabaseRead)?;
        let Some(key) = key else {
            continue;
        };
        if key.trim().is_empty() {
            // Legacy records use both NULL and an empty key for a message with no
            // stored body. Neither denotes the object-store prefix/root.
            inventory.empty_email_body_keys_skipped += 1;
            continue;
        }
        push_database_key(
            inventory,
            ReferenceKind::ReceivedEmailBody,
            id,
            &key,
            &key,
            content_type,
        );
    }
    Ok(())
}

fn read_attachments(
    connection: &Connection,
    inventory: &mut DatabaseInventory,
) -> Result<(), CutoverError> {
    let mut statement = connection
        .prepare(
            "SELECT attachment.id, attachment.s3_key, attachment.content_type,
                    email.resend_email_id, attachment.filename
             FROM intg_received_email_attachment attachment
             JOIN intg_received_email email ON email.id = attachment.email_id
             ORDER BY attachment.id",
        )
        .map_err(|_| CutoverError::DatabaseRead)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|_| CutoverError::DatabaseRead)?;

    for row in rows {
        let (id, key, content_type, resend_email_id, filename) =
            row.map_err(|_| CutoverError::DatabaseRead)?;
        let Some(key) = key else {
            continue;
        };
        let legacy_filename = filename.replace(['/', '\\', '\0'], "_");
        let source_key = format!("emails/{resend_email_id}/attachments/{legacy_filename}");
        push_database_key(
            inventory,
            ReferenceKind::ReceivedEmailAttachment,
            id,
            &key,
            &source_key,
            Some(content_type),
        );
    }
    Ok(())
}

fn push_database_key(
    inventory: &mut DatabaseInventory,
    kind: ReferenceKind,
    id: i64,
    raw_key: &str,
    source_key: &str,
    content_type: Option<String>,
) {
    match (
        canonical_database_key(raw_key),
        legacy_physical_key(source_key),
    ) {
        (Ok(key), Ok(source_key)) => inventory.references.push(DatabaseReference {
            kind,
            db_id: id,
            canonical_key: Some(key),
            source_key: Some(source_key),
            content_type,
            initial_presence: Presence::Missing,
        }),
        (Err(_), _) | (_, Err(_)) => {
            inventory.references.push(DatabaseReference {
                kind,
                db_id: id,
                canonical_key: None,
                source_key: None,
                content_type,
                initial_presence: Presence::Invalid,
            });
            inventory
                .blockers
                .push(Blocker::reference("invalid_object_key", kind, id));
        }
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;
    use tempfile::tempdir;

    use super::*;

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("artifact.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE intg_loan_workspace_photo (id INTEGER PRIMARY KEY, image_url TEXT NOT NULL);
                 CREATE TABLE intg_received_email (
                     id INTEGER PRIMARY KEY,
                     resend_email_id TEXT NOT NULL,
                     body_s3_key TEXT,
                     body_content_type TEXT
                 );
                 CREATE TABLE intg_received_email_attachment (
                     id INTEGER PRIMARY KEY,
                     email_id INTEGER NOT NULL,
                     resend_attachment_id TEXT NOT NULL,
                     filename TEXT NOT NULL,
                     s3_key TEXT,
                     content_type TEXT NOT NULL
                 );",
            )
            .unwrap();
        drop(connection);
        (directory, path)
    }

    #[test]
    fn empty_email_body_key_is_not_the_object_store_root() {
        let (_directory, path) = fixture();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO intg_received_email (id, resend_email_id, body_s3_key) VALUES (1, 'email-1', '')",
                [],
            )
            .unwrap();
        drop(connection);

        let inventory = read_database(&path).unwrap();
        assert!(inventory.references.is_empty());
        assert_eq!(inventory.empty_email_body_keys_skipped, 1);
    }

    #[test]
    fn database_is_opened_read_only_and_all_reference_kinds_are_loaded() {
        let (_directory, path) = fixture();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "INSERT INTO intg_loan_workspace_photo VALUES (1, '/static/loan-images/a.jpg');
                 INSERT INTO intg_loan_workspace_photo VALUES (2, 'https://redfin.example/a.jpg');
                 INSERT INTO intg_received_email VALUES (3, 'email-3', 'email/3/body.html', 'text/html');
                 INSERT INTO intg_received_email_attachment VALUES
                    (4, 3, 'attachment-4', 'a.pdf', 'emails/email-3/attachments/attachment-4', 'application/pdf');",
            )
            .unwrap();
        drop(connection);

        let inventory = read_database(&path).unwrap();
        assert_eq!(inventory.references.len(), 4);
        assert_eq!(inventory.blockers.len(), 0);
        assert_eq!(
            inventory.references[0].canonical_key.as_deref(),
            Some("loan-workspace/a.jpg")
        );
        assert_eq!(
            inventory.references[1].initial_presence,
            Presence::ExternalOnly
        );
    }

    #[test]
    fn wal_artifact_is_opened_immutable_without_creating_sidecars() {
        let (_directory, path) = fixture();
        let connection = Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA journal_mode = WAL", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .unwrap();
        drop(connection);
        let wal = std::path::PathBuf::from(format!("{}-wal", path.display()));
        let shm = std::path::PathBuf::from(format!("{}-shm", path.display()));
        assert!(!wal.exists());
        assert!(!shm.exists());

        read_database(&path).unwrap();
        assert!(!wal.exists());
        assert!(!shm.exists());
    }
}
