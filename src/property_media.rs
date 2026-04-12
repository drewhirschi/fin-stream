use anyhow::Context;
use regex::Regex;
use reqwest::{
    Client,
    header::{ACCEPT, CONTENT_TYPE, USER_AGENT},
};
use scraper::{Html, Selector};
use std::collections::BTreeSet;

use crate::{
    db, db::workspaces::LoanWorkspacePhotoInsert, media_storage::MediaStorage,
    models::TmoLoanDetail,
};

const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const APP_USER_AGENT: &str =
    "Mozilla/5.0 (compatible; TrustDeedsBot/1.0; +https://example.invalid/trust-deeds)";
const MAX_IMAGES_PER_LOAN: usize = 6;

pub async fn enrich_loan_workspace(
    pool: &sqlx::PgPool,
    connection_id: i64,
    detail: &TmoLoanDetail,
) -> anyhow::Result<()> {
    let media_state =
        db::workspaces::get_loan_workspace_media_state(pool, connection_id, &detail.loan_account)
            .await?;
    if media_state.has_links_or_photos() {
        return Ok(());
    }

    let search_query = build_search_query(detail);
    if search_query.is_empty() {
        return Ok(());
    }

    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;
    let storage = MediaStorage::from_env().await?;

    let mut results = Vec::new();
    for provider in [Provider::Zillow, Provider::Redfin] {
        if let Some(result) = find_provider_media(&client, provider, &search_query).await? {
            results.push(result);
        }
    }

    if results.is_empty() {
        return Ok(());
    }

    let redfin_url = results
        .iter()
        .find(|result| result.provider == Provider::Redfin)
        .map(|result| result.listing_url.as_str());
    let zillow_url = results
        .iter()
        .find(|result| result.provider == Provider::Zillow)
        .map(|result| result.listing_url.as_str());

    db::workspaces::upsert_workspace_links_if_missing(
        pool,
        connection_id,
        &detail.loan_account,
        redfin_url,
        zillow_url,
    )
    .await?;

    let mut photo_rows = Vec::new();
    let mut sort_order = 0;
    for result in results {
        for media_url in result.image_urls {
            if photo_rows.len() >= MAX_IMAGES_PER_LOAN {
                break;
            }

            let saved_url = match download_image(
                &client,
                &storage,
                &detail.loan_account,
                result.provider,
                sort_order,
                &media_url,
            )
            .await
            {
                Ok(saved_url) => saved_url,
                Err(error) => {
                    tracing::debug!("failed to cache property image {}: {}", media_url, error);
                    continue;
                }
            };

            photo_rows.push(OwnedPhotoInsert {
                provider: result.provider.as_str().to_string(),
                caption: Some(format!(
                    "{} image {}",
                    result.provider.as_str(),
                    sort_order + 1
                )),
                source_url: result.listing_url.clone(),
                image_url: saved_url,
                sort_order,
            });
            sort_order += 1;
        }
    }

    if photo_rows.is_empty() {
        return Ok(());
    }

    let borrowed_rows: Vec<LoanWorkspacePhotoInsert<'_>> = photo_rows
        .iter()
        .map(|photo| LoanWorkspacePhotoInsert {
            provider: &photo.provider,
            caption: photo.caption.as_deref(),
            source_url: &photo.source_url,
            image_url: &photo.image_url,
            sort_order: photo.sort_order,
        })
        .collect();

    db::workspaces::replace_loan_workspace_photos(
        pool,
        connection_id,
        &detail.loan_account,
        &borrowed_rows,
    )
    .await?;

    Ok(())
}

#[derive(Debug, Clone)]
struct OwnedPhotoInsert {
    provider: String,
    caption: Option<String>,
    source_url: String,
    image_url: String,
    sort_order: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    Zillow,
    Redfin,
}

impl Provider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Zillow => "zillow",
            Self::Redfin => "redfin",
        }
    }

    fn listing_host_hint(self) -> &'static str {
        match self {
            Self::Zillow => "site:zillow.com/homedetails",
            Self::Redfin => "site:redfin.com",
        }
    }

    fn image_host_hint(self) -> &'static str {
        match self {
            Self::Zillow => "photos.zillowstatic.com",
            Self::Redfin => "ssl.cdn-redfin.com",
        }
    }
}

#[derive(Debug, Clone)]
struct ProviderMediaResult {
    provider: Provider,
    listing_url: String,
    image_urls: Vec<String>,
}

fn build_search_query(detail: &TmoLoanDetail) -> String {
    [
        detail.primary_street.trim(),
        detail.primary_city.trim(),
        detail.primary_state.trim(),
        detail.primary_zip.trim(),
    ]
    .into_iter()
    .filter(|value| !value.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

async fn find_provider_media(
    client: &Client,
    provider: Provider,
    search_query: &str,
) -> anyhow::Result<Option<ProviderMediaResult>> {
    let search_url = format!(
        "{SEARCH_ENDPOINT}?q={}",
        urlencoding::encode(&format!(
            "{} {}",
            provider.listing_host_hint(),
            search_query
        ))
    );
    let search_html = fetch_text(client, &search_url).await?;
    let Some(listing_url) = extract_listing_url(&search_html, provider) else {
        return Ok(None);
    };

    let listing_html = fetch_text(client, &listing_url).await?;
    let image_urls = extract_image_urls(&listing_html, provider);
    if image_urls.is_empty() {
        return Ok(None);
    }

    Ok(Some(ProviderMediaResult {
        provider,
        listing_url,
        image_urls,
    }))
}

async fn fetch_text(client: &Client, url: &str) -> anyhow::Result<String> {
    let response = client
        .get(url)
        .header(USER_AGENT, APP_USER_AGENT)
        .header(
            ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
        )
        .send()
        .await?
        .error_for_status()?;

    Ok(response.text().await?)
}

fn extract_listing_url(html: &str, provider: Provider) -> Option<String> {
    let document = Html::parse_document(html);
    let selector = Selector::parse("a.result__a").ok()?;
    for anchor in document.select(&selector) {
        let href = anchor.value().attr("href")?;
        let candidate = urlencoding::decode(href).ok()?.into_owned();
        if candidate.contains(provider.as_str()) {
            return Some(clean_search_result_url(&candidate));
        }
    }

    let fallback = Regex::new(r#"https?://[^\s"'<>]+"#).ok()?;
    fallback
        .find_iter(html)
        .map(|m| clean_search_result_url(m.as_str()))
        .find(|candidate| candidate.contains(provider.as_str()))
}

fn clean_search_result_url(raw: &str) -> String {
    if let Some(uddg) = raw.split("uddg=").nth(1) {
        return urlencoding::decode(uddg)
            .map(|value| value.into_owned())
            .unwrap_or_else(|_| raw.to_string());
    }

    raw.replace("&amp;", "&")
}

fn extract_image_urls(html: &str, provider: Provider) -> Vec<String> {
    let mut urls = BTreeSet::new();

    if let Ok(pattern) = Regex::new(&format!(
        r#"https:\\?/\\?/{}[^"'\\\s<)]+"#,
        regex::escape(provider.image_host_hint())
    )) {
        for found in pattern.find_iter(html) {
            urls.insert(unescape_media_url(found.as_str()));
        }
    }

    let document = Html::parse_document(html);
    if let Ok(selector) =
        Selector::parse(r#"meta[property="og:image"], meta[name="twitter:image"]"#)
    {
        for node in document.select(&selector) {
            if let Some(content) = node.value().attr("content") {
                if content.contains(provider.image_host_hint()) {
                    urls.insert(content.to_string());
                }
            }
        }
    }

    urls.into_iter()
        .filter(|url| url.starts_with("http"))
        .take(MAX_IMAGES_PER_LOAN)
        .collect()
}

fn unescape_media_url(raw: &str) -> String {
    raw.replace("\\u002F", "/")
        .replace("\\/", "/")
        .replace("https:///", "https://")
        .replace("http:///", "http://")
}

async fn download_image(
    client: &Client,
    storage: &MediaStorage,
    loan_account: &str,
    provider: Provider,
    sort_order: i32,
    url: &str,
) -> anyhow::Result<String> {
    let response = client
        .get(url)
        .header(USER_AGENT, APP_USER_AGENT)
        .send()
        .await?
        .error_for_status()?;

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string);
    let extension = content_type
        .as_deref()
        .and_then(extension_from_content_type)
        .or_else(|| extension_from_url(url))
        .unwrap_or("jpg");

    let bytes = response.bytes().await?.to_vec();
    let object_key = format!(
        "{}/{}-{:02}.{}",
        sanitize_segment(loan_account),
        provider.as_str(),
        sort_order + 1,
        extension
    );
    let stored = storage
        .store(&object_key, bytes, content_type.as_deref())
        .await
        .with_context(|| format!("persisting property image {}", object_key))?;

    Ok(stored.public_url)
}

fn sanitize_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}

fn extension_from_content_type(content_type: &str) -> Option<&'static str> {
    match content_type.split(';').next()?.trim() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

fn extension_from_url(url: &str) -> Option<&'static str> {
    let lowercase = url.to_ascii_lowercase();
    if lowercase.contains(".png") {
        Some("png")
    } else if lowercase.contains(".webp") {
        Some("webp")
    } else if lowercase.contains(".jpg") || lowercase.contains(".jpeg") {
        Some("jpg")
    } else {
        None
    }
}
