# Income Streams

A personal tool for projecting cash flow across multiple income sources.

## Problem

Income arrives from multiple sources on different schedules — mortgage loan payments, salary, etc. Each has its own portal or system. It's hard to answer: **"How much money will I have on a given day?"**

Specific pain points:
- Mortgage loan payments are tracked in a clunky servicer portal (The Mortgage Office)
- Payments have a lag between "due date" and when they actually land in the bank
- No unified view across income sources
- No forward-looking projection that accounts for known delays

## Solution

A horizontal timeline that shows income events across all streams — past actuals and future projections — with the ability to manually adjust expected dates.

### Core Concepts

**Stream**: A named income source. Examples:
- "Trustee Income" — monthly payments from 4 mortgage loans serviced by Val-Chris Investments
- "Salary" — bimonthly paycheck on the 15th and last day of month

**Event**: A single payment, either past or projected. Has three key dates:
- `scheduled_date` — when the payment is supposed to happen per the schedule
- `expected_date` — manual override for when it'll actually land (e.g. "this one will be late")
- `actual_date` — when it actually arrived (filled in from sync or manually)

**Schedule**: A recurring pattern that generates projected events (e.g. "$1,062.50 on the 20th of each month until Oct 2026").

### Timeline UX

```
                    ← past ─── TODAY ─── future →

Trustee Income  ──●──●──●──●──|──○──○──○──○──○──
Salary          ──●──●──●──●──|──○──○──○──○──○──
                                    ↑
                              hover: +$4,200 by here
```

- `●` = received (past, actual)
- `○` = projected (future, from schedule)
- Hover on any point shows cumulative income from today to that date
- Click a projected event to adjust its expected date or amount
- Each stream is a horizontal lane

### Manual Overrides

The key insight: scheduled dates and actual landing dates differ. A mortgage payment "due" on the 1st might not arrive as a check until the 20th. The user can:

1. See the default projection based on schedules
2. Override individual events: "this check will be late, expect it on the 26th"
3. The projection updates to reflect the override

### Data Sources

**The Mortgage Office API** (first integration):
- Auto-sync loan portfolio, payment history, and loan details
- API base: `https://lvcprod.themortgageoffice.com`
- Auth: session-based login with companyId + account + PIN
- Endpoints for overview, portfolio, history, loan detail

**Manual entry** (for salary and other streams):
- Define a schedule (amount, frequency, day of month)
- Events auto-generated from the schedule
- Mark events as received when they land

## Non-Goals (for now)

- Expense tracking (this is income only)
- Bank account integration (manual or API-specific sync only)
- Multi-user / sharing
- Mobile app (web-first)
