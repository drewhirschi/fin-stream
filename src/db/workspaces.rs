use sqlx::PgPool;

use crate::models::{LoanWorkspacePhotoView, LoanWorkspaceView};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LoanWorkspaceMediaState {
    pub redfin_url: Option<String>,
    pub zillow_url: Option<String>,
    pub photo_count: i64,
}

impl LoanWorkspaceMediaState {
    pub fn has_links_or_photos(&self) -> bool {
        self.redfin_url.is_some() || self.zillow_url.is_some() || self.photo_count > 0
    }
}

pub async fn get_loan_workspace(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
) -> Option<LoanWorkspaceView> {
    sqlx::query_as(
        "SELECT loan_account, redfin_url, zillow_url, decision_status,
                target_contribution, actual_contribution, notes
         FROM intg.loan_workspace
         WHERE connection_id = $1 AND loan_account = $2
         LIMIT 1",
    )
    .bind(connection_id)
    .bind(loan_account)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

pub async fn upsert_loan_workspace(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
    redfin_url: Option<&str>,
    zillow_url: Option<&str>,
    decision_status: Option<&str>,
    target_contribution: Option<f64>,
    actual_contribution: Option<f64>,
    notes: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO intg.loan_workspace (
            connection_id, loan_account, redfin_url, zillow_url, decision_status,
            target_contribution, actual_contribution, notes
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (connection_id, loan_account) DO UPDATE SET
            redfin_url = excluded.redfin_url,
            zillow_url = excluded.zillow_url,
            decision_status = excluded.decision_status,
            target_contribution = excluded.target_contribution,
            actual_contribution = excluded.actual_contribution,
            notes = excluded.notes,
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(connection_id)
    .bind(loan_account)
    .bind(redfin_url)
    .bind(zillow_url)
    .bind(decision_status)
    .bind(target_contribution)
    .bind(actual_contribution)
    .bind(notes)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn get_loan_workspace_media_state(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
) -> anyhow::Result<LoanWorkspaceMediaState> {
    let row = sqlx::query_as(
        "SELECT lw.redfin_url,
                lw.zillow_url,
                COUNT(photo.id)::bigint AS photo_count
         FROM intg.loan_workspace lw
         LEFT JOIN intg.loan_workspace_photo photo
           ON photo.connection_id = lw.connection_id
          AND photo.loan_account = lw.loan_account
         WHERE lw.connection_id = $1 AND lw.loan_account = $2
         GROUP BY lw.redfin_url, lw.zillow_url",
    )
    .bind(connection_id)
    .bind(loan_account)
    .fetch_optional(pool)
    .await?;

    Ok(row.unwrap_or(LoanWorkspaceMediaState {
        redfin_url: None,
        zillow_url: None,
        photo_count: 0,
    }))
}

pub async fn upsert_workspace_links_if_missing(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
    redfin_url: Option<&str>,
    zillow_url: Option<&str>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO intg.loan_workspace (
            connection_id, loan_account, redfin_url, zillow_url
         )
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (connection_id, loan_account) DO UPDATE SET
            redfin_url = COALESCE(intg.loan_workspace.redfin_url, excluded.redfin_url),
            zillow_url = COALESCE(intg.loan_workspace.zillow_url, excluded.zillow_url),
            updated_at = TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .bind(connection_id)
    .bind(loan_account)
    .bind(redfin_url)
    .bind(zillow_url)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn replace_loan_workspace_photos(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
    photos: &[LoanWorkspacePhotoInsert<'_>],
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "DELETE FROM intg.loan_workspace_photo
         WHERE connection_id = $1 AND loan_account = $2",
    )
    .bind(connection_id)
    .bind(loan_account)
    .execute(&mut *tx)
    .await?;

    for photo in photos {
        sqlx::query(
            "INSERT INTO intg.loan_workspace_photo (
                connection_id, loan_account, provider, caption, source_url, image_url, sort_order, is_featured
             )
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(connection_id)
        .bind(loan_account)
        .bind(photo.provider)
        .bind(photo.caption)
        .bind(photo.source_url)
        .bind(photo.image_url)
        .bind(photo.sort_order)
        .bind(photo.is_featured)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn list_loan_workspace_photos(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
) -> anyhow::Result<Vec<LoanWorkspacePhotoView>> {
    let photos = sqlx::query_as(
        "SELECT id, loan_account, provider, caption, source_url, image_url, sort_order, is_featured
         FROM intg.loan_workspace_photo
         WHERE connection_id = $1 AND loan_account = $2
         ORDER BY is_featured DESC, sort_order ASC, id ASC",
    )
    .bind(connection_id)
    .bind(loan_account)
    .fetch_all(pool)
    .await?;

    Ok(photos)
}

pub async fn next_photo_sort_order(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
) -> anyhow::Result<i32> {
    let (next_order,): (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(sort_order), -1) + 1
         FROM intg.loan_workspace_photo
         WHERE connection_id = $1 AND loan_account = $2",
    )
    .bind(connection_id)
    .bind(loan_account)
    .fetch_one(pool)
    .await?;

    Ok(next_order)
}

pub async fn insert_loan_workspace_photo(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
    provider: &str,
    caption: Option<&str>,
    source_url: &str,
    image_url: &str,
    sort_order: i32,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO intg.loan_workspace_photo (
            connection_id, loan_account, provider, caption, source_url, image_url, sort_order, is_featured
         )
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(connection_id)
    .bind(loan_account)
    .bind(provider)
    .bind(caption)
    .bind(source_url)
    .bind(image_url)
    .bind(sort_order)
    .bind(false)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_featured_photo(
    pool: &PgPool,
    connection_id: i64,
    loan_account: &str,
    photo_id: i64,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query(
        "UPDATE intg.loan_workspace_photo
         SET is_featured = FALSE
         WHERE connection_id = $1 AND loan_account = $2",
    )
    .bind(connection_id)
    .bind(loan_account)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE intg.loan_workspace_photo
         SET is_featured = TRUE
         WHERE id = $3 AND connection_id = $1 AND loan_account = $2",
    )
    .bind(connection_id)
    .bind(loan_account)
    .bind(photo_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct LoanWorkspacePhotoInsert<'a> {
    pub provider: &'a str,
    pub caption: Option<&'a str>,
    pub source_url: &'a str,
    pub image_url: &'a str,
    pub sort_order: i32,
    pub is_featured: bool,
}
