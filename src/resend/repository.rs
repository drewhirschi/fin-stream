use anyhow::Context;
use libsql::{Connection, Row, TransactionBehavior, params};

use crate::workspace_inbox::{
    ReceivedEmail, ReceivedEmailAttachment, WorkspaceInboxError, WorkspaceInboxResult,
};

const LEASE_STALE_SQL: &str = "-2 minutes";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundAttachmentDraft {
    pub resend_attachment_id: String,
    pub filename: String,
    pub content_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundEmailDraft {
    pub resend_email_id: String,
    pub from_address: String,
    pub to_addresses: String,
    pub subject: Option<String>,
    pub received_at: String,
    pub raw_webhook_payload: String,
    pub attachments: Vec<InboundAttachmentDraft>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimedEmail {
    pub email: ReceivedEmail,
    pub attachments: Vec<ReceivedEmailAttachment>,
    pub lease_token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaimOutcome {
    Acquired(ClaimedEmail),
    AlreadyStored(ReceivedEmail),
    Busy(ReceivedEmail),
}

pub struct InboundEmailRepository<'connection> {
    connection: &'connection Connection,
}

impl<'connection> InboundEmailRepository<'connection> {
    pub fn new(connection: &'connection Connection) -> Self {
        Self { connection }
    }

    pub async fn claim_webhook(
        &self,
        draft: &InboundEmailDraft,
        lease_token: &str,
    ) -> WorkspaceInboxResult<ClaimOutcome> {
        validate_email_draft(draft)?;
        validate_lease_token(lease_token)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin inbound email claim transaction")?;
        transaction
            .execute(
                "INSERT OR IGNORE INTO intg_received_email ( \
                    resend_email_id, from_address, to_addresses, subject, received_at, \
                    raw_webhook_payload \
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
            .context("insert inbound email metadata")?;
        let email = email_by_resend_id(&transaction, &draft.resend_email_id)
            .await?
            .context("inserted inbound email metadata disappeared")?;
        if email.from_address != draft.from_address
            || email.to_addresses.as_bytes() != draft.to_addresses.as_bytes()
            || email.subject != draft.subject
            || email.received_at != draft.received_at
        {
            return Err(WorkspaceInboxError::conflict(
                "provider email id was replayed with different immutable metadata",
            ));
        }
        for attachment in &draft.attachments {
            upsert_attachment(&transaction, email.id, attachment).await?;
        }
        let outcome = claim_on(&transaction, email, lease_token).await?;
        transaction
            .commit()
            .await
            .context("commit inbound email claim transaction")?;
        Ok(outcome)
    }

    pub async fn claim_retry(
        &self,
        email_id: i64,
        lease_token: &str,
    ) -> WorkspaceInboxResult<ClaimOutcome> {
        if email_id <= 0 {
            return Err(WorkspaceInboxError::validation("email id must be positive"));
        }
        validate_lease_token(lease_token)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin inbound email retry claim transaction")?;
        let email = email_by_id(&transaction, email_id)
            .await?
            .ok_or_else(|| WorkspaceInboxError::not_found("inbox email"))?;
        let outcome = claim_on(&transaction, email, lease_token).await?;
        transaction
            .commit()
            .await
            .context("commit inbound email retry claim transaction")?;
        Ok(outcome)
    }

    pub async fn record_body(
        &self,
        email_id: i64,
        lease_token: &str,
        object_key: Option<&str>,
        content_type: &str,
    ) -> WorkspaceInboxResult<()> {
        validate_lease_token(lease_token)?;
        validate_content_type(content_type)?;
        if object_key.is_some_and(|key| key.is_empty() || key.len() > 1_024) {
            return Err(WorkspaceInboxError::validation(
                "email body object key is invalid",
            ));
        }
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email \
                 SET body_s3_key = ?3, body_content_type = ?4, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND processing_state = 'pending' \
                   AND EXISTS ( \
                       SELECT 1 FROM intg_received_email_processing_lease lease \
                       WHERE lease.email_id = ?1 AND lease.lease_token = ?2 \
                   )",
                params![email_id, lease_token, object_key, content_type],
            )
            .await
            .context("record inbound email body object")?;
        if changed == 1 {
            Ok(())
        } else {
            Err(WorkspaceInboxError::conflict(
                "inbound email processing lease is no longer owned",
            ))
        }
    }

    pub async fn mark_attachment_stored(
        &self,
        email_id: i64,
        attachment_id: i64,
        lease_token: &str,
        object_key: &str,
        size_bytes: i64,
    ) -> WorkspaceInboxResult<()> {
        validate_lease_token(lease_token)?;
        if object_key.is_empty() || object_key.len() > 1_024 || size_bytes < 0 {
            return Err(WorkspaceInboxError::validation(
                "stored attachment metadata is invalid",
            ));
        }
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email_attachment \
                 SET processing_state = 'stored', s3_key = ?4, size_bytes = ?5 \
                 WHERE id = ?2 AND email_id = ?1 AND processing_state <> 'stored' \
                   AND EXISTS ( \
                       SELECT 1 FROM intg_received_email_processing_lease lease \
                       WHERE lease.email_id = ?1 AND lease.lease_token = ?3 \
                   )",
                params![email_id, attachment_id, lease_token, object_key, size_bytes],
            )
            .await
            .context("record inbound email attachment object")?;
        if changed == 1 {
            return Ok(());
        }
        if !lease_is_owned(self.connection, email_id, lease_token).await? {
            return Err(WorkspaceInboxError::conflict(
                "inbound email processing lease is no longer owned",
            ));
        }
        let attachment = attachment_by_id(self.connection, attachment_id)
            .await?
            .ok_or_else(|| WorkspaceInboxError::not_found("inbox email attachment"))?;
        if attachment.email_id == email_id
            && attachment.processing_state == "stored"
            && attachment.s3_key.as_deref() == Some(object_key)
            && attachment.size_bytes == Some(size_bytes)
        {
            Ok(())
        } else {
            Err(WorkspaceInboxError::conflict(
                "stored attachment metadata does not match this processing attempt",
            ))
        }
    }

    pub async fn mark_attachment_error(
        &self,
        email_id: i64,
        attachment_id: i64,
        lease_token: &str,
    ) -> WorkspaceInboxResult<()> {
        validate_lease_token(lease_token)?;
        let changed = self
            .connection
            .execute(
                "UPDATE intg_received_email_attachment SET processing_state = 'error' \
                 WHERE id = ?2 AND email_id = ?1 AND processing_state = 'pending' \
                   AND EXISTS ( \
                       SELECT 1 FROM intg_received_email_processing_lease lease \
                       WHERE lease.email_id = ?1 AND lease.lease_token = ?3 \
                   )",
                params![email_id, attachment_id, lease_token],
            )
            .await
            .context("mark inbound email attachment failed")?;
        if changed == 1 {
            Ok(())
        } else {
            Err(WorkspaceInboxError::conflict(
                "inbound email attachment or processing lease changed before failure",
            ))
        }
    }

    pub async fn complete(
        &self,
        email_id: i64,
        lease_token: &str,
    ) -> WorkspaceInboxResult<ReceivedEmail> {
        validate_lease_token(lease_token)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin inbound email completion transaction")?;
        require_owned_lease(&transaction, email_id, lease_token).await?;
        let mut rows = transaction
            .query(
                "SELECT COUNT(*) FROM intg_received_email_attachment \
                 WHERE email_id = ?1 AND processing_state <> 'stored'",
                params![email_id],
            )
            .await
            .context("count incomplete inbound email attachments")?;
        let incomplete = rows
            .next()
            .await
            .context("read incomplete inbound email attachment count")?
            .context("attachment count query returned no row")?
            .get::<i64>(0)
            .context("decode incomplete attachment count")?;
        drop(rows);
        if incomplete != 0 {
            return Err(WorkspaceInboxError::conflict(
                "inbound email still has incomplete attachments",
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE intg_received_email \
                 SET processing_state = 'stored', error_message = NULL, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND processing_state = 'pending'",
                params![email_id],
            )
            .await
            .context("complete inbound email")?;
        if changed != 1 {
            return Err(WorkspaceInboxError::conflict(
                "inbound email state changed before completion",
            ));
        }
        delete_owned_lease(&transaction, email_id, lease_token).await?;
        let email = email_by_id(&transaction, email_id)
            .await?
            .context("completed inbound email disappeared")?;
        transaction
            .commit()
            .await
            .context("commit inbound email completion transaction")?;
        Ok(email)
    }

    pub async fn fail(
        &self,
        email_id: i64,
        lease_token: &str,
        public_error: &str,
    ) -> WorkspaceInboxResult<()> {
        validate_lease_token(lease_token)?;
        if public_error.trim().is_empty()
            || public_error.len() > 512
            || public_error.chars().any(char::is_control)
        {
            return Err(WorkspaceInboxError::validation(
                "inbound email error summary is invalid",
            ));
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .context("begin inbound email failure transaction")?;
        require_owned_lease(&transaction, email_id, lease_token).await?;
        let changed = transaction
            .execute(
                "UPDATE intg_received_email \
                 SET processing_state = 'error', error_message = ?2, \
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
                 WHERE id = ?1 AND processing_state = 'pending'",
                params![email_id, public_error],
            )
            .await
            .context("mark inbound email failed")?;
        if changed != 1 {
            return Err(WorkspaceInboxError::conflict(
                "inbound email state changed before failure was recorded",
            ));
        }
        delete_owned_lease(&transaction, email_id, lease_token).await?;
        transaction
            .commit()
            .await
            .context("commit inbound email failure transaction")?;
        Ok(())
    }
}

async fn claim_on(
    connection: &Connection,
    email: ReceivedEmail,
    lease_token: &str,
) -> WorkspaceInboxResult<ClaimOutcome> {
    if email.processing_state == "stored" {
        return Ok(ClaimOutcome::AlreadyStored(email));
    }
    connection
        .execute(
            "DELETE FROM intg_received_email_processing_lease \
             WHERE email_id = ?1 \
               AND claimed_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?2)",
            params![email.id, LEASE_STALE_SQL],
        )
        .await
        .context("expire stale inbound email processing lease")?;
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO intg_received_email_processing_lease ( \
                email_id, lease_token, claimed_at \
             ) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![email.id, lease_token],
        )
        .await
        .context("claim inbound email processing lease")?;
    if inserted == 0 {
        return Ok(ClaimOutcome::Busy(email));
    }
    connection
        .execute(
            "UPDATE intg_received_email \
             SET processing_state = 'pending', error_message = NULL, \
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') \
             WHERE id = ?1 AND processing_state IN ('pending', 'error')",
            params![email.id],
        )
        .await
        .context("start inbound email processing attempt")?;
    connection
        .execute(
            "UPDATE intg_received_email_attachment SET processing_state = 'pending' \
             WHERE email_id = ?1 AND processing_state <> 'stored'",
            params![email.id],
        )
        .await
        .context("reset incomplete inbound email attachments")?;
    let email = email_by_id(connection, email.id)
        .await?
        .context("claimed inbound email disappeared")?;
    let attachments = attachments(connection, email.id).await?;
    Ok(ClaimOutcome::Acquired(ClaimedEmail {
        email,
        attachments,
        lease_token: lease_token.to_owned(),
    }))
}

async fn upsert_attachment(
    connection: &Connection,
    email_id: i64,
    draft: &InboundAttachmentDraft,
) -> WorkspaceInboxResult<()> {
    validate_attachment_draft(draft)?;
    connection
        .execute(
            "INSERT OR IGNORE INTO intg_received_email_attachment ( \
                email_id, resend_attachment_id, filename, content_type \
             ) VALUES (?1, ?2, ?3, ?4)",
            params![
                email_id,
                draft.resend_attachment_id.clone(),
                draft.filename.clone(),
                draft.content_type.clone()
            ],
        )
        .await
        .context("insert inbound email attachment metadata")?;
    let attachment = attachment_by_provider_id(connection, email_id, &draft.resend_attachment_id)
        .await?
        .context("inserted inbound email attachment disappeared")?;
    if attachment.filename != draft.filename || attachment.content_type != draft.content_type {
        return Err(WorkspaceInboxError::conflict(
            "provider attachment id was replayed with different immutable metadata",
        ));
    }
    Ok(())
}

async fn require_owned_lease(
    connection: &Connection,
    email_id: i64,
    lease_token: &str,
) -> WorkspaceInboxResult<()> {
    if lease_is_owned(connection, email_id, lease_token).await? {
        Ok(())
    } else {
        Err(WorkspaceInboxError::conflict(
            "inbound email processing lease is no longer owned",
        ))
    }
}

async fn delete_owned_lease(
    connection: &Connection,
    email_id: i64,
    lease_token: &str,
) -> WorkspaceInboxResult<()> {
    let deleted = connection
        .execute(
            "DELETE FROM intg_received_email_processing_lease \
             WHERE email_id = ?1 AND lease_token = ?2",
            params![email_id, lease_token],
        )
        .await
        .context("release inbound email processing lease")?;
    if deleted == 1 {
        Ok(())
    } else {
        Err(WorkspaceInboxError::conflict(
            "inbound email processing lease changed before release",
        ))
    }
}

async fn lease_is_owned(
    connection: &Connection,
    email_id: i64,
    lease_token: &str,
) -> WorkspaceInboxResult<bool> {
    let mut rows = connection
        .query(
            "SELECT 1 FROM intg_received_email_processing_lease \
             WHERE email_id = ?1 AND lease_token = ?2 LIMIT 1",
            params![email_id, lease_token],
        )
        .await
        .context("query inbound email processing lease")?;
    Ok(rows
        .next()
        .await
        .context("read inbound email processing lease")?
        .is_some())
}

async fn email_by_id(
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
        .context("query inbound email")?;
    one_email(rows).await
}

async fn email_by_resend_id(
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
        .context("query inbound email by provider id")?;
    one_email(rows).await
}

async fn one_email(mut rows: libsql::Rows) -> WorkspaceInboxResult<Option<ReceivedEmail>> {
    rows.next()
        .await
        .context("read inbound email")?
        .map(email_from_row)
        .transpose()
}

async fn attachments(
    connection: &Connection,
    email_id: i64,
) -> WorkspaceInboxResult<Vec<ReceivedEmailAttachment>> {
    let mut rows = connection
        .query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state, created_at \
             FROM intg_received_email_attachment WHERE email_id = ?1 ORDER BY id",
            params![email_id],
        )
        .await
        .context("query inbound email attachments")?;
    let mut values = Vec::new();
    while let Some(row) = rows.next().await.context("read inbound email attachment")? {
        values.push(attachment_from_row(row)?);
    }
    Ok(values)
}

async fn attachment_by_id(
    connection: &Connection,
    attachment_id: i64,
) -> WorkspaceInboxResult<Option<ReceivedEmailAttachment>> {
    let mut rows = connection
        .query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state, created_at \
             FROM intg_received_email_attachment WHERE id = ?1 LIMIT 1",
            params![attachment_id],
        )
        .await
        .context("query inbound email attachment")?;
    rows.next()
        .await
        .context("read inbound email attachment")?
        .map(attachment_from_row)
        .transpose()
}

async fn attachment_by_provider_id(
    connection: &Connection,
    email_id: i64,
    provider_id: &str,
) -> WorkspaceInboxResult<Option<ReceivedEmailAttachment>> {
    let mut rows = connection
        .query(
            "SELECT id, email_id, resend_attachment_id, filename, content_type, size_bytes, \
                    s3_key, processing_state, created_at \
             FROM intg_received_email_attachment \
             WHERE email_id = ?1 AND resend_attachment_id = ?2 LIMIT 1",
            params![email_id, provider_id],
        )
        .await
        .context("query inbound email attachment by provider id")?;
    rows.next()
        .await
        .context("read inbound email attachment by provider id")?
        .map(attachment_from_row)
        .transpose()
}

fn email_from_row(row: Row) -> WorkspaceInboxResult<ReceivedEmail> {
    Ok(ReceivedEmail {
        id: row.get(0).context("decode inbound email id")?,
        resend_email_id: row.get(1).context("decode inbound provider email id")?,
        from_address: row.get(2).context("decode inbound email sender")?,
        to_addresses: row.get(3).context("decode inbound email recipients")?,
        subject: row.get(4).context("decode inbound email subject")?,
        received_at: row.get(5).context("decode inbound email received time")?,
        body_s3_key: row.get(6).context("decode inbound email body key")?,
        body_content_type: row.get(7).context("decode inbound email body type")?,
        loan_account: row.get(8).context("decode inbound email loan")?,
        processing_state: row.get(9).context("decode inbound email state")?,
        error_message: row.get(10).context("decode inbound email error")?,
        raw_webhook_payload: row.get(11).context("decode inbound email payload")?,
        created_at: row.get(12).context("decode inbound email creation time")?,
        updated_at: row.get(13).context("decode inbound email update time")?,
    })
}

fn attachment_from_row(row: Row) -> WorkspaceInboxResult<ReceivedEmailAttachment> {
    Ok(ReceivedEmailAttachment {
        id: row.get(0).context("decode inbound attachment id")?,
        email_id: row.get(1).context("decode inbound attachment email id")?,
        resend_attachment_id: row
            .get(2)
            .context("decode inbound provider attachment id")?,
        filename: row.get(3).context("decode inbound attachment filename")?,
        content_type: row.get(4).context("decode inbound attachment type")?,
        size_bytes: row.get(5).context("decode inbound attachment size")?,
        s3_key: row.get(6).context("decode inbound attachment key")?,
        processing_state: row.get(7).context("decode inbound attachment state")?,
        created_at: row
            .get(8)
            .context("decode inbound attachment creation time")?,
    })
}

fn validate_email_draft(draft: &InboundEmailDraft) -> WorkspaceInboxResult<()> {
    validate_provider_id("provider email id", &draft.resend_email_id)?;
    validate_text("sender", &draft.from_address, 1_024)?;
    validate_text("received timestamp", &draft.received_at, 128)?;
    if draft.subject.as_ref().is_some_and(|value| {
        value.len() > 2_048 || value.chars().any(|character| character == '\0')
    }) || draft.raw_webhook_payload.is_empty()
        || draft.raw_webhook_payload.len() > 256 * 1_024
        || serde_json::from_str::<serde_json::Value>(&draft.raw_webhook_payload).is_err()
        || draft.attachments.len() > 64
    {
        return Err(WorkspaceInboxError::validation(
            "inbound email metadata is invalid",
        ));
    }
    let recipients: serde_json::Value = serde_json::from_str(&draft.to_addresses)
        .map_err(|_| WorkspaceInboxError::validation("recipient list is invalid"))?;
    let recipients = recipients
        .as_array()
        .ok_or_else(|| WorkspaceInboxError::validation("recipient list is not an array"))?;
    if recipients.is_empty()
        || recipients.len() > 100
        || recipients.iter().any(|recipient| {
            recipient
                .as_str()
                .is_none_or(|value| value.is_empty() || value.len() > 1_024)
        })
    {
        return Err(WorkspaceInboxError::validation("recipient list is invalid"));
    }
    for attachment in &draft.attachments {
        validate_attachment_draft(attachment)?;
    }
    Ok(())
}

fn validate_attachment_draft(draft: &InboundAttachmentDraft) -> WorkspaceInboxResult<()> {
    validate_provider_id("provider attachment id", &draft.resend_attachment_id)?;
    validate_text("attachment filename", &draft.filename, 512)?;
    validate_content_type(&draft.content_type)
}

fn validate_provider_id(label: &str, value: &str) -> WorkspaceInboxResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} is invalid"
        )));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, max_bytes: usize) -> WorkspaceInboxResult<()> {
    if value.trim().is_empty()
        || value.len() > max_bytes
        || value.chars().any(|character| character == '\0')
    {
        return Err(WorkspaceInboxError::validation(format!(
            "{label} is invalid"
        )));
    }
    Ok(())
}

fn validate_content_type(value: &str) -> WorkspaceInboxResult<()> {
    if value.trim().is_empty()
        || value.len() > 255
        || value != value.trim()
        || value.chars().any(char::is_control)
        || value.matches('/').count() != 1
        || value.bytes().any(|byte| {
            !(byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'/' | b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                ))
        })
    {
        return Err(WorkspaceInboxError::validation("content type is invalid"));
    }
    Ok(())
}

fn validate_lease_token(value: &str) -> WorkspaceInboxResult<()> {
    if !(32..=128).contains(&value.len())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(WorkspaceInboxError::validation(
            "processing lease token is invalid",
        ));
    }
    Ok(())
}
