use trust_deeds::{db, property_media, tmo};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,trust_deeds=debug".into()),
        )
        .init();

    let pool = db::init().await?;
    let Some(connection) = db::integrations::get_connection_by_slug(&pool, "tmo").await else {
        anyhow::bail!("missing TMO integration connection")
    };

    let loan_accounts: Vec<String> = sqlx::query_scalar(
        "SELECT loan_account
         FROM intg.tmo_import_loan
         WHERE connection_id = $1 AND is_active = 1
         ORDER BY loan_account",
    )
    .bind(connection.id)
    .fetch_all(&pool)
    .await?;

    let client = tmo::client::create_client().await?;

    let mut enriched = 0usize;
    let mut failed = 0usize;

    for loan_account in loan_accounts {
        match client.get_loan_detail(&loan_account).await {
            Ok(detail) => {
                match property_media::enrich_loan_workspace(&pool, connection.id, &detail).await {
                    Ok(()) => {
                        enriched += 1;
                        tracing::info!("processed property media for loan {}", loan_account);
                    }
                    Err(error) => {
                        failed += 1;
                        tracing::warn!(
                            "failed property media enrichment for {}: {}",
                            loan_account,
                            error
                        );
                    }
                }
            }
            Err(error) => {
                failed += 1;
                tracing::warn!("failed to load TMO detail for {}: {}", loan_account, error);
            }
        }
    }

    let (photo_count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM intg.loan_workspace_photo WHERE connection_id = $1",
    )
    .bind(connection.id)
    .fetch_one(&pool)
    .await?;

    tracing::info!(
        "property media backfill complete: processed={}, failed={}, total_photos={}",
        enriched,
        failed,
        photo_count
    );

    Ok(())
}
