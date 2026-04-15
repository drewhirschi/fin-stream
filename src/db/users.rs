use sqlx::PgPool;

/// Look up a user by email. Returns (id, email, password_hash) for active users only.
pub async fn get_user_by_email(
    pool: &PgPool,
    email: &str,
) -> anyhow::Result<Option<(i64, String, String)>> {
    let row: Option<(i64, String, String)> = sqlx::query_as(
        "SELECT id, email, password_hash
         FROM app_user
         WHERE email = $1 AND is_active = 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

/// Look up a user by id. Returns (id, email, display_name) for active users only.
#[allow(dead_code)]
pub async fn get_user_by_id(
    pool: &PgPool,
    id: i64,
) -> anyhow::Result<Option<(i64, String, Option<String>)>> {
    let row: Option<(i64, String, Option<String>)> = sqlx::query_as(
        "SELECT id, email, display_name
         FROM app_user
         WHERE id = $1 AND is_active = 1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}
