use sqlx::PgPool;

use crate::models::{ReceivedEmailAttachmentView, ReceivedEmailView};

pub async fn insert_received_email(
    pool: &PgPool,
    resend_email_id: &str,
    from_address: &str,
    to_addresses: &str,
    subject: Option<&str>,
    received_at: &str,
    raw_webhook_payload: &str,
) -> anyhow::Result<Option<i64>> {
    let row: Option<(i64,)> = sqlx::query_as(
        "INSERT INTO intg.received_email (resend_email_id, from_address, to_addresses, subject, received_at, raw_webhook_payload)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (resend_email_id) DO NOTHING
         RETURNING id",
    )
    .bind(resend_email_id)
    .bind(from_address)
    .bind(to_addresses)
    .bind(subject)
    .bind(received_at)
    .bind(raw_webhook_payload)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(id,)| id))
}

pub async fn insert_attachment_row(
    pool: &PgPool,
    email_id: i64,
    resend_attachment_id: &str,
    filename: &str,
    content_type: &str,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO intg.received_email_attachment (email_id, resend_attachment_id, filename, content_type)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (email_id, resend_attachment_id) DO NOTHING
         RETURNING id",
    )
    .bind(email_id)
    .bind(resend_attachment_id)
    .bind(filename)
    .bind(content_type)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

pub async fn mark_email_body_stored(
    pool: &PgPool,
    email_id: i64,
    body_s3_key: &str,
    body_content_type: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.received_email
         SET processing_state = 'stored',
             body_s3_key = $2,
             body_content_type = $3,
             error_message = NULL,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $1",
    )
    .bind(email_id)
    .bind(body_s3_key)
    .bind(body_content_type)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_attachment_stored(
    pool: &PgPool,
    attachment_id: i64,
    s3_key: &str,
    size_bytes: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.received_email_attachment
         SET processing_state = 'stored',
             s3_key = $2,
             size_bytes = $3
         WHERE id = $1",
    )
    .bind(attachment_id)
    .bind(s3_key)
    .bind(size_bytes)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_email_error(
    pool: &PgPool,
    email_id: i64,
    error_message: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.received_email
         SET processing_state = 'error',
             error_message = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $1",
    )
    .bind(email_id)
    .bind(error_message)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_unlinked_emails(pool: &PgPool) -> Vec<ReceivedEmailView> {
    sqlx::query_as(
        "SELECT e.id,
                e.resend_email_id,
                e.from_address,
                e.to_addresses,
                e.subject,
                e.received_at,
                e.body_s3_key,
                e.body_content_type,
                e.loan_account,
                e.processing_state,
                e.error_message,
                COALESCE(a.cnt, 0) AS attachment_count,
                e.created_at
         FROM intg.received_email e
         LEFT JOIN (
             SELECT email_id, COUNT(*)::bigint AS cnt
             FROM intg.received_email_attachment
             GROUP BY email_id
         ) a ON a.email_id = e.id
         WHERE e.loan_account IS NULL
         ORDER BY e.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn list_emails_for_loan(pool: &PgPool, loan_account: &str) -> Vec<ReceivedEmailView> {
    sqlx::query_as(
        "SELECT e.id,
                e.resend_email_id,
                e.from_address,
                e.to_addresses,
                e.subject,
                e.received_at,
                e.body_s3_key,
                e.body_content_type,
                e.loan_account,
                e.processing_state,
                e.error_message,
                COALESCE(a.cnt, 0) AS attachment_count,
                e.created_at
         FROM intg.received_email e
         LEFT JOIN (
             SELECT email_id, COUNT(*)::bigint AS cnt
             FROM intg.received_email_attachment
             GROUP BY email_id
         ) a ON a.email_id = e.id
         WHERE e.loan_account = $1
         ORDER BY e.received_at DESC",
    )
    .bind(loan_account)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn get_email_by_id(pool: &PgPool, email_id: i64) -> Option<ReceivedEmailView> {
    sqlx::query_as(
        "SELECT e.id,
                e.resend_email_id,
                e.from_address,
                e.to_addresses,
                e.subject,
                e.received_at,
                e.body_s3_key,
                e.body_content_type,
                e.loan_account,
                e.processing_state,
                e.error_message,
                COALESCE(a.cnt, 0) AS attachment_count,
                e.created_at
         FROM intg.received_email e
         LEFT JOIN (
             SELECT email_id, COUNT(*)::bigint AS cnt
             FROM intg.received_email_attachment
             GROUP BY email_id
         ) a ON a.email_id = e.id
         WHERE e.id = $1",
    )
    .bind(email_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

pub async fn list_attachments_for_email(
    pool: &PgPool,
    email_id: i64,
) -> Vec<ReceivedEmailAttachmentView> {
    sqlx::query_as(
        "SELECT id,
                resend_attachment_id,
                filename,
                content_type,
                size_bytes,
                s3_key,
                processing_state
         FROM intg.received_email_attachment
         WHERE email_id = $1
         ORDER BY id ASC",
    )
    .bind(email_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn link_email_to_loan(
    pool: &PgPool,
    email_id: i64,
    loan_account: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.received_email
         SET loan_account = $2,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $1",
    )
    .bind(email_id)
    .bind(loan_account)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn unlink_email(pool: &PgPool, email_id: i64) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE intg.received_email
         SET loan_account = NULL,
             updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
         WHERE id = $1",
    )
    .bind(email_id)
    .execute(pool)
    .await?;

    Ok(())
}
