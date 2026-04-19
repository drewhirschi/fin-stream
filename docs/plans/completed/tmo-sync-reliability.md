# TMO sync reliability

Three related bugs in the TMO sync + scheduler stack. Tackle as one plan because they share surface area (`src/scheduler.rs`, `src/crypto.rs`, `src/filters.rs`, and the sync run templates).

## Problems

1. **"Failed to decrypt secret" errors** when TMO sync runs.
2. **Scheduler is only firing once a day (9pm)** even though the intent is 4×/day.
3. **Run timestamps render in UTC**, not the viewer's local timezone.

## Investigation findings (baseline)

### Decrypt errors
- `src/config.rs:40-45` — `app_encryption_key()` reads `APP_ENCRYPTION_KEY` env var and falls back to the hardcoded dev string `"trust-deeds-dev-only-encryption-key"` when unset.
- `src/crypto.rs:9-12` — `cipher()` derives the AES key via SHA-256 of whatever `app_encryption_key()` returns.
- `src/db/integrations.rs:105-150` — `get_or_bootstrap_tmo_credential()` encrypts on first sync, decrypts on later syncs. `decrypt_string()` (crypto.rs:41) is where "failed to decrypt secret" is raised.
- `key_version` column exists on `intg.tmo_credential` but is hardcoded to `1` — no rotation path.

**Hypothesis**: The Coolify container has been redeployed without `APP_ENCRYPTION_KEY` consistently set, so a bootstrap encrypted with the dev key is later being read with a real key (or vice versa). Silent fallback to the dev string makes this worse — an unset env var looks like it "works" on first sync.

### Scheduler
- `src/scheduler.rs:19-34` — `run()` is a pure in-memory loop. It queries cadence every tick, sleeps until next fire, and has no persistent queue.
- `src/scheduler.rs:38-98` — `tick()` loads cadence, computes next fire via the cron parser, sleeps.
- `src/db/integrations.rs:~277-286` — `list_scheduled_connections()` returns `(slug, sync_cadence)` from DB. Cadence is stored as a TEXT cron string.
- `src/routes/sync.rs:~370+` — `update_integration_sync_cadence()` writes the cron string.

**Hypothesis**: Two compounding issues:
1. The cadence in the DB is almost certainly `"0 21 * * *"` (once daily at 9pm UTC) rather than a 4×/day cron.
2. Even with a correct cron, the scheduler loses pending fires on container restart because it's purely in-memory. Coolify redeploys around 9pm would explain the "only the 9pm run" pattern if the cron is 4×/day and the restart happens shortly before the next slot.

### Timezone
- `src/filters.rs:85-105` — the `datetime` filter parses an RFC3339 string and formats directly with `"%m-%d-%Y %I:%M %p"`. It never converts to the viewer's timezone.
- `src/tmo/sync.rs:8,21` — timestamps are written as `chrono::Utc::now().to_rfc3339()` (correct — UTC in DB).
- `templates/integration_sync.html:79-80`, `templates/sync_logs_partial.html:4-5` — consumers of the filter.

## Fix

### Fix 1 — decrypt errors

- **Remove the silent dev-key fallback in production.** Change `src/config.rs` `app_encryption_key()` so that when `APP_ENCRYPTION_KEY` is unset *and* we're not running in debug mode, boot fails with a clear error. Keep the dev fallback only under `cfg!(debug_assertions)` or an explicit `APP_ENV=dev` guard.
- **Surface encrypt/decrypt errors with context.** Wrap `decrypt_string` callers so the error includes the `key_version` from the DB row and logs (never prints) the first few chars of the env var fingerprint (e.g., SHA-256 hex truncated to 8) so we can diff keys across deploys without leaking them.
- **Add a `/health/crypto` route (behind auth)** that round-trips an encrypt→decrypt so deploys can be validated immediately after restart.
- **Document the key requirement** in `docs/plans/completed/` deployment notes and in `CLAUDE.md` deployment section: `APP_ENCRYPTION_KEY` must be set in Coolify env vars and must not be rotated without a re-bootstrap path.
- **Recovery path**: if existing rows are undecryptable, add a `POST /integrations/tmo/reset-credential` (admin-gated) that wipes `intg.tmo_credential` rows and re-bootstraps from `TMO_ACCOUNT`/`TMO_PIN` env vars on next sync.

### Fix 2 — scheduler cadence and restart resilience

- **Switch cadence from a user-entered cron string to a typed choice** (e.g., `hourly`, `every_6h`, `4x_daily`, `daily`). Store as TEXT but normalize on read. This avoids future "oh, it was `0 21 * * *` not `0 */6 * * *`" mistakes.
- **Change the default cadence for TMO to `every_6h`** (00:00, 06:00, 12:00, 18:00 UTC). A user-facing setting can override.
- **Backfill missed runs on boot**: on scheduler startup, read `last_successful_started_at` from `sync_log` per connection and, if a scheduled slot was missed while the process was down (within a small window — say 2 hours), fire immediately. Implement by computing the most recent cron fire time before `now()` and comparing to last success.
- **Write next-fire to DB after each tick** (`intg.integration_connection.next_scheduled_at TIMESTAMPTZ`) purely as observability so the UI can show when the next run is expected. Don't use it as the source of truth (cron is).
- **Show next-run time on the integration sync page** (above the runs table per the IA plan).

### Fix 3 — timezone-aware rendering

- Render timestamps client-side in the viewer's timezone using a small progressive-enhancement script. Server emits a machine-readable timestamp; the browser formats it.
- In `src/filters.rs`, add a new filter `datetime_tz(rfc3339: &str, tz_name: Option<&str>)` that *also* emits the ISO string in a `<time datetime="…">` element with a CSS class and a fallback formatted value (US `MM-DD-YYYY hh:mm AM/PM` per CLAUDE.md rules, but suffixed "UTC" if no tz was provided).
- Add a tiny Alpine/vanilla JS helper in `static/` that upgrades `<time data-local>` elements to the viewer's local time via `Intl.DateTimeFormat` on DOMContentLoaded and on `htmx:afterSwap`.
- Update consumers: `templates/integration_sync.html:79-80`, `templates/sync_logs_partial.html:4-5`, and any other sync/run timestamp renders (`templates/integration_overview.html` recent payments, `templates/integration_detail.html` last-run line).
- Also emit the TZ abbreviation (e.g., "EDT") next to the formatted value for clarity.

## Acceptance

- Restart the app container in dev with a changed `APP_ENCRYPTION_KEY` → the app refuses to boot (does not fall back silently).
- After bootstrapping TMO credentials, restart the container with the same key → next sync succeeds, no decrypt error in logs.
- Set cadence to "every 6h" via the UI → `sync_log` shows 4 runs spaced ~6h apart over a 24h window.
- Kill and relaunch the process during a window that just missed a fire → on startup, the scheduler fires the missed run once.
- Load integration sync page from a US/Eastern browser → timestamps render in EDT/EST, not UTC. Reload after system TZ change → re-renders.

## Out of scope

- Cross-user-per-account timezone (single-user app; lean on the browser).
- Multi-key encryption rotation — document the one-time rebootstrap path, don't build key rotation yet.
