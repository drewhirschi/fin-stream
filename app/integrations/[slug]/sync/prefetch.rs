include!(concat!(env!("OUT_DIR"), "/nextrs_seeds.rs"));

pub async fn prefetch(
    req: axum::http::Request<axum::body::Body>,
    params: nextrs::Params,
) -> nextrs::QuerySeed {
    let slug = params.get("slug").unwrap_or_default().to_owned();

    nextrs::QuerySeed::new()
        .seed(get_integrations_by_slug_sync_status(
            slug,
            req.extensions(),
        ))
        .await
}
