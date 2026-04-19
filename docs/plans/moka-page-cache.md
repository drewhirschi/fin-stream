# Page Cache with Moka

## Concept

Cache rendered HTML responses in-memory using `moka`. Pages are served from cache instantly (~0.001ms) until a mutation invalidates them. This is the same model as Next.js ISR, but simpler — single server, no Redis, no distributed coordination.

**Rule: every mutation must signal what it invalidated. That's the only thing to get right.**

## Cache Zones

Group pages into invalidation zones. When a mutation happens, invalidate the entire zone rather than tracking individual pages. Simpler, slightly more aggressive, but never stale.

| Zone | Pages | Invalidated By |
|------|-------|----------------|
| `tmo` | `/integrations/tmo`, `/integrations/tmo/loans`, `/integrations/tmo/payments`, `/integrations/tmo/sync` | TMO sync, loan workspace mutations |
| `tmo_loan:{account}` | `/integrations/tmo/loans/{account}` | TMO sync, workspace save/photo, email link/unlink |
| `forecast` | `/forecast` | Any event/stream/account/view mutation, TMO sync, cash balance change |
| `streams` | `/streams` | Event/stream/account mutation, TMO sync |
| `inbox` | `/inbox` | Email webhook, email delete |
| `inbox:{id}` | `/inbox/{id}` | Email webhook, link/unlink, delete, retry |
| `dashboard` | `/` | TMO sync, account balance change |
| `integrations` | `/integrations` | TMO sync (connection metadata changes) |

## Implementation

### 1. Add moka dependency

```toml
# Cargo.toml
moka = { version = "0.12", features = ["future"] }
```

### 2. Add cache to AppState

```rust
use moka::future::Cache;
use std::time::Duration;

pub struct AppState {
    pub db: PgPool,
    // ...existing fields...
    pub page_cache: Cache<String, String>,  // cache key → rendered HTML
}

// In main.rs, build the cache:
let page_cache = Cache::builder()
    .max_capacity(500)                    // max 500 cached pages
    .time_to_live(Duration::from_secs(300))  // 5 min TTL as safety net
    .build();
```

The TTL is a safety net — not the primary invalidation strategy. Even if we miss an invalidation signal, pages go stale for at most 5 minutes.

### 3. Cache helper middleware/extractor

A small helper that checks the cache before running the handler:

```rust
/// Try to serve from cache. On miss, call the handler, cache the result, return it.
async fn cached_page(
    cache: &Cache<String, String>,
    key: &str,
    render: impl Future<Output = String>,
) -> impl IntoResponse {
    if let Some(html) = cache.get(key) {
        return Html(html).into_response();
    }
    let html = render.await;
    cache.insert(key.to_string(), html.clone()).await;
    Html(html).into_response()
}
```

### 4. Use in handlers

Before (current):
```rust
async fn integration_overview(...) -> Response {
    // 4 DB queries + template render every time
    let loans = get_active_loans(&state.db).await;
    // ...
    template.into_response()
}
```

After:
```rust
async fn integration_overview(...) -> Response {
    let cache_key = format!("tmo:{slug}");
    cached_page(&state.page_cache, &cache_key, async {
        let loans = get_active_loans(&state.db).await;
        // ...
        template.render().unwrap()
    }).await
}
```

### 5. Invalidation at mutation points

This is the critical part. Every mutation needs one line to bust the relevant cache zone.

#### TMO Sync (`src/tmo/sync.rs` — `run_full_sync`)
At the end of a successful sync:
```rust
// Bust everything TMO-related + downstream
state.page_cache.invalidate_all();  
// Or more targeted:
// invalidate_zone(&state.page_cache, "tmo");
// invalidate_zone(&state.page_cache, "forecast");
// invalidate_zone(&state.page_cache, "streams");
// invalidate_zone(&state.page_cache, "dashboard");
```
Since sync touches loans, payments, portfolio snapshot, and events — it's simpler to just `invalidate_all()` here. Sync runs every 6 hours, so one full cache bust is fine.

#### API mutations (`src/routes/api.rs`)
| Route | Invalidate |
|-------|------------|
| `POST/PATCH /api/events` | `forecast`, `streams`, `tmo` |
| `POST/PATCH /api/accounts` | `forecast`, `streams`, `dashboard` |
| `POST/PATCH /api/streams` | `forecast`, `streams` |
| `POST/PATCH /api/views` | `forecast` |
| `POST /api/settings/cash` | `forecast`, `dashboard` |
| `POST /api/sync/balance` | `forecast`, `streams`, `dashboard` |

#### Email mutations (`src/routes/pages.rs`)
| Route | Invalidate |
|-------|------------|
| `POST /inbox/{id}/link` | `inbox:{id}`, `tmo_loan:{account}` |
| `POST /inbox/{id}/unlink` | `inbox:{id}`, `tmo_loan:{account}` |
| `POST /inbox/{id}/delete` | `inbox`, `inbox:{id}` |
| `POST /inbox/{id}/retry` | `inbox:{id}` |

#### Loan workspace mutations (`src/routes/pages.rs`)
| Route | Invalidate |
|-------|------------|
| `POST .../workspace` | `tmo_loan:{account}` |
| `POST .../workspace/photos` | `tmo_loan:{account}` |
| `POST .../photos/{id}/feature` | `tmo_loan:{account}` |

#### Webhook (`src/routes/webhooks.rs`)
| Route | Invalidate |
|-------|------------|
| `POST /webhooks/resend` | `inbox` |

### 6. Zone invalidation helper

```rust
/// Invalidate all cache entries whose key starts with the given prefix.
fn invalidate_zone(cache: &Cache<String, String>, prefix: &str) {
    cache.invalidate_entries_if(move |key, _| key.starts_with(prefix));
}
```

## Cache key scheme

```
tmo:overview          → /integrations/tmo
tmo:loans             → /integrations/tmo/loans
tmo:payments          → /integrations/tmo/payments
tmo:sync              → /integrations/tmo/sync
tmo_loan:{account}    → /integrations/tmo/loans/{account}
forecast:{view_id}    → /forecast?view_id=X
streams               → /streams
inbox                 → /inbox
inbox:{id}            → /inbox/{id}
dashboard             → /
integrations          → /integrations
```

## What NOT to cache

- POST/mutation responses (redirects, HTMX swap responses)
- `/health`, `/ready` — trivial, no DB
- `/integrations/tmo/debug` — admin page, should always be fresh
- HTMX partial responses (panel loads, etc.) — these are small and context-dependent

## Expected impact

| Metric | Before | After (cache hit) |
|--------|--------|-------------------|
| `/integrations/tmo` latency | ~17ms | ~0.01ms |
| `/integrations/tmo` RPS ceiling | ~3,800 | ~190,000 (same as /health) |
| Memory overhead | ~18MB | ~20-25MB (cached HTML is small) |

Cache misses (first request after invalidation) still take ~8-10ms with the `tokio::join!` optimization.

## Simplicity notes

- **No Redis**: single server, in-process cache is all we need
- **No stale-while-revalidate**: pages are either cached or re-rendered. With 8ms render time, there's no need for background refresh.
- **Aggressive invalidation is fine**: even if we over-invalidate, the worst case is a single cache miss that takes 8ms. Under-invalidating (serving stale data) is worse.
- **The 5-minute TTL is a safety net, not the strategy**: mutations drive invalidation. TTL is just insurance against bugs.
