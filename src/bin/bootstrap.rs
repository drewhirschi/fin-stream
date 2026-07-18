#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let result = trust_deeds::bootstrap_local_from_env().await?;
    println!(
        "bootstrap complete: primary account {}, default view {}, {} streams; writes enabled, scheduler off",
        result.primary_account_id,
        result.default_view_id,
        result.stream_ids.len()
    );
    Ok(())
}
