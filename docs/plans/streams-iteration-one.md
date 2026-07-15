# Streams — Iteration One: Make It Actually Usable

**Status:** Draft (pending eng review)
**Date:** 2026-06-27

## North star

One question: **"On [date], how much cash will I have?"** A single pooled cash
balance, anchored to *today*, moved forward and backward by the money I expect to
flow **in** and **out**. Everything below serves that and nothing else (no net
worth, no per-account accounting).

This is the same Core Question as `docs/manifest.md` — that manifest needs the
same accuracy refresh as the other docs (see "Docs to fix"), but its north star
already matches and stays the anchor.

## Why this iteration exists

The core loop is already built and wired — `/streams` is a real CRUD config
surface, `/forecast` ("Timeline") has a live running-balance hero + scrubber +
composer, TMO sync feeds realized payments in, and "Sync Monarch" sets a starting
balance. It is *not usable* because of a few quiet correctness bugs sitting on top
of working plumbing. This iteration fixes those and adds the two missing pieces
(forward projection of real income + reconciliation) so the headline number can
be trusted.

## The model (decided with the owner)

Every projected payment is an **expected event** carrying two kinds of
uncertainty — *when* and *how much*. **Reconciliation** is collapsing that
uncertainty when reality arrives.

The *how-much* uncertainty splits streams into **two tracks**:

- **Known amount** — you know the number ahead of time (salary, rent, trust-deed
  payments). The projection is a hard figure.
- **Estimated amount** — you don't (credit-card payment, commission). The
  projection is a flagged *estimate* you can set/adjust, always corrected by the
  real number when it lands.

Amount-certainty is a property of the stream, not a guess made per event.

The atomic unit is the **individual expected event**, not a statistical window.
A stream is a **lane of individual, movable payments**. Schedules and integration
import only *seed* default-dated events; the events themselves are the editable
truth — each can be moved, edited, added, or removed independently.

Three archetypes, one mechanism:

| Owner's stream | In/Out | When | Amount | Notes |
|---|---|---|---|---|
| Salary | In | exact days (15th + last, or biweekly) | **known** | fixed figure |
| Trust-deed income (TMO) | In | 5 loans, each defaults to a day (e.g. 20th), nudged/reconciled to the real check | **known-ish** | net of est. fee; the "rental"-like property-secured income |
| Commission | In | irregular | **estimated** | manual estimate, corrected on landing |
| Rent | Out | fixed day | **known** | |
| Credit cards | Out | monthly payment | **estimated** | model the monthly *payment* outflow, not individual charges; corrected to the real payment |

### Decisions locked

1. **Direction is first-class.** A stream is **Income** or **Expense**. Amounts
   are entered/stored as **magnitudes**; sign is applied from the stream's
   direction at compute time. This kills today's bug where a `$500` expense
   *adds* `$500` (kind is currently cosmetic — nothing negates it).
2. **One pooled cash number**, one cash account, anchored to a real **as-of
   date** (default today). No per-account rollup.
3. **Events are individually movable / add / remove** within a stream. Default
   date seeds them; the user re-dates each as reality dictates.
4. **Reconciliation = Manual + auto-TMO.** Mark any expected event "landed" (real
   date + amount), or **manually link an integration actual to the expected event
   it fulfills**. TMO actuals auto-reconcile *into* their projected event (no
   duplicate row). Monarch/other auto-matching is iteration two.
5. **Amount certainty is a stream property** — **known** vs **estimated**.
   Estimated streams (credit cards, commission) show a flagged point estimate that
   is always corrected by the actual on reconcile. No statistical band in v1.

## Ground truth this plan builds on

Live schema is **Postgres**, built imperatively in
`src/db/mod.rs::run_migrations` (no `migrations/` dir, no `sqlx::migrate!`). Key
facts (the prose docs are stale — see "Docs to fix"):

- `stream` (`src/db/mod.rs:57`): `id, name, type, kind, description,
  default_account_id, configuration, parent_id, is_active, ...`. **No direction
  column.** `kind`/`type` never affect sign.
- `stream_event` (`src/db/mod.rs:99`): **two-date** model — `expected_date DATE
  NOT NULL`, `actual_date DATE`, `amount DOUBLE PRECISION`, `status` (`projected`
  / `confirmed` / `received`), `source_type`, `source_id`, `metadata`,
  `UNIQUE(stream_id, source_type, source_id)`. (`scheduled_date` was dropped.)
- `stream_schedule` (`src/db/mod.rs:121`): one active monthly schedule is read per
  stream (`LATERAL ... LIMIT 1`, `src/db/streams.rs:444`); generator skips any
  non-`monthly` frequency (`src/db/streams.rs:350`).
- Forecast engine (`src/db/forecasts.rs:155`): `running = starting_balance; for e:
  running += e.amount`, ordered by `expected_date`. **Real math, but
  mis-anchored** — the page requests a window starting `today-120`
  (`templates/forecast.html:619`) and treats the balance as the opening balance
  *there*, so "cash today" is only right when nothing happened in 120 days.
- Starting balance (`src/db/forecasts.rs:57`): primary `account.balance` →
  `settings.current_cash` → `portfolio_snapshot.trust_balance`. Startup seeds the
  primary account to `0.0`, so the onboarding "set your cash" card
  (`templates/forecast.html:8`, gated on `!has_balance`) **never renders**.
- TMO (`src/tmo/sync.rs`): imports only **past** payments as `tmo_history` events;
  **future income is never projected**. `intg.tmo_payment_event_link` already
  links a payment to an event row.
- Integration boundary: `intg.*` schema, enforced by
  `tools/check_intg_boundary.sh`. Streams/cash live in `public.*`.

## Iteration-one scope

### A. Direction (in/out) as a first-class concept
- Add `direction` to `stream` (`'in'` | `'out'`), set from the existing
  Income/Expense kind. Backfill from current `kind`/`type`.
- Store `stream_event.amount` as a **magnitude**; apply sign from the owning
  stream's direction in `compute_forecast` (`src/db/forecasts.rs:155`) and in the
  per-day inflow/outflow breakdown. Migrate existing rows to magnitudes.
- UI: replace "type a negative number" with an explicit Income/Expense control;
  amount inputs accept positive numbers only.
- Credit cards **are in v1** as single-direction **Expense** streams that model
  the monthly *payment* outflow (estimated amount), not individual charges.

### B. Cash anchored to today (fix the headline number)
- Give the cash balance a real **as-of date** (today by default); store it
  alongside the balance.
- Re-anchor `compute_forecast`: the balance is correct **as of** its date; sum
  forward for future events, subtract backward to reconstruct the past. "Cash on
  today" must equal the entered cash-on-hand when no future events have landed.
- Replace the dead onboarding card with an **always-available "Cash on hand: $X
  as of [date] — edit"** control on `/forecast`. Treat seeded `$0` /
  never-confirmed as "needs setting" so a new user is prompted.
- Single source of truth for the anchor; stop silently falling back to TMO
  `trust_balance` without indication.

### C. Per-stream event management (move / add / remove individually)
- A per-stream view of its upcoming events (the lane), each row editable.
- **Move** one event's date (PATCH override already exists at
  `src/db/events.rs:61`), **edit** amount, **add** a one-off event to the stream,
  **remove** an event.
- Add the missing **`DELETE /api/events/{id}`** (only POST/PATCH exist today,
  `src/routes/api.rs:17`).

### D. Recurrence that fits real streams
- Read **all** active schedules per stream (drop the `LIMIT 1`), and/or let one
  stream own several seed rules.
- Frequencies: `monthly` (exists) + **semi-monthly** (15th + last), **biweekly**
  (anchor date), **annual**, and **one-time-on-a-date**. Generator
  (`src/db/streams.rs:320`) emits one default-dated event per occurrence; each is
  then individually movable.
- Surface `end_date` (column exists, no UI today).

### E. Import + project TMO income forward, then reconcile
- For each active TMO loan, project a monthly expected event (default day 20,
  amount = `regular_payment` net of estimated service fee) into the trustee
  stream — 5 loans → 5 movable events/month.
- **Auto-reconcile:** when a real check lands in TMO sync (`src/tmo/sync.rs:207`),
  match it to the open projected event (by stream + loan + period, date
  tolerance) and **collapse in place** — fill `actual_date` + real amount, mark
  landed, no duplicate row. Reuse/repoint `intg.tmo_payment_event_link`.
- Keeps the `intg` boundary intact: integrations write **actuals**, streams own
  **expectations**, reconciliation is the matching layer in `public`.

### F. Delete / archive streams
- Add a deactivate/delete path (none exists today) so mis-created streams don't
  pollute every view forever.

### G. Amount certainty (known vs estimated)
- Add an amount-certainty property to streams (`known` | `estimated`).
- Known streams project a hard figure; estimated streams project a **flagged
  point estimate** (a typical amount you set, e.g. "~$1,500" credit card), shown
  distinctly so a guess never reads as a fact.
- On reconcile, the actual always overwrites the estimate; estimated streams are
  expected to carry variance and are the prime target for v2 auto-tuning.

## Out of scope (iteration two+)

- Auto-matching **Monarch** (and any non-TMO) actuals to expectations.
- Variance **history** + "your rental usually lands on the 27th" auto-tuning of
  default dates.
- Proactive **"did it land?" nudges** for overdue expecteds.
- Modeling individual credit-card *charges* (v1 models the monthly payment
  outflow only); statistical amount **bands** for estimates; multi-account cash;
  multi-user.
- Monarch transaction import as streams; Monarch scheduler/auto-sync; Monarch
  detail UI (today its pages are gated to `slug == 'tmo'`).

## Data-model changes (summary)

- `stream`: add `direction TEXT` (`'in'`/`'out'`) and `amount_certainty TEXT`
  (`'known'`/`'estimated'`); backfill from `kind`.
- `stream_event`: amounts normalized to **magnitude**; reconciliation collapses
  in place (no schema change strictly required — `actual_date`/`status` exist).
  Add a `DELETE` path.
- `stream_schedule`: support multiple active per stream; add `semimonthly`,
  `biweekly`, `annual`, `one_time` frequencies; expose `end_date`.
- Cash anchor: persist an explicit **as-of date** with the balance; converge on a
  single source of truth.
- TMO: generate forward projected events per loan (new `source_type`, e.g.
  `tmo_projected`); reconcile actuals into them.

## Docs to fix (part of "get the docs to actual state")

- `docs/data-model.md` — **regenerate from live schema.** Currently dead wrong
  (claims SQLite / INTEGER / REAL / three-date model / macOS Keychain).
- `schema.sql` (repo root) — **delete.** Unreferenced and stale (still has
  `scheduled_date`; missing `account`, `settings`, `stream_view`, `intg`).
- `docs/manifest.md` — fix the stack section (Postgres, Tailwind build step, auth)
  and acknowledge accounts/views/Monarch/`intg`.
- `docs/vision.md` — remove "expense tracking — income only" non-goal; the
  product is explicitly **in and out**.

## Validation

- New user: prompted for cash on hand; entering `$X` makes "cash today" == `$X`.
- An Income stream and an Expense stream with the *same* positive amount move the
  balance in opposite directions.
- TMO: 5 loans project 5 movable events/month; when a check syncs, the projection
  collapses to the actual (no duplicate), and a late check moves its event.
- Moving / adding / deleting a single event re-computes the balance correctly.
- Semi-monthly + biweekly streams generate the right occurrences.
- An estimated stream (credit card) shows a flagged estimate that the real
  payment overwrites on reconcile.
- "Cash on [future date]" matches a hand-walked sum of expected flows.
- `tools/check_intg_boundary.sh` still passes (no `intg.*` leakage into `public`
  stream code).

## Open questions

- TMO default day: hard-default to the 20th, or seed from each loan's
  `next_payment_date` / day-of-month?
- Estimated streams: seed a typical amount up front, or leave blank until the
  first actual establishes a baseline?
- Reconciliation match tolerance (how many days around the expected date counts as
  "the same payment")?
