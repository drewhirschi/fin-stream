# Payments page — mobile-first redesign

The payments page (both global `/payments` and integration-scoped `/integrations/tmo/payments`) is one of the few pages the owner actually uses on a phone. Today it's a dense desktop table with redundant Loan/Borrower/Property columns and a cramped date column. Make it readable on a 390px viewport and focus on the fields that matter: *has it been processed* (check number + amount) and *when*.

## Problems

1. **Date column is cramped** — not from the filter (MM-DD-YYYY is already compact) but from table width pressure on mobile.
2. **Loan / Borrower / Property are three columns for one thing.** They all describe the same loan; they should collapse into one column with a truncated primary label and a hover/tap affordance for details.
3. **Too many columns overall** for what the owner cares about on the go. Priority: date, processed indicator (check # + amount), loan identity. De-emphasize: service fee / interest / principal breakdown (still accessible, just not primary on mobile).

## Investigation findings

### Current columns

**Global payments** — `templates/payments.html:22-27`:
Date | Kind | Loan | Check # | Source | Amount | State
- Date renders actual/expected/scheduled with a label (`templates/payments.html:35-44`), uses `|date` filter.
- State badge reflects actual/expected/scheduled (`:65-72`).

**Integration payments** — `templates/integration_payments.html:29-51`:
Date | Loan | Borrower | Property | Check # | Amount | Service Fee | Interest | Principal
- All three Loan/Borrower/Property come from the same `TmoImportPaymentView` row (`src/models/mod.rs:363-383`) — `loan_account`, `borrower_name`, `property_name`. Denormalized in `intg.tmo_import_payment`.

### Filters and formatting
- `src/filters.rs:76-83` — `date` filter is fine. Already in US `MM-DD-YYYY` format.
- `src/filters.rs` also has `money`; currency is handled.

### Processed signal
- Global: `PaymentView.actual_date.is_some()` reliably means "actually executed" (template uses this at `:66-72`).
- Integration: `TmoImportPaymentView.check_number` + `processing_state` (field at `:379` of models).

### Mobile infrastructure
- No responsive table logic today. `<table>` wrapped in `overflow-x-auto` — horizontal scroll on mobile, which is the current bad UX.
- Base layout uses DaisyUI drawer already (`templates/base.html:19-85`).
- No tooltip library. Existing patterns: native `title=` in `base.html` sidebar; forecast.html has an Alpine.js hover overlay at ~line 50. No DaisyUI `tooltip` class in use.

## Fix

### Strategy

Two rendering modes for the payments list, switching at the `md` breakpoint:

- **`>= md`**: keep a table but slimmed down and with a merged "Loan" column (account + borrower + property stacked vertically or shown as primary + muted secondary).
- **`< md`**: render each payment as a card with:
  - Top line: date (left) + amount (right, large).
  - Middle: loan account (mono) + borrower name truncated.
  - Bottom: check # pill (or "not processed" pill) and small secondary fee/interest/principal chips that can be tapped to expand.

Use only what the project already has: DaisyUI + Tailwind + Alpine.js + HTMX. No new libs.

### Fix 1 — merge Loan/Borrower/Property into one column

In `templates/integration_payments.html`:

- Replace the three columns with a single "Loan" column that renders:
  - Primary line: `loan_account` (mono).
  - Secondary line: borrower name truncated to ~24 chars using CSS `truncate` + a `title=` attribute carrying the full borrower and property (cheap hover). For a richer desktop hover, follow the forecast.html Alpine pattern: `@mouseenter`/`@mouseleave` toggling a positioned `x-show` div with borrower + property on separate lines.
  - Drop the standalone Borrower and Property columns.

Apply the same pattern to the global payments page loan column.

### Fix 2 — date formatting

- Keep `|date` (MM-DD-YYYY), but:
  - On mobile cards, render just the date without a "(expected)" / "(scheduled)" parenthetical — instead show a small colored badge next to the amount indicating state.
  - On desktop, keep the state label under the date as today but in a smaller muted line to reclaim vertical density.

### Fix 3 — processed indicator as the hero

- On mobile cards: if `check_number` is present (integration view) or `actual_date.is_some()` (global view), show a green "Processed · #{check_number}" pill; else a muted "Not yet processed" pill. Amount is always shown large.
- On desktop table: move "Check #" next to "Amount"; style an empty check number as a muted dash; color the row subtly when processed.

### Fix 4 — collapse service fee / interest / principal on mobile

- Hide these columns under `md` using `hidden md:table-cell`.
- On mobile cards, render a small "Details" chevron that toggles an expanded region (Alpine `x-data="{ open: false }"`) showing the breakdown fields. Keep the default collapsed so the primary view stays scannable.

### Fix 5 — general mobile polish

- Ensure the outer page wrapper uses the existing drawer pattern from `base.html`; the payments content itself is a `card bg-base-100` with `p-3 sm:p-5` padding.
- Tap targets: all pills/buttons use `btn-sm` not `btn-xs` on mobile (per CLAUDE.md mobile rule).
- Sticky date header: optional, only if the implementation stays light — group payments by date and render a sticky `<h3>` per group on mobile.

## Acceptance

- Open `/integrations/tmo/payments` on a 390px viewport → payments render as cards, one per row, with date + amount + check number all visible without horizontal scroll.
- The same page on a 1440px viewport → table shows Date · Loan (account + truncated borrower with hover popover) · Check # · Amount · fee/interest/principal.
- Hover over a truncated borrower name on desktop → full borrower + property appear.
- A payment without a check number → shows "Not yet processed" pill, not a bare dash in the primary spot.
- The `|date` filter output looks unchanged (MM-DD-YYYY).
- No new npm/cargo dependencies introduced.

## Out of scope

- Redesigning the Payment model or adding a real "cleared_at" field on imports.
- Filters/search on the payments page (not mentioned; leave existing behavior).
- Global payments page deep changes — this plan keeps its column shape but applies the same loan-column merge and mobile card pattern.
