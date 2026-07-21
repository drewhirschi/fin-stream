# Trust Deeds

Personal income and trust-deed portfolio workspace. The main application is the repository-root NextRS app.

## Stack
- Rust 2024 + NextRS 0.4 (git-pinned for route telemetry) + Axum 0.8 + Tokio
- React 19 TypeScript route components in `app/`
- Shared React UI package in `client/`
- libSQL/Turso persistence with checksum-verified migrations in `migrations/`
- Tailwind CSS 4 and shadcn-style components
- Vercel Rust function in `api/index.rs`; local server in `src/main.rs`

## Development
```sh
cp .env.example .env
npm ci --prefix client --no-audit --no-fund
npm run --prefix client css
cargo install cargo-nextrs-dev # once per machine
cargo dev
```

The local app serves on port 3003 by default. `cargo dev` is the canonical
NextRS rebuild/restart and full-page live-reload workflow. Its default build
uses the Turso environment configured in `.env`; local SQLite is an explicit
`--no-default-features --features local-db` mode.

## Project structure
- `app/` — NextRS React pages, layouts, and Rust route handlers
- `client/src/` — shared React shell, components, API client, formatting helpers, and CSS
- `src/` — Rust domain logic, libSQL repositories, auth, providers, sync, and media
- `api/` — Vercel serverless entry point
- `migrations/` — authoritative libSQL schema
- `public/` — generated client bundles and static assets
- `templates/` — transitional Askama responses used by Rust fallback/compatibility handlers
- `tools/` — one-shot PostgreSQL/Turso and object cutover tools; excluded from deployment

## Mobile
- Dashboard, timeline, inbox, integrations, loans, and payments must work at a 320px viewport.
- Prefer stacked cards for primary mobile data flows. Dense operational tables may scroll horizontally.
- Interactive controls should provide roughly 44px touch targets on coarse pointers.
- Prevent inputs below 16px on phones to avoid iOS form zoom.
- Keep horizontal sub-navigation scrollable and the application drawer keyboard-dismissible.

## Display formatting
- React views must use the shared `date`, `dateTime`, and `money` helpers in `client/src/lib/utils.ts`.
- Rust-rendered compatibility views must use the filters in `src/filters.rs`.
- Do not show raw ISO dates or hand-format user-visible currency.

## Deployment
- The repository root is the Vercel project root.
- `vercel.json` builds the React client and the remote-libSQL Rust function.
- Production uses Turso and private S3-compatible object storage.
- The historical PostgreSQL/Coolify application is not part of the main build; its cutover procedure is retained in `CUTOVER.md`.

## gstack
Use the `/browse` skill from gstack for all web browsing. Never use `mcp__claude-in-chrome__*` tools.

Available skills: `/office-hours`, `/plan-ceo-review`, `/plan-eng-review`, `/plan-design-review`, `/design-consultation`, `/design-shotgun`, `/design-html`, `/review`, `/ship`, `/land-and-deploy`, `/canary`, `/benchmark`, `/browse`, `/connect-chrome`, `/qa`, `/qa-only`, `/design-review`, `/setup-browser-cookies`, `/setup-deploy`, `/retro`, `/investigate`, `/document-release`, `/codex`, `/cso`, `/autoplan`, `/plan-devex-review`, `/devex-review`, `/careful`, `/freeze`, `/guard`, `/unfreeze`, `/gstack-upgrade`, `/learn`.

## Skill routing
When a request matches an available skill, invoke it before ad-hoc work.

- Product ideas or brainstorming → `/office-hours`
- Bugs and errors → `/investigate`
- Ship, deploy, push, or create PR → `/ship`
- QA or test the site → `/qa`
- Code review → `/review`
- Documentation after shipping → `/document-release`
- Design systems and brand → `/design-consultation`
- Visual audit and polish → `/design-review`
- Architecture review → `/plan-eng-review`
