use anyhow::Context;
use libsql::{Connection, Row, Rows, TransactionBehavior, params};

use super::{
    AttachmentUpsert, DeletedEmailMetadata, EmailUpsert, InboxEmailListItem, LoanWorkspace,
    LoanWorkspaceDraft, LoanWorkspacePhoto, LoanWorkspacePhotoDraft, ReceivedEmail,
    ReceivedEmailAttachment, ReceivedEmailAttachmentDraft, ReceivedEmailDetail, ReceivedEmailDraft,
    WorkspaceInboxError, WorkspaceInboxResult,
};

const MAX_LOAN_ACCOUNT_BYTES: usize = 128;

pub struct WorkspaceInboxRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> WorkspaceInboxRepository<'connection> {
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub async fn workspace(
        &self,
        connection_id: i64,
        loan_account: &str,
    ) -> WorkspaceInboxResult<Option<LoanWorkspace>> {
        workspace_on(self.connection, connection_id, loan_account).await
    }

    pub async fn list_workspaces(
        &self,
        connection_id: i64,
    ) -> WorkspaceInboxResult<Vec<LoanWorkspace>> {
        let rows = self
            .connection
            .query(
                "SELECT id, connection_id, loan_account, redfin_url, zillow_url, \
                        decision_status, target_contribution, actual_contribution, notes, \
                        created_at, updated_at \
                 FROM intg_loan_workspace WHERE connection_id = ?1 \
                 ORDER BY loan_account COLLATE NOCASE, id",
                params![connection_id],
            )
            .await
            .context("query loan workspaces")?;
        collect(rows, workspace_from_row, "loan workspace").await
    }

    /// Upsert all user-owned workspace fields atomically while retaining the
    /// original row id and creation timestamp on updates.
    pub async fn upsert_workspace(
        &self,
        draft: &LoanWorkspaceDraft,
    ) -> WorkspaceInboxResult<LoanWorkspace> {
        validate_workspace(draft)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin loan workspace transaction")?;
        let mut rows = transaction
            .query(
                "INSERT INTO intg_loan_workspace ( \
                    connection_id, loan_account, redfin_url, zillow_url, decision_status, \
                    target_contribution, actual_contribution, notes \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT (connection_id, loan_account) DO UPDATE SET \
                    redfin_url = excluded.redfin_url, \
                    zillow_url = excluded.zillow_url, \
                    decision_status = excluded.decision_status, \
                    target_contribution = excluded.target_contribution, \
                    actual_contribution = excluded.actual_contribution, \
                    notes = excluded.notes, \
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 RETURNING id",
                params![
                    draft.connection_id,
                    draft.loan_account.clone(),
                    draft.redfin_url.clone(),
                    draft.zillow_url.clone(),
                    draft.decision_status.clone(),
                    draft.target_contribution,
                    draft.actual_contribution,
                    draft.notes.clone(),
                ],
            )
            .await
            .context("upsert loan workspace")?;
        let id = rows
            .next()
            .await
            .context("read upserted loan workspace id")?
            .context("loan workspace upsert returned no row")?
            .get::<i64>(0)
            .context("decode upserted loan workspace id")?;
        drop(rows);
        let workspace = workspace_by_id_on(&transaction, id)
            .await?
            .context("upserted loan workspace disappeared")?;
        transaction
            .commit()
            .await
            .context("commit loan workspace transaction")?;
        Ok(workspace)
    }

    pub async fn photo(
        &self,
        connection_id: i64,
        loan_account: &str,
        photo_id: i64,
    ) -> WorkspaceInboxResult<Option<LoanWorkspacePhoto>> {
        photo_on(self.connection, connection_id, loan_account, photo_id).await
    }

    pub async fn list_photos(
        &self,
        connection_id: i64,
        loan_account: &str,
    ) -> WorkspaceInboxResult<Vec<LoanWorkspacePhoto>> {
        photos_on(self.connection, connection_id, loan_account).await
    }

    pub async fn create_photo_metadata(
        &self,
        draft: &LoanWorkspacePhotoDraft,
    ) -> WorkspaceInboxResult<LoanWorkspacePhoto> {
        validate_photo(draft)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin photo metadata transaction")?;
        let mut rows = transaction
            .query(
                "INSERT OR IGNORE INTO intg_loan_workspace_photo ( \
                    connection_id, loan_account, provider, caption, source_url, image_url, \
                    sort_order, is_featured \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0) RETURNING id",
                params![
                    draft.connection_id,
                    draft.loan_account.clone(),
                    draft.provider.clone(),
                    draft.caption.clone(),
                    draft.source_url.clone(),
                    draft.image_url.clone(),
                    draft.sort_order,
                ],
            )
            .await
            .context("insert photo metadata")?;
        let inserted_id = rows
            .next()
            .await
            .context("read inserted photo metadata id")?
            .map(|row| {
                row.get::<i64>(0)
                    .context("decode inserted photo metadata id")
            })
            .transpose()?;
        drop(rows);
        let Some(id) = inserted_id else {
            let existing_id = photo_id_by_identity_on(
                &transaction,
                draft.connection_id,
                &draft.loan_account,
                &draft.provider,
                &draft.image_url,
            )
            .await?;
            return Err(WorkspaceInboxError::conflict(format!(
                "photo metadata already exists{}",
                existing_id
                    .map(|id| format!(" as row {id}"))
                    .unwrap_or_default()
            )));
        };
        let photo = photo_on(&transaction, draft.connection_id, &draft.loan_account, id)
            .await?
            .context("inserted photo metadata disappeared")?;
        transaction
            .commit()
            .await
            .context("commit photo metadata transaction")?;
        Ok(photo)
    }

    /// Finalize one direct browser upload. Replaying the same signed finalize
    /// request returns the existing row, while a new photo receives its sort
    /// order and row in one immediate transaction.
    pub async fn create_manual_photo_metadata(
        &self,
        connection_id: i64,
        loan_account: &str,
        caption: Option<&str>,
        image_url: &str,
    ) -> WorkspaceInboxResult<LoanWorkspacePhoto> {
        let draft = LoanWorkspacePhotoDraft {
            connection_id,
            loan_account: loan_account.to_owned(),
            provider: "manual".to_owned(),
            caption: caption.map(str::to_owned),
            source_url: "manual-upload".to_owned(),
            image_url: image_url.to_owned(),
            sort_order: 0,
        };
        validate_photo(&draft)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin manual photo metadata transaction")?;
        if let Some(id) = photo_id_by_identity_on(
            &transaction,
            connection_id,
            loan_account,
            "manual",
            image_url,
        )
        .await?
        {
            let photo = photo_on(&transaction, connection_id, loan_account, id)
                .await?
                .context("existing manual photo metadata disappeared")?;
            transaction
                .commit()
                .await
                .context("commit idempotent manual photo transaction")?;
            return Ok(photo);
        }
        let mut rows = transaction
            .query(
                "INSERT INTO intg_loan_workspace_photo ( \
                    connection_id, loan_account, provider, caption, source_url, image_url, \
                    sort_order, is_featured \
                 ) VALUES ( \
                    ?1, ?2, 'manual', ?3, 'manual-upload', ?4, \
                    (SELECT COALESCE(MAX(sort_order), -1) + 1 \
                     FROM intg_loan_workspace_photo \
                     WHERE connection_id = ?1 AND loan_account = ?2), \
                    0 \
                 ) RETURNING id",
                params![connection_id, loan_account, caption, image_url],
            )
            .await
            .context("insert manual photo metadata")?;
        let id = rows
            .next()
            .await
            .context("read manual photo metadata id")?
            .context("manual photo metadata insert returned no row")?
            .get::<i64>(0)
            .context("decode manual photo metadata id")?;
        drop(rows);
        let photo = photo_on(&transaction, connection_id, loan_account, id)
            .await?
            .context("inserted manual photo metadata disappeared")?;
        transaction
            .commit()
            .await
            .context("commit manual photo metadata transaction")?;
        Ok(photo)
    }

    /// Return exact imported photo locations for authenticated media-reference
    /// authorization. Canonicalization stays at the media boundary so this
    /// repository never rewrites historical object keys.
    pub async fn photo_image_locations(&self) -> WorkspaceInboxResult<Vec<String>> {
        let mut rows = self
            .connection
            .query(
                "SELECT image_url FROM intg_loan_workspace_photo ORDER BY id",
                (),
            )
            .await
            .context("query loan photo media locations")?;
        let mut locations = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .context("read loan photo media location")?
        {
            locations.push(
                row.get::<String>(0)
                    .context("decode loan photo media location")?,
            );
        }
        Ok(locations)
    }

    pub async fn email_object_is_referenced(&self, object_key: &str) -> WorkspaceInboxResult<bool> {
        let mut rows = self
            .connection
            .query(
                "SELECT EXISTS( \
                    SELECT 1 FROM intg_received_email WHERE body_s3_key = ?1 \
                    UNION ALL \
                    SELECT 1 FROM intg_received_email_attachment WHERE s3_key = ?1 \
                 )",
                params![object_key],
            )
            .await
            .context("query email object reference")?;
        let value = rows
            .next()
            .await
            .context("read email object reference")?
            .context("email object reference query returned no row")?
            .get::<i64>(0)
            .context("decode email object reference")?;
        match value {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WorkspaceInboxError::from(anyhow::anyhow!(
                "email object reference flag is not boolean"
            ))),
        }
    }

    pub async fn set_featured_photo(
        &self,
        connection_id: i64,
        loan_account: &str,
        photo_id: i64,
    ) -> WorkspaceInboxResult<LoanWorkspacePhoto> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin featured photo transaction")?;
        if photo_on(&transaction, connection_id, loan_account, photo_id)
            .await?
            .is_none()
        {
            return Err(WorkspaceInboxError::not_found(format!(
                "photo {photo_id} in workspace {loan_account}"
            )));
        }
        transaction
            .execute(
                "UPDATE intg_loan_workspace_photo SET is_featured = 0 \
                 WHERE connection_id = ?1 AND loan_account = ?2 AND is_featured = 1",
                params![connection_id, loan_account],
            )
            .await
            .context("clear prior featured photo")?;
        let changed = transaction
            .execute(
                "UPDATE intg_loan_workspace_photo SET is_featured = 1 \
                 WHERE id = ?3 AND connection_id = ?1 AND loan_account = ?2",
                params![connection_id, loan_account, photo_id],
            )
            .await
            .context("set featured photo")?;
        if changed != 1 {
            return Err(WorkspaceInboxError::conflict(
                "featured photo changed concurrently",
            ));
        }
        let photo = photo_on(&transaction, connection_id, loan_account, photo_id)
            .await?
            .context("featured photo disappeared")?;
        transaction
            .commit()
            .await
            .context("commit featured photo transaction")?;
        Ok(photo)
    }

    /// Delete only the metadata row and return it unchanged. The caller may
    /// use the returned URL/key at its separately configured object boundary.
    pub async fn delete_photo_metadata(
        &self,
        connection_id: i64,
        loan_account: &str,
        photo_id: i64,
    ) -> WorkspaceInboxResult<LoanWorkspacePhoto> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin delete photo metadata transaction")?;
        let photo = photo_on(&transaction, connection_id, loan_account, photo_id)
            .await?
            .ok_or_else(|| {
                WorkspaceInboxError::not_found(format!(
                    "photo {photo_id} in workspace {loan_account}"
                ))
            })?;
        let changed = transaction
            .execute(
                "DELETE FROM intg_loan_workspace_photo \
                 WHERE id = ?3 AND connection_id = ?1 AND loan_account = ?2",
                params![connection_id, loan_account, photo_id],
            )
            .await
            .context("delete photo metadata")?;
        if changed != 1 {
            return Err(WorkspaceInboxError::conflict(
                "photo metadata changed concurrently",
            ));
        }
        transaction
            .commit()
            .await
            .context("commit delete photo metadata transaction")?;
        Ok(photo)
    }

    pub async fn upsert_received_email(
        &self,
        draft: &ReceivedEmailDraft,
    ) -> WorkspaceInboxResult<EmailUpsert> {
        validate_email(draft)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin received email upsert transaction")?;
        let mut rows = transaction
            .query(
                "INSERT OR IGNORE INTO intg_received_email ( \
                    resend_email_id, from_address, to_addresses, subject, received_at, \
                    raw_webhook_payload \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
                params![
                    draft.resend_email_id.clone(),
                    draft.from_address.clone(),
                    draft.to_addresses.clone(),
                    draft.subject.clone(),
                    draft.received_at.clone(),
                    draft.raw_webhook_payload.clone(),
                ],
            )
            .await
            .context("upsert received email metadata")?;
        let inserted_id = rows
            .next()
            .await
            .context("read received email upsert result")?
            .map(|row| row.get::<i64>(0).context("decode inserted email id"))
            .transpose()?;
        drop(rows);
        let email = email_by_resend_id_on(&transaction, &draft.resend_email_id)
            .await?
            .context("received email upsert returned no row")?;
        transaction
            .commit()
            .await
            .context("commit received email upsert transaction")?;
        Ok(EmailUpsert {
            inserted: inserted_id.is_some(),
            email,
        })
    }

    pub async fn email(&self, email_id: i64) -> WorkspaceInboxResult<Option<ReceivedEmail>> {
        email_on(self.connection, email_id).await
    }

    pub async fn email_by_resend_id(
        &self,
        resend_email_id: &str,
    ) -> WorkspaceInboxResult<Option<ReceivedEmail>> {
        email_by_resend_id_on(self.connection, resend_email_id).await
    }

    pub async fn list_emails(
        &self,
        include_linked: bool,
    ) -> WorkspaceInboxResult<Vec<ReceivedEmail>> {
        let sql = if include_linked {
            "SELECT id, resend_email_id, from_address, to_addresses, subject, received_at, \
                    body_s3_key, body_content_type, loan_account, processing_state, \
                    error_message, raw_webhook_payload, created_at, updated_at \
             FROM intg_received_email ORDER BY created_at DESC, id DESC"
        } else {
            "SELECT id, resend_email_id, from_address, to_addresses, subject, received_at, \
                    body_s3_key, body_content_type, loan_account, processing_state, \
                    error_message, raw_webhook_payload, created_at, updated_at \
             FROM intg_received_email WHERE loan_account IS NULL \
             ORDER BY created_at DESC, id DESC"
        };
        let rows = self
            .connection
            .query(sql, ())
            .await
            .context("query received emails")?;
        collect(rows, email_from_row, "received email").await
    }

    pub async fn list_inbox_items(
        &self,
        include_linked: bool,
    ) -> WorkspaceInboxResult<Vec<InboxEmailListItem>> {
        let linked_predicate = if include_linked {
            ""
        } else {
            "WHERE email.loan_account IS NULL"
        };
        let sql = format!(
            "SELECT email.id, email.resend_email_id, email.from_address, email.to_addresses, \
                    email.subject, email.received_at, email.body_s3_key, \
                    email.body_content_type, email.loan_account, email.processing_state, \
                    email.error_message, email.raw_webhook_payload, email.created_at, \
                    email.updated_at, \
                    (SELECT COUNT(*) FROM intg_received_email_attachment attachment \
                     WHERE attachment.email_id = email.id) AS attachment_count \
             FROM intg_received_email email {linked_predicate} \
             ORDER BY email.created_at DESC, email.id DESC"
        );
        let rows = self
            .connection
            .query(&sql, ())
            .await
            .context("query inbox email list")?;
        collect(rows, inbox_item_from_row, "inbox email").await
    }

    pub async fn list_emails_for_loan(
        &self,
        loan_account: &str,
    ) -> WorkspaceInboxResult<Vec<ReceivedEmail>> {
        let rows = self
            .connection
            .query(
                "SELECT id, resend_email_id, from_address, to_addresses, subject, received_at, \
                        body_s3_key, body_content_type, loan_account, processing_state, \
                        error_message, raw_webhook_payload, created_at, updated_at \
                 FROM intg_received_email WHERE loan_account = ?1 \
                 ORDER BY received_at DESC, id DESC",
                params![loan_account],
            )
            .await
            .context("query received emails for loan")?;
        collect(rows, email_from_row, "received email").await
    }

    pub async fn email_detail(
        &self,
        email_id: i64,
    ) -> WorkspaceInboxResult<Option<ReceivedEmailDetail>> {
        let Some(email) = email_on(self.connection, email_id).await? else {
            return Ok(None);
        };
        let attachments = attachments_on(self.connection, email_id).await?;
        Ok(Some(ReceivedEmailDetail { email, attachments }))
    }

    pub async fn mark_email_body_stored(
        &self,
        email_id: i64,
        body_s3_key: &str,
        body_content_type: &str,
    ) -> WorkspaceInboxResult<ReceivedEmail> {
        validate_nonempty("body content type", body_content_type)?;
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email \
                 SET processing_state = 'stored', body_s3_key = ?2, body_content_type = ?3, \
                     error_message = NULL, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND processing_state = 'pending'",
                params![email_id, body_s3_key, body_content_type],
            )
            .await
            .context("mark email body stored")?;
        let email = required_email(self.connection, email_id).await?;
        if changed == 1
            || (email.processing_state == "stored"
                && email.body_s3_key.as_deref() == Some(body_s3_key)
                && email.body_content_type.as_deref() == Some(body_content_type))
        {
            return Ok(email);
        }
        Err(WorkspaceInboxError::conflict(format!(
            "email {email_id} cannot transition from {} to stored",
            email.processing_state
        )))
    }

    pub async fn mark_email_error(
        &self,
        email_id: i64,
        error_message: &str,
    ) -> WorkspaceInboxResult<ReceivedEmail> {
        validate_nonempty("email error message", error_message)?;
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email \
                 SET processing_state = 'error', error_message = ?2, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND processing_state = 'pending'",
                params![email_id, error_message],
            )
            .await
            .context("mark email error")?;
        let email = required_email(self.connection, email_id).await?;
        if changed == 1 || email.processing_state == "error" {
            return Ok(email);
        }
        Err(WorkspaceInboxError::conflict(format!(
            "email {email_id} cannot transition from {} to error",
            email.processing_state
        )))
    }

    pub async fn reset_email_for_retry(
        &self,
        email_id: i64,
    ) -> WorkspaceInboxResult<ReceivedEmail> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin email retry transaction")?;
        let current = required_email(&transaction, email_id).await?;
        if current.processing_state == "stored" {
            return Err(WorkspaceInboxError::conflict(format!(
                "stored email {email_id} does not need retry"
            )));
        }
        transaction
            .execute(
                "UPDATE intg_received_email \
                 SET processing_state = 'pending', error_message = NULL, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1",
                params![email_id],
            )
            .await
            .context("reset email for retry")?;
        transaction
            .execute(
                "UPDATE intg_received_email_attachment SET processing_state = 'pending' \
                 WHERE email_id = ?1 AND processing_state <> 'stored'",
                params![email_id],
            )
            .await
            .context("reset email attachments for retry")?;
        let email = required_email(&transaction, email_id).await?;
        transaction
            .commit()
            .await
            .context("commit email retry transaction")?;
        Ok(email)
    }

    pub async fn link_email(
        &self,
        email_id: i64,
        loan_account: &str,
    ) -> WorkspaceInboxResult<ReceivedEmail> {
        validate_nonempty("loan account", loan_account)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin link email transaction")?;
        let current = required_email(&transaction, email_id).await?;
        if let Some(existing) = current.loan_account.as_deref()
            && existing != loan_account
        {
            return Err(WorkspaceInboxError::conflict(format!(
                "email {email_id} is already linked to loan {existing}; unlink it before relinking"
            )));
        }
        transaction
            .execute(
                "UPDATE intg_received_email SET loan_account = ?2, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![email_id, loan_account],
            )
            .await
            .context("link email to loan")?;
        let email = required_email(&transaction, email_id).await?;
        transaction
            .commit()
            .await
            .context("commit link email transaction")?;
        Ok(email)
    }

    /// Link an inbox email only to a currently imported active TMO loan.
    ///
    /// The existence check, conflict check, and update share one immediate
    /// transaction. This is the browser-facing mutation; the lower-level
    /// `link_email` method remains available to provider/import code that has
    /// already established its own source invariant.
    pub async fn link_email_to_imported_tmo_loan(
        &self,
        email_id: i64,
        loan_account: &str,
    ) -> WorkspaceInboxResult<ReceivedEmail> {
        validate_email_id(email_id)?;
        validate_loan_account_input(loan_account)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin validated email link transaction")?;
        let current = required_email(&transaction, email_id).await?;
        if let Some(existing) = current.loan_account.as_deref()
            && existing != loan_account
        {
            return Err(WorkspaceInboxError::conflict(format!(
                "email {email_id} is already linked to another loan"
            )));
        }

        let mut rows = transaction
            .query(
                "SELECT loan.id \
                 FROM intg_tmo_import_loan loan \
                 JOIN intg_integration_connection connection \
                   ON connection.id = loan.connection_id \
                 WHERE connection.slug = 'tmo' \
                   AND connection.provider = 'mortgage_office' \
                   AND connection.status IN ('active', 'degraded', 'error') \
                   AND loan.is_active = 1 \
                   AND loan.loan_account = ?1 \
                 LIMIT 1",
                params![loan_account],
            )
            .await
            .context("validate imported TMO loan for email link")?;
        let loan_exists = rows
            .next()
            .await
            .context("read imported TMO loan validation")?
            .is_some();
        drop(rows);
        if !loan_exists {
            return Err(WorkspaceInboxError::not_found("active imported TMO loan"));
        }

        let changed = transaction
            .execute(
                "UPDATE intg_received_email SET loan_account = ?2, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1",
                params![email_id, loan_account],
            )
            .await
            .context("link email to validated imported TMO loan")?;
        if changed != 1 {
            return Err(WorkspaceInboxError::conflict(
                "email changed while it was being linked",
            ));
        }
        let email = required_email(&transaction, email_id).await?;
        transaction
            .commit()
            .await
            .context("commit validated email link transaction")?;
        Ok(email)
    }

    pub async fn unlink_email(&self, email_id: i64) -> WorkspaceInboxResult<ReceivedEmail> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin unlink email transaction")?;
        required_email(&transaction, email_id).await?;
        transaction
            .execute(
                "UPDATE intg_received_email SET loan_account = NULL, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![email_id],
            )
            .await
            .context("unlink email from loan")?;
        let email = required_email(&transaction, email_id).await?;
        transaction
            .commit()
            .await
            .context("commit unlink email transaction")?;
        Ok(email)
    }

    pub async fn create_attachment_metadata(
        &self,
        draft: &ReceivedEmailAttachmentDraft,
    ) -> WorkspaceInboxResult<AttachmentUpsert> {
        validate_attachment(draft)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin attachment upsert transaction")?;
        if email_on(&transaction, draft.email_id).await?.is_none() {
            return Err(WorkspaceInboxError::not_found(format!(
                "email {}",
                draft.email_id
            )));
        }
        let mut rows = transaction
            .query(
                "INSERT OR IGNORE INTO intg_received_email_attachment ( \
                    email_id, resend_attachment_id, filename, content_type \
                 ) VALUES (?1, ?2, ?3, ?4) RETURNING id",
                params![
                    draft.email_id,
                    draft.resend_attachment_id.clone(),
                    draft.filename.clone(),
                    draft.content_type.clone(),
                ],
            )
            .await
            .context("upsert attachment metadata")?;
        let inserted_id = rows
            .next()
            .await
            .context("read attachment upsert result")?
            .map(|row| row.get::<i64>(0).context("decode inserted attachment id"))
            .transpose()?;
        drop(rows);
        let attachment =
            attachment_by_provider_id_on(&transaction, draft.email_id, &draft.resend_attachment_id)
                .await?
                .context("attachment upsert returned no row")?;
        transaction
            .commit()
            .await
            .context("commit attachment upsert transaction")?;
        Ok(AttachmentUpsert {
            inserted: inserted_id.is_some(),
            attachment,
        })
    }

    pub async fn list_attachments(
        &self,
        email_id: i64,
    ) -> WorkspaceInboxResult<Vec<ReceivedEmailAttachment>> {
        attachments_on(self.connection, email_id).await
    }

    pub async fn mark_attachment_stored(
        &self,
        attachment_id: i64,
        s3_key: &str,
        size_bytes: i64,
    ) -> WorkspaceInboxResult<ReceivedEmailAttachment> {
        validate_nonempty("attachment storage key", s3_key)?;
        if size_bytes < 0 {
            return Err(WorkspaceInboxError::validation(
                "attachment size cannot be negative",
            ));
        }
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email_attachment \
                 SET processing_state = 'stored', s3_key = ?2, size_bytes = ?3 \
                 WHERE id = ?1 AND processing_state = 'pending'",
                params![attachment_id, s3_key, size_bytes],
            )
            .await
            .context("mark attachment stored")?;
        let attachment = required_attachment(self.connection, attachment_id).await?;
        if changed == 1
            || (attachment.processing_state == "stored"
                && attachment.s3_key.as_deref() == Some(s3_key)
                && attachment.size_bytes == Some(size_bytes))
        {
            return Ok(attachment);
        }
        Err(WorkspaceInboxError::conflict(format!(
            "attachment {attachment_id} cannot transition from {} to stored",
            attachment.processing_state
        )))
    }

    pub async fn mark_attachment_error(
        &self,
        attachment_id: i64,
    ) -> WorkspaceInboxResult<ReceivedEmailAttachment> {
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email_attachment SET processing_state = 'error' \
                 WHERE id = ?1 AND processing_state = 'pending'",
                params![attachment_id],
            )
            .await
            .context("mark attachment error")?;
        let attachment = required_attachment(self.connection, attachment_id).await?;
        if changed == 1 || attachment.processing_state == "error" {
            return Ok(attachment);
        }
        Err(WorkspaceInboxError::conflict(format!(
            "attachment {attachment_id} cannot transition from {} to error",
            attachment.processing_state
        )))
    }

    pub async fn reset_attachment_for_retry(
        &self,
        attachment_id: i64,
    ) -> WorkspaceInboxResult<ReceivedEmailAttachment> {
        let attachment = required_attachment(self.connection, attachment_id).await?;
        if attachment.processing_state == "stored" {
            return Err(WorkspaceInboxError::conflict(format!(
                "stored attachment {attachment_id} does not need retry"
            )));
        }
        self.connection
            .execute(
                "UPDATE intg_received_email_attachment SET processing_state = 'pending' \
                 WHERE id = ?1",
                params![attachment_id],
            )
            .await
            .context("reset attachment for retry")?;
        required_attachment(self.connection, attachment_id).await
    }

    pub async fn delete_email_metadata(
        &self,
        email_id: i64,
    ) -> WorkspaceInboxResult<DeletedEmailMetadata> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin delete email metadata transaction")?;
        let email = required_email(&transaction, email_id).await?;
        let attachments = attachments_on(&transaction, email_id).await?;
        let changed = transaction
            .execute(
                "DELETE FROM intg_received_email WHERE id = ?1",
                params![email_id],
            )
            .await
            .context("delete email metadata")?;
        if changed != 1 {
            return Err(WorkspaceInboxError::conflict(
                "email metadata changed concurrently",
            ));
        }
        transaction
            .commit()
            .await
            .context("commit delete email metadata transaction")?;
        Ok(DeletedEmailMetadata { email, attachments })
    }
}

async fn workspace_on(
    connection: &Connection,
    connection_id: i64,
    loan_account: &str,
) -> WorkspaceInboxResult<Option<LoanWorkspace>> {
    let rows = connection
        .query(
            "SELECT id, connection_id, loan_account, redfin_url, zillow_url, \
                    decision_status, target_contribution, actual_contribution, notes, \
                    created_at, updated_at \
             FROM intg_loan_workspace \
             WHERE connection_id = ?1 AND loan_account = ?2 LIMIT 1",
            params![connection_id, loan_account],
        )
        .await
        .context("query loan workspace")?;
    one(rows, workspace_from_row, "loan workspace").await
}

async fn workspace_by_id_on(
    connection: &Connection,
    id: i64,
) -> WorkspaceInboxResult<Option<LoanWorkspace>> {
    let rows = connection
        .query(
            "SELECT id, connection_id, loan_account, redfin_url, zillow_url, \
                    decision_status, target_contribution, actual_contribution, notes, \
                    created_at, updated_at \
             FROM intg_loan_workspace WHERE id = ?1 LIMIT 1",
            params![id],
        )
        .await
        .context("query loan workspace by id")?;
    one(rows, workspace_from_row, "loan workspace").await
}

async fn photos_on(
    connection: &Connection,
    connection_id: i64,
    loan_account: &str,
) -> WorkspaceInboxResult<Vec<LoanWorkspacePhoto>> {
    let rows = connection
        .query(
            "SELECT id, connection_id, loan_account, provider, caption, source_url, image_url, \
                    sort_order, is_featured, created_at \
             FROM intg_loan_workspace_photo \
             WHERE connection_id = ?1 AND loan_account = ?2 \
             ORDER BY is_featured DESC, sort_order, id",
            params![connection_id, loan_account],
        )
        .await
        .context("query loan workspace photos")?;
    collect(rows, photo_from_row, "loan workspace photo").await
}

async fn photo_on(
    connection: &Connection,
    connection_id: i64,
    loan_account: &str,
    photo_id: i64,
) -> WorkspaceInboxResult<Option<LoanWorkspacePhoto>> {
    let rows = connection
        .query(
            "SELECT id, connection_id, loan_account, provider, caption, source_url, image_url, \
                    sort_order, is_featured, created_at \
             FROM intg_loan_workspace_photo \
             WHERE id = ?3 AND connection_id = ?1 AND loan_account = ?2 LIMIT 1",
            params![connection_id, loan_account, photo_id],
        )
        .await
        .context("query loan workspace photo")?;
    one(rows, photo_from_row, "loan workspace photo").await
}

async fn photo_id_by_identity_on(
    connection: &Connection,
    connection_id: i64,
    loan_account: &str,
    provider: &str,
    image_url: &str,
) -> WorkspaceInboxResult<Option<i64>> {
    let mut rows = connection
        .query(
            "SELECT id FROM intg_loan_workspace_photo \
             WHERE connection_id = ?1 AND loan_account = ?2 AND provider = ?3 AND image_url = ?4 \
             LIMIT 1",
            params![connection_id, loan_account, provider, image_url],
        )
        .await
        .context("query existing photo metadata")?;
    let id = rows
        .next()
        .await
        .context("read existing photo metadata")?
        .map(|row| row.get::<i64>(0).context("decode existing photo id"))
        .transpose()?;
    Ok(id)
}

async fn email_on(
    connection: &Connection,
    email_id: i64,
) -> WorkspaceInboxResult<Option<ReceivedEmail>> {
    let rows = connection
        .query(
            "SELECT id, resend_email_id, from_address, to_addresses, subject, received_at, \
                    body_s3_key, body_content_type, loan_account, processing_state, \
                    error_message, raw_webhook_payload, created_at, updated_at \
             FROM intg_received_email WHERE id = ?1 LIMIT 1",
            params![email_id],
        )
        .await
        .context("query received email")?;
    one(rows, email_from_row, "received email").await
}

async fn email_by_resend_id_on(
    connection: &Connection,
    resend_email_id: &str,
) -> WorkspaceInboxResult<Option<ReceivedEmail>> {
    let rows = connection
        .query(
            "SELECT id, resend_email_id, from_address, to_addresses, subject, received_at, \
                    body_s3_key, body_content_type, loan_account, processing_state, \
                    error_message, raw_webhook_payload, created_at, updated_at \
             FROM intg_received_email WHERE resend_email_id = ?1 LIMIT 1",
            params![resend_email_id],
        )
        .await
        .context("query received email by provider id")?;
    one(rows, email_from_row, "received email").await
}

async fn required_email(
    connection: &Connection,
    email_id: i64,
) -> WorkspaceInboxResult<ReceivedEmail> {
    email_on(connection, email_id)
        .await?
        .ok_or_else(|| WorkspaceInboxError::not_found(format!("email {email_id}")))
}

async fn attachments_on(
    connection: &Connection,
    email_id: i64,
) -> WorkspaceInboxResult<Vec<ReceivedEmailAttachment>> {
    let rows = connection
        .query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state, created_at \
             FROM intg_received_email_attachment WHERE email_id = ?1 ORDER BY id",
            params![email_id],
        )
        .await
        .context("query received email attachments")?;
    collect(rows, attachment_from_row, "received email attachment").await
}

async fn attachment_by_provider_id_on(
    connection: &Connection,
    email_id: i64,
    resend_attachment_id: &str,
) -> WorkspaceInboxResult<Option<ReceivedEmailAttachment>> {
    let rows = connection
        .query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state, created_at \
             FROM intg_received_email_attachment \
             WHERE email_id = ?1 AND resend_attachment_id = ?2 LIMIT 1",
            params![email_id, resend_attachment_id],
        )
        .await
        .context("query received email attachment by provider id")?;
    one(rows, attachment_from_row, "received email attachment").await
}

async fn required_attachment(
    connection: &Connection,
    attachment_id: i64,
) -> WorkspaceInboxResult<ReceivedEmailAttachment> {
    let rows = connection
        .query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state, created_at \
             FROM intg_received_email_attachment WHERE id = ?1 LIMIT 1",
            params![attachment_id],
        )
        .await
        .context("query required received email attachment")?;
    one(rows, attachment_from_row, "received email attachment")
        .await?
        .ok_or_else(|| WorkspaceInboxError::not_found(format!("attachment {attachment_id}")))
}

async fn collect<T>(
    mut rows: Rows,
    decode: fn(&Row) -> anyhow::Result<T>,
    label: &'static str,
) -> WorkspaceInboxResult<Vec<T>> {
    let mut values = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .with_context(|| format!("read {label} row"))?
    {
        values.push(decode(&row)?);
    }
    Ok(values)
}

async fn one<T>(
    mut rows: Rows,
    decode: fn(&Row) -> anyhow::Result<T>,
    label: &'static str,
) -> WorkspaceInboxResult<Option<T>> {
    let Some(row) = rows
        .next()
        .await
        .with_context(|| format!("read {label} row"))?
    else {
        return Ok(None);
    };
    Ok(Some(decode(&row)?))
}

fn workspace_from_row(row: &Row) -> anyhow::Result<LoanWorkspace> {
    Ok(LoanWorkspace {
        id: row.get(0).context("decode workspace id")?,
        connection_id: row.get(1).context("decode workspace connection id")?,
        loan_account: row.get(2).context("decode workspace loan account")?,
        redfin_url: row.get(3).context("decode workspace Redfin URL")?,
        zillow_url: row.get(4).context("decode workspace Zillow URL")?,
        decision_status: row.get(5).context("decode workspace decision status")?,
        target_contribution: row.get(6).context("decode workspace target contribution")?,
        actual_contribution: row.get(7).context("decode workspace actual contribution")?,
        notes: row.get(8).context("decode workspace notes")?,
        created_at: row.get(9).context("decode workspace creation time")?,
        updated_at: row.get(10).context("decode workspace update time")?,
    })
}

fn photo_from_row(row: &Row) -> anyhow::Result<LoanWorkspacePhoto> {
    let is_featured = row.get::<i64>(8).context("decode photo featured flag")?;
    if !matches!(is_featured, 0 | 1) {
        anyhow::bail!("photo featured flag must be 0 or 1, got {is_featured}");
    }
    Ok(LoanWorkspacePhoto {
        id: row.get(0).context("decode photo id")?,
        connection_id: row.get(1).context("decode photo connection id")?,
        loan_account: row.get(2).context("decode photo loan account")?,
        provider: row.get(3).context("decode photo provider")?,
        caption: row.get(4).context("decode photo caption")?,
        source_url: row.get(5).context("decode photo source URL")?,
        image_url: row.get(6).context("decode photo image URL")?,
        sort_order: row.get(7).context("decode photo sort order")?,
        is_featured: is_featured == 1,
        created_at: row.get(9).context("decode photo creation time")?,
    })
}

fn email_from_row(row: &Row) -> anyhow::Result<ReceivedEmail> {
    let processing_state = row
        .get::<String>(9)
        .context("decode email processing state")?;
    if !matches!(processing_state.as_str(), "pending" | "stored" | "error") {
        anyhow::bail!("unknown email processing state");
    }
    Ok(ReceivedEmail {
        id: row.get(0).context("decode email id")?,
        resend_email_id: row.get(1).context("decode email provider id")?,
        from_address: row.get(2).context("decode email sender")?,
        to_addresses: row.get(3).context("decode email recipients")?,
        subject: row.get(4).context("decode email subject")?,
        received_at: row.get(5).context("decode email received time")?,
        body_s3_key: row.get(6).context("decode email body storage key")?,
        body_content_type: row.get(7).context("decode email body content type")?,
        loan_account: row.get(8).context("decode email loan account")?,
        processing_state,
        error_message: row.get(10).context("decode email error message")?,
        raw_webhook_payload: row.get(11).context("decode raw email webhook payload")?,
        created_at: row.get(12).context("decode email creation time")?,
        updated_at: row.get(13).context("decode email update time")?,
    })
}

fn inbox_item_from_row(row: &Row) -> anyhow::Result<InboxEmailListItem> {
    let attachment_count = row.get::<i64>(14).context("decode attachment count")?;
    if attachment_count < 0 {
        anyhow::bail!("attachment count cannot be negative");
    }
    Ok(InboxEmailListItem {
        email: email_from_row(row)?,
        attachment_count,
    })
}

fn attachment_from_row(row: &Row) -> anyhow::Result<ReceivedEmailAttachment> {
    let processing_state = row
        .get::<String>(7)
        .context("decode attachment processing state")?;
    if !matches!(processing_state.as_str(), "pending" | "stored" | "error") {
        anyhow::bail!("unknown attachment processing state");
    }
    Ok(ReceivedEmailAttachment {
        id: row.get(0).context("decode attachment id")?,
        email_id: row.get(1).context("decode attachment email id")?,
        resend_attachment_id: row.get(2).context("decode attachment provider id")?,
        filename: row.get(3).context("decode attachment filename")?,
        content_type: row.get(4).context("decode attachment content type")?,
        size_bytes: row.get(5).context("decode attachment size")?,
        s3_key: row.get(6).context("decode attachment storage key")?,
        processing_state,
        created_at: row.get(8).context("decode attachment creation time")?,
    })
}

fn validate_workspace(draft: &LoanWorkspaceDraft) -> WorkspaceInboxResult<()> {
    if draft.connection_id <= 0 {
        return Err(WorkspaceInboxError::validation(
            "connection id must be a positive integer",
        ));
    }
    validate_loan_account_input(&draft.loan_account)?;
    validate_workspace_url("Redfin URL", draft.redfin_url.as_deref())?;
    validate_workspace_url("Zillow URL", draft.zillow_url.as_deref())?;
    if let Some(status) = draft.decision_status.as_deref()
        && !matches!(
            status,
            "new" | "reviewing" | "committed" | "funded" | "passed"
        )
    {
        return Err(WorkspaceInboxError::validation(format!(
            "unknown workspace decision status {status}"
        )));
    }
    validate_optional_nonnegative("target contribution", draft.target_contribution)?;
    validate_optional_nonnegative("actual contribution", draft.actual_contribution)?;
    if draft
        .notes
        .as_ref()
        .is_some_and(|notes| notes.len() > 20_000)
    {
        return Err(WorkspaceInboxError::validation(
            "workspace notes are too long",
        ));
    }
    Ok(())
}

fn validate_photo(draft: &LoanWorkspacePhotoDraft) -> WorkspaceInboxResult<()> {
    if draft.connection_id <= 0 {
        return Err(WorkspaceInboxError::validation(
            "connection id must be a positive integer",
        ));
    }
    validate_loan_account_input(&draft.loan_account)?;
    validate_nonempty("photo provider", &draft.provider)?;
    validate_nonempty("photo source URL", &draft.source_url)?;
    validate_nonempty("photo image URL", &draft.image_url)?;
    if draft.provider.len() > 64
        || draft
            .caption
            .as_ref()
            .is_some_and(|value| value.len() > 512)
        || draft.source_url.len() > 2_048
        || draft.image_url.len() > 2_048
        || draft.sort_order < 0
    {
        return Err(WorkspaceInboxError::validation(
            "photo metadata exceeds its supported bounds",
        ));
    }
    Ok(())
}

fn validate_email(draft: &ReceivedEmailDraft) -> WorkspaceInboxResult<()> {
    validate_nonempty("provider email id", &draft.resend_email_id)?;
    validate_nonempty("email sender", &draft.from_address)?;
    let recipients: serde_json::Value =
        serde_json::from_str(&draft.to_addresses).map_err(|error| {
            WorkspaceInboxError::validation(format!("invalid recipients JSON: {error}"))
        })?;
    if !recipients.is_array() {
        return Err(WorkspaceInboxError::validation(
            "email recipients JSON must be an array",
        ));
    }
    chrono::DateTime::parse_from_rfc3339(&draft.received_at).map_err(|error| {
        WorkspaceInboxError::validation(format!("invalid received timestamp: {error}"))
    })?;
    if let Some(payload) = draft.raw_webhook_payload.as_deref() {
        serde_json::from_str::<serde_json::Value>(payload).map_err(|error| {
            WorkspaceInboxError::validation(format!("invalid raw webhook JSON: {error}"))
        })?;
    }
    Ok(())
}

fn validate_attachment(draft: &ReceivedEmailAttachmentDraft) -> WorkspaceInboxResult<()> {
    validate_nonempty("provider attachment id", &draft.resend_attachment_id)?;
    validate_nonempty("attachment filename", &draft.filename)?;
    validate_nonempty("attachment content type", &draft.content_type)
}

fn validate_nonempty(label: &str, value: &str) -> WorkspaceInboxResult<()> {
    if value.trim().is_empty() {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} cannot be empty"
        )));
    }
    Ok(())
}

fn validate_email_id(email_id: i64) -> WorkspaceInboxResult<()> {
    if email_id <= 0 {
        return Err(WorkspaceInboxError::validation(
            "email id must be a positive integer",
        ));
    }
    Ok(())
}

fn validate_loan_account_input(loan_account: &str) -> WorkspaceInboxResult<()> {
    validate_nonempty("loan account", loan_account)?;
    if loan_account.len() > MAX_LOAN_ACCOUNT_BYTES {
        return Err(WorkspaceInboxError::validation("loan account is too long"));
    }
    if loan_account.trim() != loan_account || loan_account.chars().any(char::is_control) {
        return Err(WorkspaceInboxError::validation(
            "loan account contains unsupported whitespace or control characters",
        ));
    }
    Ok(())
}

fn validate_optional_finite(label: &str, value: Option<f64>) -> WorkspaceInboxResult<()> {
    if value.is_some_and(|value| !value.is_finite()) {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} must be finite"
        )));
    }
    Ok(())
}

fn validate_optional_nonnegative(label: &str, value: Option<f64>) -> WorkspaceInboxResult<()> {
    validate_optional_finite(label, value)?;
    if value.is_some_and(|value| value < 0.0) {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} cannot be negative"
        )));
    }
    Ok(())
}

fn validate_workspace_url(label: &str, value: Option<&str>) -> WorkspaceInboxResult<()> {
    let Some(value) = value else {
        return Ok(());
    };
    if value.len() > 2_048 {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} is too long"
        )));
    }
    let parsed = url::Url::parse(value)
        .map_err(|_| WorkspaceInboxError::validation(format!("{label} is invalid")))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} must be an HTTP or HTTPS URL without credentials"
        )));
    }
    Ok(())
}
