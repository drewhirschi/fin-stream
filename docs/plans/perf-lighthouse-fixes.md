# Lighthouse perf fixes

## Why this plan exists

Measured Lighthouse on the public app (2026-04-24). Mobile score **60/100**, all timing metrics cluster at **~7 seconds** (FCP 7.1s, LCP 7.1s, Speed Index 7.1s). No JS or layout issues — TBT 0ms, CLS 0. The whole budget is being burned on oversized, uncompressed assets on the critical path.

The user was separately seeing **77** from some other run (likely desktop or warm-cache), same underlying causes.

## Baseline measurement (2026-04-24)

Method: SSH-tunnel from laptop → gory's Docker bridge → container on 10.0.1.13:3000, Lighthouse 12.8.2 with mobile form-factor + simulated 3G throttling. This skips Cloudflare Access so we measure our backend, not Cloudflare's login interstitial.

```
PERF SCORE: 60
FCP: 7.1 s
LCP: 7.1 s
Speed Index: 7.1 s
TBT: 0 ms
CLS: 0
Total page weight: 1,752 KiB
```

Top asset sizes on `/login`:

| Resource | Bytes | Encoding |
|---|--:|---|
| `/static/favicon.ico` | 631,395 | none |
| `/static/vendor/css/daisyui.css` | 789,506 | none |
| `/static/vendor/js/tailwindcss.js` | 271,265 | none |
| `/static/vendor/css/daisyui-themes.css` | 38,347 | none |
| `/static/vendor/fonts/inter-variable.woff2` | 23,692 | (woff2 is already compressed) |
| `/static/vendor/fonts/manrope-variable.woff2` | 24,376 | (woff2 is already compressed) |

Render-blocking resources, per Lighthouse (wasted ms of render time):

| Resource | Size | Wasted |
|---|--:|--:|
| `daisyui.css` | 789 KB | **6,452 ms** |
| `daisyui-themes.css` | 38 KB | 1,202 ms |
| `vault.css` | 10 KB | 452 ms |
| `style.css` | 2 KB | 302 ms |

Top Lighthouse opportunities:

| Opportunity | Est. savings |
|---|--:|
| Enable text compression | **4,880 ms** |
| Reduce unused JavaScript | 750 ms |
| Reduce unused CSS | 300 ms |

## Root causes

1. **Zero compression** on any response — dynamic HTML or static assets. The Rust app's Router has no `tower-http` `CompressionLayer`, and Traefik on gory has no compression middleware configured. Every byte ships raw.
2. **`favicon.ico` is a 1024×1024 PNG, 631 KB.** File command confirms: `PNG image data, 1024 x 1024, 8-bit/color RGBA`. The `.ico` extension is a lie. A real favicon is 1–5 KB. This alone is 36% of the page weight.
3. **DaisyUI is shipped unpurged** (789 KB for the full kitchen-sink bundle). Lighthouse finds 35 KB of the rules are never matched by any element on the page — and that's just the ones it could analyze. A real purged build is typically 20–80 KB.
4. **Tailwind CSS is doing browser-side JIT compilation** — we ship `static/vendor/js/tailwindcss.js` (271 KB) and it parses our HTML and synthesizes CSS classes in the user's browser on every page load. Lighthouse finds 109 KB of that runtime unused per page. This is an explicit tradeoff we made (noted in `CLAUDE.md`: "DaisyUI 5 + Tailwind CSS 4 (browser JIT)") — no build step, fast dev iteration, but a real cost for users.
5. **All 800+ KB of CSS is render-blocking** — sits in `<head>` as `<link rel="stylesheet">` with no `media` attribute or `rel="preload"` hinting. Browser blocks first paint until they all parse.
6. **Fonts aren't preloaded.** Two 24 KB woff2 fonts load late in the waterfall; preload links would start them at kickoff.
7. **No `Cache-Control` on static assets.** `ServeDir` emits ETag + Last-Modified (so browsers *can* do conditional requests and get 304s) but emits no `Cache-Control` header, so every visit still round-trips for every asset to validate. First-load perf is unaffected; return-visit perf takes N extra round-trips for N assets.

## Target state

**Mobile Lighthouse perf 95+.** FCP/LCP under 2s on simulated 3G. Total page weight under 300 KiB. No single asset over 100 KiB.

All five fixes are independent. Any one is a win. In estimated order of impact:

1. Compression → removes 4.9s of waterfall time
2. Right-sized favicon → removes 630 KB of weight
3. Purged DaisyUI + no browser JIT → removes 1,000+ KB of weight and 109 KB of parse-and-execute JS
4. Font preload → ~200-400 ms earlier first paint
5. `Cache-Control` on `/static` → cuts return-visit load time to near-zero (304s, or nothing if cache fresh)

## The five changes

### 1. `tower-http` CompressionLayer (code — smallest diff, biggest immediate impact)

**File: `Cargo.toml`**
```toml
tower-http = { version = "0.6", features = ["fs", "trace", "compression-br", "compression-gzip"] }
```

**File: `src/main.rs`** — add the layer on the outer Router, after session layer so compression wraps everything.
```rust
use tower_http::compression::CompressionLayer;

let app = Router::new()
    .merge(public)
    .merge(protected)
    .nest_service("/static", ServeDir::new("static"))
    .fallback(routes::pages::not_found)
    .layer(session_layer)
    .layer(CompressionLayer::new())     // new
    .with_state(state.clone());
```

Tower-http negotiates br → gzip → identity based on `Accept-Encoding`. No content is compressed that already arrived compressed (woff2, already-gzipped static files are left alone).

**Throughput tradeoff.** Default compression is brotli level 4, which costs ~0.5-1ms of CPU per 12 KB HTML response. Our current `/bench/render` ceiling of ~83k RPS (per `rust-web-scaffold/docs/benchmark-go-vs-rust.md`) will drop to roughly 40-60k RPS at peak load. For the actual production use case (≪10 RPS), this is undetectable. If we ever need to claw RPS back, `CompressionLayer::new().quality(CompressionLevel::Fastest)` drops to brotli-1 — bytes shrink ~60% instead of ~80%, CPU cost drops ~10×.

Verification: `curl -I -H 'Accept-Encoding: gzip, br' http://.../login` should show `Content-Encoding: br`.

### 2. Real favicon (static asset swap)

Regenerate `static/favicon.ico` as a proper 32×32 (or 16×16+32×32 multi-size) ICO, ideally under 5 KB. If ImageMagick is available: `convert static/favicon.ico -resize 32x32 -background transparent static/favicon-32.ico`. If not, ship an inline SVG favicon (`<link rel="icon" type="image/svg+xml" href="/static/favicon.svg">`) — modern browsers prefer SVG anyway, and an SVG favicon is ~500 bytes.

### 3. Build-time Tailwind + DaisyUI purge (the invasive fix)

**Architectural change.** Today's flow: browser downloads `tailwindcss.js` (271 KB) + `daisyui.css` (789 KB) + `daisyui-themes.css` (38 KB). The JS scans the DOM for classes and synthesizes CSS rules at runtime.

Target flow: at Docker-build time, a Node stage runs the Tailwind CLI over our HTML templates, purges unused classes, and emits a single minified `app.css` of ~30–80 KB. Browser loads one file. No JIT JS.

**New files:**

`package.json`
```json
{
  "name": "trust-deeds-css",
  "private": true,
  "devDependencies": {
    "@tailwindcss/cli": "^4.1.0",
    "daisyui": "^5.0.0"
  }
}
```

`static/app.input.css`
```css
@import "tailwindcss";
@plugin "daisyui";

/* our own small rules can live here too; they'll be minified alongside */
```

**Dockerfile change** — insert a Node builder stage before the runtime:

```dockerfile
FROM node:22-alpine AS css-builder
WORKDIR /css
COPY package.json package-lock.json ./
RUN npm ci
COPY static/app.input.css ./
COPY templates ./templates
# Purges against everything Tailwind finds in templates/
RUN npx @tailwindcss/cli -i ./app.input.css -o ./app.css --minify

# ... existing Rust builder stage ...

FROM debian:bookworm-slim AS runtime
# ... existing copies ...
COPY --from=css-builder /css/app.css /app/static/app.css
```

**`templates/base.html` change** — replace the three vendor CSS links + the Tailwind JS tag with a single:
```html
<link rel="stylesheet" href="/static/app.css">
```

**Keep** the vendored files in `static/vendor/` in the repo. They're zero cost if nothing references them, and they're useful as an offline fallback if the build step ever breaks.

**Local dev loop** — the user still wants live-reload CSS during development. Add to the Makefile:
```make
css-watch:
	bunx @tailwindcss/cli -i static/app.input.css -o static/app.css --watch
```
Run in a second terminal alongside `cargo watch -x run`.

**Accept this cost**: the Docker build grows by ~90 seconds (npm install) on cold builds. With BuildKit layer caching, warm builds are <5s added. Tradeoff: 90s of build time for saving every user 4+ seconds per page load.

**Revisit `CLAUDE.md`** — remove the "Tailwind CSS 4 (browser JIT)" note and replace with "Tailwind CSS 4 (built at Docker build time, purged against templates/)."

### 4. Preload fonts

**File: `templates/base.html`** — add to `<head>`, above the stylesheet links:
```html
<link rel="preload" as="font" type="font/woff2" crossorigin
      href="/static/vendor/fonts/manrope-variable.woff2">
<link rel="preload" as="font" type="font/woff2" crossorigin
      href="/static/vendor/fonts/inter-variable.woff2">
```

Tiny change. Probably only ~200 ms on a simulated-3G cold load, but it's free.

### 5. Cache-Control on `/static`

Wrap `ServeDir` with a `SetResponseHeaderLayer` that adds `Cache-Control: public, max-age=3600, must-revalidate`. `ServeDir` already emits `ETag` + `Last-Modified`, so after the 1-hour TTL the browser sends `If-None-Match` and gets a 304 with no body. Within the TTL, the browser skips the network entirely.

```rust
use tower_http::set_header::SetResponseHeaderLayer;
use http::{HeaderValue, header};

let static_service = ServiceBuilder::new()
    .layer(SetResponseHeaderLayer::if_not_present(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600, must-revalidate"),
    ))
    .service(ServeDir::new("static"));

// then: .nest_service("/static", static_service)
```

Why 1 hour and not longer? Without hash-in-filename cache-busting, a shorter TTL is our deploy propagation window. 1 hour means users on the old CSS will pick up the new one within an hour of a deploy. If we later adopt hashed filenames (`app.<hash>.css`), we can bump this to a year + `immutable`.

Enable `set-header` feature on `tower-http` in `Cargo.toml`, add `tower = "0.5"` for `ServiceBuilder`.

## Non-goals (explicitly out of scope for this PR)

- **CDN.** We're behind Tailscale + Cloudflare Tunnel; the tunnel itself does some caching. A proper CDN for static assets is a bigger deployment change and doesn't belong bundled with Lighthouse fixes.
- **Image optimization / WebP / AVIF.** The app doesn't serve many images today; property photos on `/integrations/tmo/loans/{n}` use S3. If we revisit perf after these fixes and images are the new bottleneck, that's a separate plan.
- **Critical CSS inlining.** We could extract the above-the-fold rules and inline them. Saves another ~100-200ms but adds real complexity. Revisit if we're still not at 95+ after the above.
- **HTTP/3, early hints (103), resource hints beyond preload.** All real wins, all Traefik/Coolify config, none fit with a code-only PR.

## Verification plan

1. Build locally: `docker build -t fin-stream:perf-test .`
2. Check the built CSS size: `docker run --rm --entrypoint ls fin-stream:perf-test -l /app/static/app.css` — expect 30–80 KB.
3. Run container with `DATABASE_URL=...` pointing at dev DB, curl `/login`:
   - `curl -I -H 'Accept-Encoding: br' http://localhost:3000/login` → expect `Content-Encoding: br`.
   - `curl http://localhost:3000/login | grep -c 'app.css'` → expect 1 match, 0 matches for `daisyui.css` or `tailwindcss.js`.
4. Re-run Lighthouse through the SSH-tunnel approach from this plan.
5. Target metrics: perf ≥95, FCP <1.5s, LCP <2s, total page weight <300 KiB.

## Rollout

- Not a data migration, not a schema change, not a breaking API change. Safe to merge + deploy normally.
- Zero downtime expected (Coolify rolling deploy).
- If the purged CSS accidentally drops a class we actually use, some UI element will look wrong. Mitigation: Tailwind v4's `content` auto-detection covers `templates/**/*.html` by default; we add explicit `@source` directives if any classes are constructed dynamically in Rust strings (audit for `format!` / `concat!` touching class names before merging).

## Measurement after

Log the post-fix Lighthouse score in `docs/plans/completed/perf-lighthouse-fixes.md` when archiving this plan, including:
- Mobile + desktop perf scores
- FCP, LCP, Speed Index, TBT, CLS
- Total page weight
- Single largest asset

So the next perf pass has a baseline.

## Actual results (measured 2026-04-24 on the built container)

Ran the full Docker build locally, spun it up against a throwaway Postgres sidecar, pointed Lighthouse mobile at `/login`:

```
BEFORE:  PERF  60  FCP 7.1s  LCP 7.1s  SI 7.1s  page 1,752 KiB
AFTER:   PERF 100  FCP 0.9s  LCP 1.4s  SI 0.9s  page     74 KiB
```

**23× smaller page weight. 5-8× faster paint. Perfect score.**

Wire-level verification on the new image:

```
/login                                   2,467 B → 736 B (br, -70%)
/static/app.css                         96,632 B → 16,862 B (br, -83%)
/static/favicon.ico                    631,395 B → 5,430 B (-99%)
/static/vendor/fonts/*.woff2          already compressed (woff2), preloaded
Cache-Control: public, max-age=3600, must-revalidate on /static
Cache-Control: private, max-age=60 on /login (axum default, fine)
```

Only remaining Lighthouse opportunity: "Use HTTP/2" (150ms), which is a Traefik/Coolify config and out of scope for a code-only PR.
