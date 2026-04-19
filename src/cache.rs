use moka::future::Cache;
use std::sync::Arc;
use std::time::Duration;

/// In-memory page cache backed by moka.
///
/// When `PAGE_CACHE=0` (or disabled), all operations are no-ops: `get` always
/// returns `None` and `insert`/`invalidate` do nothing. This lets handlers use
/// the cache unconditionally without `if` guards everywhere.
#[derive(Clone)]
pub struct PageCache {
    inner: Option<Arc<Cache<String, String>>>,
}

impl PageCache {
    /// Build a live cache with the given max capacity and TTL safety net.
    pub fn new(max_capacity: u64, ttl: Duration) -> Self {
        Self {
            inner: Some(Arc::new(
                Cache::builder()
                    .max_capacity(max_capacity)
                    .time_to_live(ttl)
                    .build(),
            )),
        }
    }

    /// Build a disabled (no-op) cache.
    pub fn disabled() -> Self {
        Self { inner: None }
    }

    /// Build from the `PAGE_CACHE` env var. Enabled by default; set
    /// `PAGE_CACHE=0` to disable.
    pub fn from_env() -> Self {
        let disabled = std::env::var("PAGE_CACHE")
            .map(|v| v == "0" || v.eq_ignore_ascii_case("false"))
            .unwrap_or(false);

        if disabled {
            tracing::info!("page cache disabled (PAGE_CACHE=0)");
            Self::disabled()
        } else {
            tracing::info!("page cache enabled (max 500 entries, 5 min TTL)");
            Self::new(500, Duration::from_secs(300))
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    /// Get a cached page by key. Returns `None` on miss or if disabled.
    pub async fn get(&self, key: &str) -> Option<String> {
        self.inner.as_ref()?.get(key).await
    }

    /// Insert a rendered page into the cache.
    pub async fn insert(&self, key: String, html: String) {
        if let Some(cache) = &self.inner {
            cache.insert(key, html).await;
        }
    }

    /// Invalidate all entries whose key starts with the given prefix.
    pub async fn invalidate_prefix(&self, prefix: &str) {
        if let Some(cache) = &self.inner {
            let prefix = prefix.to_string();
            cache
                .invalidate_entries_if(move |key, _| key.starts_with(&prefix))
                .ok();
        }
    }

    /// Invalidate a single key.
    pub async fn invalidate(&self, key: &str) {
        if let Some(cache) = &self.inner {
            cache.invalidate(key).await;
        }
    }

    /// Invalidate everything. Used after TMO sync which touches most data.
    pub async fn invalidate_all(&self) {
        if let Some(cache) = &self.inner {
            cache.invalidate_all();
        }
    }
}
