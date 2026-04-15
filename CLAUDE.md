# Trust Deeds

Personal income projection tool. Rust web app using the scaffold pattern from `/Users/drew/w/rust-web-scaffold/`.

## Stack
- Rust (edition 2024) + Axum 0.8 + Tokio
- PostgreSQL via sqlx (runtime-tokio)
- Askama templates (templates/ directory)
- DaisyUI 5 + Tailwind CSS 4 (browser JIT) + HTMX 2 + Alpine.js 3
- All frontend deps vendored in static/vendor/ (no CDN)

## Development
```sh
cargo watch -x run   # hot reload (install cargo-watch first)
cargo run            # manual run, serves on PORT from .env (default 3001)
```

## Project structure
- `src/main.rs` — entry point, router, state
- `src/routes/` — Axum route handlers (pages, sync, health)
- `src/templates/` — Askama template structs
- `src/tmo/` — The Mortgage Office API client + sync engine
- `src/models/` — data types (DB models, TMO API types, view models)
- `src/db/` — PostgreSQL init, migrations, helpers
- `templates/` — HTML templates (Askama)
- `static/` — CSS, vendored JS
- `data/` — local data directory (optional, gitignored)

## Environment
- `TMO_ACCOUNT` and `TMO_PIN` must be set to sync from The Mortgage Office
- `TMO_COMPANY_ID` defaults to "vci"
- `DATABASE_URL` defaults to `postgres://postgres:postgres@127.0.0.1:5432/trust_deeds`

## Deployment (gory)

The app is deployed to a home server named `gory` running Coolify. **Everything stays behind Tailscale** — no ports are forwarded on the home router.

- **Access paths**:
  - `gory` (Tailscale MagicDNS, IP `100.82.34.75`) — remote access via VPN
  - `gory.local` (mDNS on LAN, IPs `192.168.4.x`) — local access on home network
- **Coolify dashboard**: hosted on port 80 via Traefik host-based routing. Just type `gory` in a browser.
- **Trust Deeds app**: Tailscale-only hostname via Coolify's Traefik. No public DNS.
- **Public exposure**: ONLY the webhook endpoint (`webhooks.<domain>/webhooks/resend`) via a **Cloudflare Tunnel**. See `docs/plans/session-auth.md`.
- **Coolify API**: `http://gory:8000`, token in `gory_coolify_access_token.txt` (gitignored). The `Makefile` has `make deploy/logs/status/stats/envs/build/ship` targets.
- **Container image**: `ghcr.io/drewhirschi/fin-stream:latest`, built multi-arch (amd64 + arm64) via `make build`, then `make deploy` tells Coolify to re-pull and redeploy.
- **App auth**: session-based password auth (Argon2id + `tower-sessions` + Postgres store). See `docs/plans/session-auth.md`. Webhook and health routes are public; everything else requires login.

Do NOT propose architectures that expose the dashboard or the app to the public internet directly. Public exposure is opt-in per route via Cloudflare Tunnel only.

## Display formatting
- Always use the shared Askama display filters in `src/filters.rs` for user-visible dates, datetimes, money, and grouped counts.
- Do not render raw ISO dates like `YYYY-MM-DD` in templates unless the user explicitly asks for that format.
- Do not hand-format currency in templates with `"{:.2}"`, `"{:.0}"`, or ad hoc comma logic. Use the shared money filter instead.
- U.S. display rule: dates should render month-day-year, and money should render with thousands separators. Zero dollars should display as `0`; non-zero whole-dollar amounts should display with `.00`.

## gstack
Use the `/browse` skill from gstack for all web browsing. Never use `mcp__claude-in-chrome__*` tools.

Available skills: `/office-hours`, `/plan-ceo-review`, `/plan-eng-review`, `/plan-design-review`, `/design-consultation`, `/design-shotgun`, `/design-html`, `/review`, `/ship`, `/land-and-deploy`, `/canary`, `/benchmark`, `/browse`, `/connect-chrome`, `/qa`, `/qa-only`, `/design-review`, `/setup-browser-cookies`, `/setup-deploy`, `/retro`, `/investigate`, `/document-release`, `/codex`, `/cso`, `/autoplan`, `/plan-devex-review`, `/devex-review`, `/careful`, `/freeze`, `/guard`, `/unfreeze`, `/gstack-upgrade`, `/learn`.

## Skill routing

When the user's request matches an available skill, ALWAYS invoke it using the Skill
tool as your FIRST action. Do NOT answer directly, do NOT use other tools first.
The skill has specialized workflows that produce better results than ad-hoc answers.

Key routing rules:
- Product ideas, "is this worth building", brainstorming → invoke office-hours
- Bugs, errors, "why is this broken", 500 errors → invoke investigate
- Ship, deploy, push, create PR → invoke ship
- QA, test the site, find bugs → invoke qa
- Code review, check my diff → invoke review
- Update docs after shipping → invoke document-release
- Weekly retro → invoke retro
- Design system, brand → invoke design-consultation
- Visual audit, design polish → invoke design-review
- Architecture review → invoke plan-eng-review
- Save progress, checkpoint, resume → invoke checkpoint
- Code quality, health check → invoke health
