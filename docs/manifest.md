# Income Streams — Product Manifest

## The Problem

I have money coming in from multiple sources on different schedules, and I can't easily answer: **"How much money will I have on a given day?"**

Right now I'm juggling:
- A mortgage servicing portal (The Mortgage Office / Val-Chris Investments) that shows loan payments, but the UX is clunky and doesn't project forward
- Bank statements that show when money actually landed, which is different from when it was "due"
- Email notifications about payments
- My paycheck schedule

There's no unified view. I can't see what's been paid, what's coming, or what my cash position will look like next Tuesday. I especially lose track of timing — a payment "due" on the 1st might not land in my bank until the 20th.

## The Solution

A personal tool that visualizes all income as **streams** on a **horizontal timeline**.

### Streams

A stream is a named income source. Each stream has its own schedule, its own data source, and its own lane on the timeline.

**Stream 1: Trustee Income** (building first)
- I have fractional ownership in 4 mortgage loans serviced by Val-Chris Investments
- Each loan pays monthly — borrowers pay the servicer, the servicer sends me my share
- Data comes from The Mortgage Office API (reverse-engineered from their portal)
- Loans: Lee (Dana Point), Nakagawa (Piedmont), Carvajal (Monrovia), Morales (Gardena)
- My shares range from ~7-9% of each loan
- Monthly payments range from ~$396 to ~$1,063

**Stream 2: Salary** (future)
- Paycheck on the 15th and last day of each month
- Fixed amounts, manually entered

**More streams later** — any recurring income source.

### The Timeline

The core UI is a horizontal scrollable timeline, day by day.

- **Looking left (past):** Shows actual payments that were received, with real dates
- **Looking right (future):** Shows projected payments based on schedules
- **Today** is the anchor point in the middle
- **Each stream** is a horizontal lane/row
- **Hover** on any point in the future shows: "By this date, you will have received $X since today across all streams"
- **Click** a projected event to see details or edit it

### Manual Overrides

This is the key feature that makes this more than just a dashboard.

Payments don't land when they're "due." There's always a lag — sometimes predictable, sometimes not. I want to be able to:

1. See the default projection based on schedules (e.g., "4 payments expected around the 20th")
2. Override individual events: "I know this one is late — expect it on the 26th instead"
3. See the projection update in real time to reflect my overrides

The question I'm always trying to answer: **"On [date], how much money will I have received since today?"**

### Data Sync (The Mortgage Office)

The first integration syncs data from The Mortgage Office portal:
- **Login:** POST to their API with company ID, account number, and PIN
- **Portfolio:** Pull all active loans with balances, rates, payment amounts
- **Payment history:** Pull all past payments with breakdowns (interest, principal, service fees)
- **Loan details:** Property info, LTV, appraised values, maturity dates
- **Overview:** Portfolio-level metrics (total value, yield, YTD interest)

Sync is manual for now (click a button), shows live progress, and keeps a log of all sync runs.

The API base is `https://lvcprod.themortgageoffice.com`. Auth is session-based cookies after login. Account is 3589 under company VCI.

### What I Care About Most

1. **What's been paid** — actual payments received, with dates and amounts
2. **What payments I can expect** — projected from loan schedules, adjustable
3. **When money will land** — not when it's "due" but when it actually hits my bank
4. **Keeping track of these loans** — borrower, property, balance, rate, maturity, delinquency status
5. **Portfolio health** — yield, YTD income, trust balance, which loans are late

### What I Don't Care About (Yet)

- Expenses or outflows
- Bank account integration
- Multi-user or sharing
- Mobile app
- Automated/scheduled syncing (manual is fine for now)

## Technical Decisions

- **Rust** (Axum + Askama + SQLite via sqlx) — matches my web scaffold
- **DaisyUI + Tailwind + HTMX + Alpine.js** — vendored, no build step
- **SQLite** — local personal tool, tiny dataset, will work for decades
- **Generic streams model** — `stream` → `stream_event` → `stream_schedule` so salary and future sources plug in the same way
- **Three-date model** on events: `scheduled_date` (from schedule), `expected_date` (my override), `actual_date` (when it happened)
- **TMO-specific tables** alongside generic ones for loan details and portfolio snapshots
