# Income Streams — Product Manifest

## The Problem

I have money coming in from multiple sources on different schedules, and I can't easily answer: **"How much money will I have on a given day?"**

Right now I'm juggling:
- A mortgage servicing portal (The Mortgage Office / Val-Chris Investments) that shows loan payments, but the UX is clunky and doesn't project forward
- Bank statements that show when money actually landed, which is different from when it was "due"
- Email notifications about payments
- My paycheck schedule

There's no unified view. I can't see what's been paid, what's coming, or what my cash position will look like next Tuesday. I especially lose track of timing — a payment "due" on the 1st might not land in my bank until the 20th.

## The Core Question

**"On [date], how much money will I have?"**

Everything in this tool exists to answer that question.

## The Solution

A personal tool that models all cash flow as **streams** and projects my **cash position** forward on a **scrollable timeline**.

---

## Streams

A stream is a named source (or sink) of money. Each stream has its own schedule, its own data source, and its own lane on the timeline.

### Trustee Income (built, syncing)
- I have fractional ownership in 4 mortgage loans serviced by Val-Chris Investments
- Each loan pays monthly — borrowers pay the servicer, the servicer sends me my share
- Data syncs from The Mortgage Office API (reverse-engineered from their portal at lenders.themortgageoffice.com)
- Current loans: Lee (Dana Point), Nakagawa (Piedmont), Carvajal (Monrovia), Morales (Gardena)
- My shares range from ~7-9% of each loan
- Monthly payments range from ~$396 to ~$1,063
- Projected payments subtract estimated average service fee for accuracy

### Expenses (built, manual entry)
- Manual outflows — bills, rent, big purchases
- Added via the forecast page
- Appear as red dots/negative amounts on the timeline

### Salary (future)
- Paycheck on the 15th and last day of each month
- Fixed amounts, manually entered schedule

### More streams later
- Any recurring income or expense source can plug into the same model

---

## The Forecast

The forecast page is the centerpiece. It answers the core question by projecting a running cash balance.

### How it works
1. **Set a starting cash balance** — "I have $X right now"
2. **Events accumulate** — inflows (trustee payments) add, outflows (expenses) subtract
3. **Running balance** updates day by day into the future
4. **Scrub the timeline** to see your cash position at any date

### The Timeline Scrubber
- Horizontal range: 90 days in the past to 180 days into the future
- Today is the anchor point
- Drag or use keyboard (arrows, Shift+arrows, Home) to scrub
- **Hero display** at the top shows the cash position at the scrubbed date
- Color-coded: green when positive, red when negative

### Stream Lanes
- Each stream gets a horizontal lane with event dots
- Green dots = inflows, red dots = outflows
- Delinquent loans get a warning marker
- Month markers for orientation

### Events Table
Below the timeline, a table shows every event in the forecast window:
- Date, source/label, amount, running balance, status
- Received payments, projected payments, and manual expenses all intermixed chronologically

### The Answer Badge
In the sidebar nav, a persistent badge shows: **"Cash on [date]: $X"**
- Date = the next big expense (over a configurable threshold)
- Gives an at-a-glance answer to "am I going to be okay?"
- Links to the forecast page for detail

---

## Manual Overrides

Payments don't land when they're "due." There's always a lag — sometimes predictable, sometimes not.

- Every projected event has a `scheduled_date` (from the schedule), an `expected_date` (my override), and an `actual_date` (when it really happened)
- On the forecast page, I can click a projected event and say "this one won't land until the 26th"
- The forecast updates immediately to reflect the override
- Received (past) events cannot be overridden — they're facts

---

## Data Sync (The Mortgage Office)

The TMO integration pulls all loan and payment data:

**What syncs:**
- **Overview** — portfolio value, yield, YTD interest/principal, trust balance
- **Portfolio** — all active loans with balances, rates, payment amounts, delinquency
- **Loan details** — property info, LTV, appraised values, maturity dates, original balances
- **Payment history** — every check received with breakdown (interest, principal, service fees, charges)

**How it works:**
- Manual trigger: click "Run Sync" on the sync page
- Logs in to TMO API with company ID (VCI), account (3589), and PIN
- Runs in the background, HTMX polls for live progress
- Deduplicates payments via unique constraint on (stream_id, source_type, source_id)
- Cleans up stale projected events before generating new ones
- Estimates average service fee from historical payments for projection accuracy
- Generates projected future events for 6 months out based on loan schedules
- Logs every sync run (started, finished, status, counts, errors)

**API details:**
- Base URL: `https://lvcprod.themortgageoffice.com`
- Auth: session-based cookies after POST `/api/login`
- Response envelope: `{ data, success, errorType, error, errorStackTrace }`

---

## What I Care About

1. **Cash position projection** — the forecast is the whole point
2. **What's been paid** — actual payments received, with dates and amounts
3. **What payments I can expect** — projected from loan schedules, adjustable
4. **When money will actually land** — not when it's "due" but when it hits my bank
5. **Keeping track of these loans** — borrower, property, balance, rate, maturity, delinquency
6. **Portfolio health** — yield, YTD income, trust balance, which loans are late
7. **Adding expenses** — so the forecast reflects outflows too

---

## What's Built

### Pages
- **Dashboard** (`/`) — portfolio stats, active loans table, recent and upcoming payments
- **Loans** (`/loans`) — card view of each loan with full details
- **Payments** (`/payments`) — chronological payment history with status badges
- **Forecast** (`/forecast`) — timeline scrubber, stream lanes, events table, expense entry, date overrides
- **Sync** (`/sync`) — trigger sync, live progress, sync history log

### API
- `GET /api/forecast` — computes cash forecast with running balance for a date range
- `POST /api/events` — create manual expense/event entries
- `PATCH /api/events/:id` — override expected date on projected events
- `POST /api/settings/cash` — set starting cash balance

### Data Model
- `stream` — named income/expense source
- `stream_event` — individual cash movements (past and projected), three-date model
- `stream_schedule` — recurring patterns
- `tmo_loan` — TMO loan details
- `tmo_account` — TMO auth info (PIN in env, not DB)
- `portfolio_snapshot` — daily portfolio metrics for trends
- `sync_log` — sync audit trail
- `settings` — key-value store (starting cash balance, etc.)

---

## What's Not Built Yet

- **Salary stream** — manually defined schedule, auto-generates projected events
- **Horizontal timeline visualization** — the full scrollable day-by-day lanes UI (currently the scrubber + table, not the visual stream lanes with dots that you can scroll through)
- **Bank account integration** — automatic actuals from bank feeds
- **Automated/scheduled syncing** — currently manual button click
- **Mobile** — web-first, no responsive optimization yet
- **Multi-user** — personal single-user tool

## Next Feature: Loan Workspace

The next meaningful expansion is to turn each loan into a richer workspace, not just a synced record.

That means:

- Redfin and Zillow links
- bucket-backed image and document storage
- linked email threads
- notes and underwriting context

See [docs/loan-workspace-manifest.md](/Users/drew/w/trust-deeds/docs/loan-workspace-manifest.md) for the dedicated feature manifest.

---

## Technical Stack

- **Rust** (edition 2024) — Axum 0.8, Tokio, sqlx (async SQLite)
- **Askama** — type-safe HTML templates
- **DaisyUI 5 + Tailwind CSS 4** (browser JIT) — vendored, no build step
- **HTMX 2 + Alpine.js 3** — interactive UI without a JS framework
- **SQLite** — local personal tool, tiny dataset
- **Precision Vault design** — dark theme, Manrope/Inter/Roboto Mono fonts, emerald/crimson palette
