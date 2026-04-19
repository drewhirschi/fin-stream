# Integrations + dashboard information architecture

Three related UI reorganizations that all shift responsibility *away from the global views and into the per-integration workspace*. Bundled because they touch the same nav, layout, and route surface.

## Problems

1. **"Sync" entry in the global left nav is confusing** — sync is always per-integration; it shouldn't be at the top level.
2. **Integration sync page** wastes horizontal space with a vertical settings card next to a runs table. On phones it's fine; on desktop the settings should be a horizontal strip above the table so the full table width is usable.
3. **Main dashboard (`/`) is entirely TMO-specific** (portfolio value, yield, YTD interest, cash in trust, pending checks, active loans, recent payments). Those belong on the TMO integration overview; the global dashboard should be blank / placeholder pending a real multi-integration summary.

## Investigation findings

### Global nav
- `templates/base.html:58-85` — global nav links: Dashboard, Integrations, Inbox, Timeline, Canvas, Streams, **Sync** (to remove, line 82-85).
- Global `/sync` route: `src/routes/sync.rs:44` (`sync_page`), renders `templates/sync.html`.
- Legacy "Run a sync" empty-state links in: `templates/index.html:58`, `templates/integrations.html`, `templates/integration_loans.html`, `templates/integration_payments.html`. These must be retargeted at integration-scoped sync URLs (`/integrations/tmo/sync`) instead of `/sync`.
- Integration-scoped sync already exists: `/integrations/{slug}/sync` → `src/routes/pages.rs:491` (`integration_sync`), renders `templates/integration_sync.html`.

### Integration sync layout
- `templates/integration_sync.html`:
  - Lines 9-49 — vertical settings card (cron input, "Sync now", status div).
  - Lines 50-101 — runs table.
  - Outer grid: `grid grid-cols-1 xl:grid-cols-[20rem_minmax(0,1fr)] gap-6`.

### Dashboard stats
- `templates/index.html:10-51` — six stat cards (Portfolio Value, Yield, YTD Interest, Cash in Trust, Pending Checks, Active Loans) and a recent payments section.
- `src/routes/pages.rs:73-103` (`index()` handler) — queries `portfolio_snapshot` (latest row), `db::loans::get_active_loans()`, `db::events::get_recent_payments()`.
- `templates/integration_overview.html:4-126` — currently shows an "At a glance" card (loans count, recent payments count, pending imports count) at lines 22-40 and a "Recent payments" card at lines 85-121.
- `src/routes/pages.rs:121-145` (`integration_overview()`) — does **not** currently query `portfolio_snapshot`. Needs to.

## Fix

### Fix 1 — remove global `/sync` nav

- Delete the Sync nav item in `templates/base.html:82-85`.
- Retarget the four "Run a sync" empty-state links to `/integrations/tmo/sync` (hardcode `tmo` — it's the only real integration; when we add more, reconsider).
- Decide on the `/sync` route and `templates/sync.html`:
  - **Preferred**: remove the route and template entirely; its value (global sync runs across all integrations) is better served by the per-integration sync page for now.
  - If we want to keep it, leave it unlinked as a debug-only URL (no action needed after the nav removal).
  - Recommendation: delete `src/routes/sync.rs`'s `sync_page` handler + route registration and `templates/sync.html`. Keep the POST endpoints it exposes (`run_sync`, `update_integration_sync_cadence`) — those are HTMX targets used by the integration sync page.

### Fix 2 — horizontal settings strip above the runs table

- In `templates/integration_sync.html`, restructure from a 2-column grid to a vertical stack:
  - Top: a horizontal settings card with label + cadence input + "Sync now" button laid out with `flex flex-col sm:flex-row sm:items-end gap-4`. On phone it wraps to vertical; on desktop it's one row.
  - Add a "Next scheduled run" readout in this strip (wired to the `next_scheduled_at` observability field from the TMO sync reliability plan — if that plan hasn't landed yet, omit gracefully).
  - Below: the runs table, full-width.
- Make the cadence input a typed select (hourly / every 6h / every 12h / daily) once the TMO sync reliability plan lands. Until then, keep the cron textbox but add a helper text line underneath explaining accepted format.

### Fix 3 — move TMO stats into TMO integration overview, blank main dashboard

**Step 1 — integration overview gains the six stat cards + recent payments.**

- Copy the six stat cards (lines 10-51 of `templates/index.html`) into `templates/integration_overview.html` at the top of the page, above the "At a glance" card. Use the existing card/grid styling already present on that page for consistency.
- Extend `integration_overview()` in `src/routes/pages.rs:121-145` to also query `portfolio_snapshot` (latest row) and active loans count. Feed into a shared view model. Recent payments is already there (lines 85-121 of the template) — keep it.
- The "At a glance" card (lines 22-40 of `integration_overview.html`) now partially overlaps with the new cards. Remove the redundant "Loans" and "Recent payments count" tiles; leave "Pending imports" since it's integration-specific.

**Step 2 — main dashboard becomes a deliberately empty placeholder.**

- Replace the content of `templates/index.html` with a minimal placeholder: a one-line hero ("Welcome back") and a link/CTA directing to the TMO integration. No stat cards, no payments table.
- In `src/routes/pages.rs:73-103` (`index()`), remove the `portfolio_snapshot` and recent-payments queries; the handler should just render the simplified template. Leave the query helpers (`db::loans::get_active_loans`, `db::events::get_recent_payments`) in place — they're now only used by the integration overview.
- Add a short code comment on the handler noting that the dashboard is intentionally empty until multi-integration summary is designed (**remove that exception to the "no comments" rule only if it's non-obvious to the next reader; otherwise skip it**).

## Acceptance

- Global nav no longer contains "Sync"; no empty-state page links to `/sync`.
- `/sync` either 404s cleanly or redirects to `/integrations/tmo/sync`.
- Integration sync page on desktop shows settings strip above a full-width runs table; on mobile the strip wraps.
- TMO integration overview shows Portfolio Value / Yield / YTD Interest / Cash in Trust / Pending Checks / Active Loans + Recent Payments.
- Main dashboard renders a minimal placeholder — no TMO numbers.
- Links from the dashboard to the TMO integration are clear (CTA or nav callout).

## Out of scope

- Multi-integration dashboard summary (deliberate: rebuild once a second integration exists).
- Reworking the "pending imports" concept (lives on integration overview; leave alone).
