# Trust Deeds

Personal income projection tool. Rust web app using the scaffold pattern from `/Users/drew/w/rust-web-scaffold/`.

## Stack
- Rust (edition 2024) + Axum 0.8 + Tokio
- SQLite via sqlx (runtime-tokio)
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
- `src/db/` — SQLite init, migrations, helpers
- `templates/` — HTML templates (Askama)
- `static/` — CSS, vendored JS
- `data/` — SQLite database (gitignored)

## Environment
- `TMO_ACCOUNT` and `TMO_PIN` must be set to sync from The Mortgage Office
- `TMO_COMPANY_ID` defaults to "vci"
- `DATABASE_URL` defaults to `sqlite:data/income.db?mode=rwc`

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
