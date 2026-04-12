use trust_deeds::media_storage;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let buckets = media_storage::list_configured_buckets_from_env().await?;
    if buckets.is_empty() {
        println!("No buckets returned.");
    } else {
        for bucket in buckets {
            println!("{}", bucket);
        }
    }

    Ok(())
}
