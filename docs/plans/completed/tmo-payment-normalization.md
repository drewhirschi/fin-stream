# Remove "Loan" From the App Layer — Streams Only

## Direction (settled)

Decision: **the app layer has no concept of a loan.** It knows only streams (of payments or expenses), events on those streams, accounts, portfolio snapshots, and user-owned data (inbox, sessions).

Loan-shaped things — borrower names, property addresses, interest rates, delinquency, featured photos, the "active loans" table on today's dashboard — are TMO-specific views and live entirely behind `/integrations/tmo/*` routes and in `intg.*` tables. TMO-specific code is fine, even welcome; it's just quarantined.

## Target invariant

> No SQL string, type, template, or function outside the integration layer references `intg.*` tables, loan-shaped columns (`borrower_name`, `principal_balance`, `is_delinquent`, `loan_account`, etc.), or TMO-specific concepts — **including keys inside JSON blobs on public tables.**
>
> Integration layer = `src/tmo/**`, `src/monarch/**`, `src/resend.rs`, `src/routes/webhooks.rs`, `src/routes/integrations.rs` (whatever file holds the `/integrations/*` handlers, currently the `integration_*` fns in `src/routes/pages.rs`), and `src/db/integrations.rs`. Migrations in `src/db/mod.rs` are exempt.
>
> App layer = everything else: dashboard index, forecast, streams, canvas, inbox (non-linking), accounts, auth.

## Normalized schema (target)

### `stream` (1:N events, hierarchy-capable)

```
stream
  id                  BIGSERIAL PK
  parent_id           BIGINT NULL REFERENCES stream(id)   -- rollup hook (see note)
  name                TEXT NOT NULL
  kind                TEXT NOT NULL      -- 'inflow' | 'outflow'
  default_account_id  BIGINT
  is_active           BOOLEAN NOT NULL DEFAULT true
  configuration       TEXT               -- app-native only
  created_at, updated_at
```

Events belong to exactly one stream (1:N). If we need "TMO inflow" as an aggregate over per-loan streams, that's a parent/child relationship, not an M:N. Adding `parent_id` now is cheap and keeps the door open; computing rollups can come later. No code needs to use `parent_id` on day one.

### `stream_event` (just dates, amount, status)

```
stream_event
  id              BIGSERIAL PK
  stream_id       BIGINT NOT NULL REFERENCES stream(id)
  account_id      BIGINT
  label           TEXT
  expected_date   DATE NOT NULL          -- when we think it will/did happen
  actual_date     DATE NULL              -- NULL until money moves
  amount          DOUBLE PRECISION NOT NULL
  status          TEXT NOT NULL DEFAULT 'projected'
                                         -- 'projected'|'confirmed'|'received'|'missed'|'canceled'
  source_type     TEXT                   -- opaque hint: 'tmo_history'|'monarch_txn'|'manual'
  source_id       TEXT                   -- opaque hint, no FK
  notes           TEXT
  created_at, updated_at
  UNIQUE(stream_id, source_type, source_id)
```

Changes from today:
- **Two dates, not three.** Drop `scheduled_date`. The app only needs "expected" and "actual." If we later want "original schedule said X, I've overridden to Y," that belongs on `stream_schedule` (the rule), not on every event.
- **No TMO-shaped JSON.** `metadata` gets dropped or restricted to app-native keys only. Today's `check_number` / `is_pending_print_check` / `loan_account` keys all leave. TMO sync keeps those in `intg.tmo_import_payment`; integration handlers join back when they need them.
- **Lateness is derived**, never stored: `actual_date IS NULL AND expected_date < today AND status IN ('projected','confirmed')`.

### Integration link tables (quarantine-respecting polymorphic FKs)

Instead of putting typed FKs (`tmo_payment_id`, `monarch_txn_id`) on `stream_event` — which would leak intg into the public schema — each integration owns a link table in `intg`:

```
intg.tmo_payment_event_link
  tmo_payment_id   BIGINT PRIMARY KEY REFERENCES intg.tmo_import_payment(id) ON DELETE CASCADE
  stream_event_id  BIGINT NOT NULL REFERENCES stream_event(id) ON DELETE CASCADE
  created_at       TIMESTAMPTZ NOT NULL DEFAULT NOW()
```

Parallel tables appear under `intg.monarch_*` etc. as integrations are added. `stream_event.source_type`/`source_id` stay as human-readable debugging hints, but the load-bearing reference is the link table. This gives real FK integrity (cascades work both ways) without the public schema knowing any integration exists.

## What's already done

- `stream_event` carries every payment with `expected_date`, `actual_date`, `status`, `source_type='tmo_history'`. Idempotent upsert via unique key `(stream_id, source_type, source_id)`.
- TMO sync (`src/tmo/sync.rs:200-284`) translates `intg.tmo_import_payment.check_number` into event status: blank → `confirmed` with `expected_date`, populated → `received` with `actual_date`.
- `intg.tmo_import_loan.stream_id` links each TMO loan to its stream.
- `src/db/events.rs` reads only from `stream_event` (with one TMO-shaped exception noted below).
- `src/db/accounts.rs` and `portfolio_snapshot` are app-native. Clean.

## What has to move (concrete inventory)

### Schema: collapse dates, drop JSON keys, add link tables

1. **Migrate `stream_event` to two dates:**
   ```sql
   UPDATE stream_event SET expected_date = scheduled_date WHERE expected_date IS NULL;
   ALTER TABLE stream_event ALTER COLUMN expected_date SET NOT NULL;
   ALTER TABLE stream_event DROP COLUMN scheduled_date;
   ```
   Update the index on `(stream_id, scheduled_date)` to use `expected_date`. Update TMO sync and any other writer to stop setting `scheduled_date`.

2. **Strip TMO-shaped keys out of `stream_event.metadata`.** Either drop the column or restrict it. Either way, TMO sync stops writing `check_number`, `is_pending_print_check`, `loan_account` into events. Templates/handlers that read those keys get rewritten to join back to intg via the link table.

3. **Create `intg.tmo_payment_event_link`** and backfill from existing `(source_type='tmo_history', source_id)` pairs. Update TMO sync to write the link row alongside the event upsert.

4. **Add `stream.parent_id BIGINT NULL`** (unused for now — hook for future rollups).

### Dashboard (`index` handler + `templates/index.html`)

**Today:** renders an "Active Loans" table sourced from `intg.tmo_import_loan` via `db::loans::get_active_loans` (`src/db/loans.rs:6-27`). The stat card "Active Loans" counts those rows. The table shows borrower, property, rate, balance, regular payment, maturity, delinquency, featured photo.

**Target:** stream-centric dashboard. Proposed content, none of it loan-shaped:

- **Cash snapshot** (already present): starting balance, portfolio_value, portfolio_yield, ytd_interest, trust_balance, outstanding_checks. All from `portfolio_snapshot`. Keep as-is.
- **Upcoming inflow (next 30 days)**: sum of `stream_event.amount` where `status IN ('projected','confirmed')`, `amount > 0`, `expected_date` between now and +30d. Break down by stream.
- **Recent activity**: last N `stream_event` rows with `actual_date` set (already available via `db::events::get_recent_payments`, keep).
- **Top streams by projected 30-day inflow**: small table or list showing `stream.name`, next expected event date, expected amount. Pure `stream` + `stream_event` join.
- **Late indicators** (optional): derived from dates (see formula above). No `is_delinquent` column needed.

The "Active Loans" table itself either disappears from the dashboard, or moves to `/integrations/tmo/loans` (which already exists). Dashboard links there for "drill into TMO loans".

### Data access layer

| Current | Action |
|---|---|
| `src/db/loans.rs::get_active_loans` | Move to `src/db/integrations.rs` as `list_active_tmo_loans`. Only caller after the reshape is `/integrations/tmo/loans`. |
| `src/db/loans.rs::get_loan_by_account` | Move to `src/db/integrations.rs` as `get_tmo_loan_by_account`. Only caller is `/integrations/tmo/loans/:account`. |
| `src/db/loans.rs` (the file) | Delete once both functions have moved. |
| `src/db/events.rs::get_payments_for_loan` (`events.rs:61-96`) | Reads `stream_event` but filters by `metadata.loan_account` — which won't exist anymore. Rewrite to join `stream_event` through `intg.tmo_payment_event_link` to `intg.tmo_import_payment` for TMO-specific fields. Move to `src/db/integrations.rs` as `list_tmo_payment_history`. |
| `src/models/mod.rs::LoanView`, `LoanDetailView`, `LoanPaymentHistoryView` | Move to an intg-scoped module, e.g. `src/models/tmo.rs` or inline with `src/db/integrations.rs`. Remove from `models/mod.rs`'s public app surface. |

### Forecast query (`src/db/forecasts.rs:134`)

Drop the `LEFT JOIN intg.tmo_import_loan` and the `is_delinquent` field entirely from the forecast response.

Replace with the derived "late" rule: `status IN ('projected','confirmed') AND expected_date < today AND actual_date IS NULL`. Computed in SQL or in the handler — either's fine.

### One-off binary (`src/bin/backfill_property_media.rs`)

Reads `intg.tmo_import_loan` directly. It's a TMO-specific maintenance tool. Either move to `src/bin/tmo_backfill_property_media.rs` and tag it intg, or delete if served its purpose. Open question.

### Cross-schema write (`src/db/streams.rs:112`)

`UPDATE intg.tmo_import_loan SET stream_id = $1 WHERE stream_id IS NULL` — streams code reaching into intg. Move to `src/db/integrations.rs` as `backfill_tmo_loan_stream_ids(pool, stream_id)`, or move the call site into TMO sync where it naturally belongs.

## Plan phases

### Phase 1 — schema migration

No UI change. Tighten the shape.

1. Migrate `stream_event`: backfill `expected_date`, drop `scheduled_date`, update indexes.
2. Add `stream.parent_id` (nullable, unused).
3. Create `intg.tmo_payment_event_link`. Backfill from existing `(source_type='tmo_history', source_id)` pairs.
4. Update TMO sync to:
   - stop writing `scheduled_date`
   - stop writing TMO-shaped keys into `stream_event.metadata`
   - write a link row in `intg.tmo_payment_event_link` on each event upsert
5. Strip or restrict `stream_event.metadata` (decision point: drop column entirely, or keep for app-native use).
6. `cargo build` + `cargo test`. Sync a sample loan end-to-end and verify event rows + link rows look right.

### Phase 2 — relocate data-access functions

No behavior change. Just move files.

1. Create/extend `src/db/integrations.rs` with:
   - `list_active_tmo_loans` (from `loans.rs:get_active_loans`)
   - `get_tmo_loan_by_account` (from `loans.rs:get_loan_by_account`)
   - `list_tmo_payment_history` (from `events.rs:get_payments_for_loan`, rewritten to join through the link table)
   - `backfill_tmo_loan_stream_ids` (from `streams.rs:112`)
2. Move `LoanView`, `LoanDetailView`, `LoanPaymentHistoryView` out of `src/models/mod.rs` into an intg-scoped module.
3. Update `/integrations/tmo/*` handlers to call the new locations.
4. Delete `src/db/loans.rs`. Remove `get_payments_for_loan` from `events.rs`.
5. `cargo build` + `cargo test`. Pure refactor, should be green.

### Phase 3 — reshape the dashboard

1. Remove the "Active Loans" table from `templates/index.html` and everything it references.
2. Remove `loans: Vec<LoanView>` from `IndexTemplate`. Add the new stream-centric fields described above.
3. New queries for the dashboard (all in `db::streams` or `db::events`):
   - `list_upcoming_stream_inflow(pool, horizon_days)` → aggregate
   - `list_top_streams_by_projected_inflow(pool, horizon_days, limit)` → per-stream rows
   - `list_late_events(pool)` → events past due with no actual_date (optional)
4. Update `index()` handler to populate the new template.
5. Visual QA: dashboard still feels useful without the loans table.

### Phase 4 — purge intg from forecast

1. Drop the `LEFT JOIN intg.tmo_import_loan` and the `is_delinquent` column from `get_forecast_events` (`src/db/forecasts.rs:134`).
2. Remove `is_delinquent` from `ForecastRow` / `ForecastResponse` / template.
3. Replace with derived lateness.
4. Visual QA on `/forecast`.

### Phase 5 — enforcement

1. Add `tools/check_intg_boundary.sh`:

   ```bash
   #!/usr/bin/env bash
   set -euo pipefail
   # Block any reference to intg.* or TMO-shaped JSON keys outside the integration layer.
   violations=$(rg -e '\bintg[._]' -e 'check_number' -e 'loan_account' -e 'is_pending_print_check' src/ \
     --glob '!src/tmo/**' \
     --glob '!src/monarch/**' \
     --glob '!src/resend.rs' \
     --glob '!src/routes/webhooks.rs' \
     --glob '!src/routes/integrations.rs' \
     --glob '!src/db/integrations.rs' \
     --glob '!src/db/mod.rs' \
     --glob '!src/bin/tmo_*.rs' \
     || true)
   if [ -n "$violations" ]; then
     echo "intg schema / TMO-shaped reference leak detected:"
     echo "$violations"
     exit 1
   fi
   ```

2. Split `src/routes/pages.rs`: move `integration_*` handlers into `src/routes/integrations.rs` so the allowlist is cleaner and the split is physical, not conventional.

3. Wire the check into CI alongside `cargo test`.

### Phase 6 — verify the quarantine works

Scratch-branch test: `ALTER SCHEMA intg RENAME TO intg_quarantined;` in a dev DB, then `cargo build && cargo run`:

- Dashboard, `/forecast`, `/streams`, `/canvas`, `/inbox`, `/login` all render and function.
- `/integrations/*` pages will 500 or show empty states — expected and fine.
- Revert the rename, app returns to full function.

This is the acceptance test for the whole refactor.

## Future: non-TMO imports (Monarch et al.)

The normalized schema above is integration-agnostic. Adding Monarch (or any new source) follows the same recipe:

1. Raw rows land in `intg.monarch_*` tables owned by `src/monarch/**`.
2. Normalizer emits `stream_event` rows with `source_type='monarch_txn'`, `source_id=<monarch id>`, `actual_date` set (imports are historical), `status='received'`.
3. Link rows in `intg.monarch_txn_event_link` tie events back to raw rows for drill-down.
4. Stream assignment is the hard part — a bank account has inflows/outflows across many streams and needs classification rules. That's a separate design (likely a `stream_rule` table or user-driven categorization), not a schema change.

The schema doesn't need any modification to support this; the integration layer grows, the app layer doesn't.

## Migration / backfill

- Phase 1 is destructive-ish (drops `scheduled_date`, may drop `metadata`). Back up the dev DB before running; prod migration runs the same SQL forward.
- After Phase 1: re-run TMO sync to repopulate link table rows and confirm no drift.
- Sanity check before starting: `SELECT count(*) FROM intg.tmo_import_payment WHERE processing_state != 'normalized'` should be `0`.

## Non-goals

- No obligor/portfolio-entity abstraction. Loans don't get a first-class app home — they stay in TMO.
- No M:N event↔stream. Hierarchy via `stream.parent_id` is the rollup story if we ever need one.
- No changes to the loan detail UI at `/integrations/tmo/loans/:account`. It can and should stay rich and TMO-shaped.
- No changes to Monarch or Resend request-path code yet.
- No change to the sync/normalize pipeline logic — it already works. This plan reshapes the storage model and the read side.

## Open questions

1. **`stream_event.metadata`**: drop the column entirely, or keep it for app-native use (user notes, manual tags)? Current vote: drop, add back if a real app-native use shows up.
2. **`backfill_property_media.rs`**: keep as TMO-tagged tool or delete?
3. **`src/routes/pages.rs` split**: move `integration_*` handlers to their own file in Phase 5 (when the allowlist needs it) or do it earlier as pure cleanup?
