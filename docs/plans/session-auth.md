# Session Auth + Narrow Webhook Exposure

## Context

The app runs on Coolify on a home server (`gory`) behind Tailscale today with zero authentication. A public endpoint is needed for Resend webhooks (already built at `src/routes/webhooks.rs`, not yet wired in).

**Access model (preserve as-is):**
- Coolify dashboard: `gory` on port 80 via Traefik host routing (LAN + Tailscale)
- Trust Deeds app: Tailscale-only hostname through Coolify's Traefik
- No port forwarding on the home router — only the webhook subdomain is exposed publicly, via Cloudflare Tunnel

**Layered approach:**
1. **Network layer** — Main app stays Tailscale-only. A Cloudflare Tunnel exposes *only* `webhooks.<domain>` publicly, which the Coolify reverse proxy routes to `/webhooks/resend` on the app. No ports opened on the gateway; DNS stays at current registrar via CNAME to `*.cfargotunnel.com`.
2. **App layer** — Session-based password auth on all non-webhook routes. Defense in depth for when trusted users get added soon, and protects data even if Tailscale is misconfigured or a device is compromised.

Auth approach: cookie-based password sessions using `tower-sessions` with Postgres-backed storage. A `require_auth` middleware protects all routes except health checks, the webhook endpoint, the login page, and static files.

---

## Dependencies — `Cargo.toml`

Add:
```toml
argon2 = "0.5"
tower-sessions = "0.14"
tower-sessions-sqlx-store = { version = "0.14", features = ["postgres"] }
```

Argon2id for password hashing (sha2 is not a password hash). `tower-sessions` gives native Axum 0.8 `Session` extractor; the sqlx store reuses the existing `PgPool`.

---

## 1. DB Migration — `src/db/mod.rs`

Add to end of `run_migrations()`:

```sql
CREATE TABLE IF NOT EXISTS app_user (
    id            BIGSERIAL PRIMARY KEY,
    email         TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,
    display_name  TEXT,
    is_active     INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
    updated_at    TEXT    NOT NULL DEFAULT TO_CHAR(NOW() AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')
);
```

After migrations, call `ensure_admin_user(&pool)` — reads `ADMIN_EMAIL` + `ADMIN_PASSWORD` env vars, hashes with Argon2id, inserts with `ON CONFLICT (email) DO NOTHING`. Logs a warning at startup if no users exist and no admin env vars are set.

The session store table is auto-created by `tower-sessions-sqlx-store` via `PostgresStore::migrate()`.

## 2. DB Queries — new `src/db/users.rs`

Register as `pub mod users;` in `src/db/mod.rs`.

- `get_user_by_email(pool, email) -> Option<(i64, String, String)>` — returns (id, email, password_hash)
- `get_user_by_id(pool, id) -> Option<(i64, String, Option<String>)>` — returns (id, email, display_name)

## 3. Auth Module — new `src/auth.rs`

Register as `pub mod auth;` in `src/lib.rs`.

- `hash_password(password: &str) -> Result<String>` — Argon2id with random salt
- `verify_password(password: &str, hash: &str) -> Result<bool>` — Argon2id verify
- `require_auth` middleware fn:
  - Reads `session.get::<i64>("user_id")`
  - If present: continue to handler
  - If absent + HTMX request (`HX-Request: true`): return 200 with `HX-Redirect: /login` header (prevents HTMX from swapping login HTML into a partial target)
  - If absent + API request (`/api/*` path or `Accept: application/json`): return 401 JSON `{"error": "unauthorized"}`
  - If absent + browser request: 302 redirect to `/login`

## 4. Auth Routes — new `src/routes/auth.rs`

Register as `pub mod auth;` in `src/routes/mod.rs`.

```rust
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
}
```

- **GET /login** — render `LoginTemplate`. If session already has `user_id`, redirect to `/`.
- **POST /login** — accept `Form { email, password }`. Look up user by email, verify password. On success: `session.insert("user_id", id)`, redirect to `/`. On failure: re-render login with error.
- **POST /logout** — `session.flush()`, redirect to `/login`.

## 5. Login Template — `templates/login.html`

Standalone page (does NOT extend `base.html` — no sidebar for unauthenticated users). Same `<head>` as base: DaisyUI, Tailwind, vault.css.

Centered card on dark background: "Trust Deeds" heading (Manrope), email field, password field, sign-in button (emerald-pulse), optional error alert. No registration link — admin creates users.

## 6. Template Struct — `src/templates/mod.rs`

```rust
#[derive(Template)]
#[template(path = "login.html")]
pub struct LoginTemplate {
    pub title: String,
    pub error: Option<String>,
}
```

Add to `impl_into_response!` macro.

## 7. Config — `src/config.rs`

Add:
```rust
pub fn admin_email() -> Option<String>      // ADMIN_EMAIL
pub fn admin_password() -> Option<String>   // ADMIN_PASSWORD
```

Session secret: derive from existing `APP_ENCRYPTION_KEY` using `SHA-256("session:" + key)`. No new env var needed.

## 8. Router Restructure — `src/main.rs`

```rust
// Session store
let session_store = PostgresStore::new(pool.clone());
session_store.migrate().await?;

let session_layer = SessionManagerLayer::new(session_store)
    .with_name("__td_session")
    .with_same_site(SameSite::Lax)
    .with_http_only(true)
    .with_expiry(Expiry::OnInactivity(time::Duration::days(7)));

// Public — no auth check
let public = Router::new()
    .merge(routes::health::router())
    .merge(routes::webhooks::router())
    .merge(routes::auth::router());

// Protected — auth middleware
let protected = Router::new()
    .merge(routes::media::router())
    .merge(routes::pages::router())
    .merge(routes::sync::router())
    .merge(routes::api::router())
    .layer(middleware::from_fn_with_state(state.clone(), auth::require_auth));

let app = Router::new()
    .merge(public)
    .merge(protected)
    .nest_service("/static", ServeDir::new("static"))
    .fallback(routes::pages::not_found)
    .layer(session_layer)
    .with_state(state.clone());
```

Session layer wraps everything (so `/login` can read/write sessions). Auth middleware only wraps the protected group. Static files and health checks stay fully open.

Also wire up `routes::webhooks::router()` here (currently built but not merged). Add `pub mod webhooks;` to `src/routes/mod.rs`.

## 9. Wire Up — `src/lib.rs` + `src/routes/mod.rs`

- `src/lib.rs`: add `pub mod auth;`
- `src/routes/mod.rs`: add `pub mod auth;` and `pub mod webhooks;`

## 10. Cookie Security

- `Secure` flag: set based on `cfg!(debug_assertions)` — false in dev (no TLS), true in release
- `SameSite::Lax` — needed for form POSTs, prevents CSRF from third-party sites
- `HttpOnly(true)` — no JS access to session cookie
- 7-day inactivity expiry — generous for a personal tool

## 11. Password Hashing in Async Context

Argon2id is intentionally slow (~100-300ms). Wrap in `tokio::task::spawn_blocking` during login to avoid blocking the async runtime.

---

## Files Modified

| File | Change |
|---|---|
| `Cargo.toml` | Add argon2, tower-sessions, tower-sessions-sqlx-store |
| `src/lib.rs` | Add `pub mod auth;` |
| `src/config.rs` | Add `admin_email()`, `admin_password()` |
| `src/db/mod.rs` | Add `pub mod users;`, app_user migration, `ensure_admin_user()` |
| `src/routes/mod.rs` | Add `pub mod auth;`, `pub mod webhooks;` |
| `src/templates/mod.rs` | Add `LoginTemplate`, register in macro |
| `src/main.rs` | Session layer, router split into public/protected groups, wire webhooks |

## Files Created

| File | Purpose |
|---|---|
| `src/auth.rs` | Password hashing, verification, `require_auth` middleware |
| `src/db/users.rs` | User DB queries |
| `src/routes/auth.rs` | Login/logout route handlers |
| `templates/login.html` | Login page |

---

## Env Vars

| Var | Required | Purpose |
|---|---|---|
| `ADMIN_EMAIL` | First run | Seed admin user email |
| `ADMIN_PASSWORD` | First run | Seed admin user password (hashed at startup) |

No `SESSION_SECRET` needed — derived from `APP_ENCRYPTION_KEY`.

---

## Verification

### App-level auth
1. `cargo build` — compiles
2. Set `ADMIN_EMAIL` + `ADMIN_PASSWORD` in .env, `cargo run`
3. Visit `/` — redirected to `/login`
4. Visit `/health` — returns "ok" (no auth)
5. Login with admin creds — redirected to dashboard, session cookie set
6. All pages accessible while logged in
7. POST to `/webhooks/resend` with valid Svix headers — 200 (no auth needed)
8. Open incognito, hit `/api/forecast` — 401 JSON
9. Open incognito, hit `/` — redirect to `/login`
10. POST `/logout` — redirect to `/login`, session destroyed

---

## Network Topology (Cloudflare Tunnel for the Webhook Only)

Goal: expose `webhooks.<domain>` publicly via Cloudflare Tunnel; keep main app hostname on Tailscale. No port forwarding, no NS migration.

### 1. Cloudflare account + tunnel

1. Create a Cloudflare account (if none). You don't need to transfer the domain — the zone can stay in Cloudflare as a minimal "free" zone, OR skip adding the zone entirely and use the `*.cfargotunnel.com` address directly.
2. In **Zero Trust → Networks → Tunnels**, create a tunnel named e.g. `fin-stream-webhooks`. Copy the tunnel token.
3. Note the tunnel's UUID-based `cfargotunnel.com` address (e.g. `abc123-def.cfargotunnel.com`).

### 2. DNS at your current registrar

Add a CNAME at your existing DNS provider (Namecheap/Porkbun/etc.):

```
webhooks.<domain>   CNAME   abc123-def.cfargotunnel.com
```

No NS change needed. Cloudflare won't proxy or WAF this record (it's not in a Cloudflare-managed zone), but that's fine — the tunnel is the protection boundary.

### 3. Cloudflared as a Coolify service

Add a `cloudflared` container alongside the app in Coolify. Two ways:

**Option A: Coolify "Service" (docker-compose style)**

Add a compose service:

```yaml
services:
  cloudflared:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel --no-autoupdate run --token ${CF_TUNNEL_TOKEN}
    networks:
      - coolify
```

Put `CF_TUNNEL_TOKEN` in Coolify env vars (marked secret).

**Option B: Standalone Coolify application** pointing at the `cloudflare/cloudflared:latest` image with the same command + token.

### 4. Tunnel public hostname → app

In the Cloudflare Zero Trust dashboard for this tunnel, add a public hostname:

- **Subdomain:** `webhooks`
- **Domain:** `<domain>` (or pick one Cloudflare manages; if using bare CNAME to cfargotunnel, skip this and set up "unmanaged" routing via a config file instead)
- **Service:** `http://<coolify-app-internal-host>:3000`
  - In Coolify's default Docker network the app is reachable by its service name, e.g. `http://trust-deeds:3000`.

Optional but recommended — restrict what paths this tunnel accepts:

- **Access → Applications** in Zero Trust: create a "Self-hosted" app for `webhooks.<domain>/webhooks/*` with a **Service Auth** policy that blocks by default. Since Resend POSTs without auth, you can't require Cloudflare Access on the path itself — instead, rely on the `Svix` signature check in `webhooks.rs`.
- OR add a tunnel-level rule that only forwards requests whose path starts with `/webhooks/` and returns 404 otherwise. This prevents someone from hitting `/api/*` or `/` through the public hostname even without auth.

### 5. Coolify reverse-proxy / hostname split

Coolify's embedded Traefik maps each app to one or more hostnames. Two hostnames on the same app:

- `trust-deeds.tailnet.ts.net` (or whatever Tailscale serves) — full app, Tailscale-only. Bound to the Tailscale interface.
- `webhooks.<domain>` — served only through the Cloudflare tunnel.

In Coolify: go to the app → **Domains** — add both hostnames. Coolify generates Traefik labels automatically. For the Tailscale hostname, no public DNS record exists, so it's only reachable via the tailnet. For `webhooks.<domain>`, the public DNS points at the tunnel, so it's only reachable via Cloudflare.

### 6. Path-level restriction on the public hostname (recommended)

Add a Traefik middleware (or cloudflared ingress rule) that returns 404 for anything other than `/webhooks/*` on `webhooks.<domain>`. Easiest place: the tunnel config. Example `config.yml` for cloudflared:

```yaml
ingress:
  - hostname: webhooks.<domain>
    path: ^/webhooks/.*
    service: http://trust-deeds:3000
  - hostname: webhooks.<domain>
    service: http_status:404
  - service: http_status:404
```

This way even if someone discovers the tunnel hostname, only `/webhooks/*` reaches the app.

### 7. Resend dashboard

- Add webhook URL: `https://webhooks.<domain>/webhooks/resend`
- Copy the signing secret → `RESEND_WEBHOOK_SECRET` in Coolify env vars
- Send a test webhook from Resend → verify 200 in logs

### Env Vars (Coolify)

| Var | Purpose |
|---|---|
| `CF_TUNNEL_TOKEN` | Cloudflared connection token (secret) |
| `RESEND_WEBHOOK_SECRET` | Svix signing secret from Resend dashboard |
| `ADMIN_EMAIL` / `ADMIN_PASSWORD` | Seed the first login user |

### Network Verification

1. From Tailscale-connected device: `curl https://trust-deeds.tailnet.ts.net/health` → `ok`
2. From Tailscale-connected device: `curl https://webhooks.<domain>/health` → 404 (path not in tunnel ingress)
3. From phone on cellular (no Tailscale): `curl https://trust-deeds.tailnet.ts.net/` → connection fails
4. From phone on cellular: `curl -X POST https://webhooks.<domain>/webhooks/resend` with no Svix headers → 401
5. Resend test webhook → app logs show successful insert into `received_email` table

---

## Future: opening the app beyond Tailscale

When ready to share with a few people, just add the main hostname to the same tunnel (or a second tunnel) with a Cloudflare Access policy in front. The session auth already built protects the app data; Cloudflare Access adds an SSO layer so you don't have to hand out passwords.
