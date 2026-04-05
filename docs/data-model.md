# Data Model

SQLite database. All dates stored as ISO-8601 TEXT (`YYYY-MM-DD` or `YYYY-MM-DDTHH:MM:SS`). All monetary amounts stored as REAL.

## Entity Relationship

```
stream 1──* stream_schedule    (a stream has recurring payment patterns)
stream 1──* stream_event       (a stream has individual payment events)
stream 1──* tmo_loan           (mortgage stream links to TMO loan details)

stream_schedule ···> stream_event  (schedules generate projected events)
tmo_loan ···> stream_event         (TMO sync creates/updates events)
```

## Tables

### `stream`

A named income source.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | |
| name | TEXT NOT NULL | "Trustee Income", "Salary" |
| type | TEXT NOT NULL | "mortgage_portfolio", "salary", "manual" |
| description | TEXT | |
| is_active | INTEGER | DEFAULT 1 |
| created_at | TEXT | ISO-8601 |
| updated_at | TEXT | ISO-8601 |

### `stream_event`

A single payment — past or projected. The central table for the timeline.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK AUTO | |
| stream_id | INTEGER FK→stream | |
| label | TEXT | "Lee - 14 Ritz Cove Dr", "Paycheck" |
| scheduled_date | TEXT NOT NULL | When it's supposed to happen per the schedule |
| expected_date | TEXT | Manual override — when it'll actually land. NULL = use scheduled_date |
| actual_date | TEXT | When it actually happened. NULL = hasn't yet |
| amount | REAL NOT NULL | |
| status | TEXT | 'projected', 'confirmed', 'received', 'late', 'missed' |
| source_id | TEXT | External ref (check number, etc.) |
| source_type | TEXT | "tmo_history", "manual", "schedule" |
| metadata | TEXT | JSON — type-specific data (interest/principal breakdown) |
| notes | TEXT | User notes |
| created_at | TEXT | |
| updated_at | TEXT | |

**Unique constraint**: `(stream_id, source_type, source_id)` — prevents duplicate synced events.

**Effective date logic**: The date to use for timeline placement is:
```
COALESCE(actual_date, expected_date, scheduled_date)
```

### `stream_schedule`

Recurring pattern that generates projected events.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | |
| stream_id | INTEGER FK→stream | |
| label | TEXT | Identifies what this schedule is for |
| amount | REAL NOT NULL | |
| frequency | TEXT NOT NULL | 'monthly', 'bimonthly', 'weekly' |
| day_of_month | INTEGER | For monthly. -1 = last day of month |
| start_date | TEXT NOT NULL | |
| end_date | TEXT | NULL = indefinite |
| is_active | INTEGER | DEFAULT 1 |
| metadata | TEXT | JSON — extra info (loan account, etc.) |
| created_at | TEXT | |
| updated_at | TEXT | |

### `tmo_loan`

Loan details synced from The Mortgage Office API. Linked to the trustee income stream.

| Column | Type | Notes |
|--------|------|-------|
| loan_account | TEXT PK | Natural key from API (e.g. "20805") |
| stream_id | INTEGER FK→stream | Links to the trustee income stream |
| borrower_name | TEXT | |
| property_address | TEXT | |
| property_city | TEXT | |
| property_state | TEXT | |
| property_zip | TEXT | |
| property_type | TEXT | "SFR", "Land", etc. |
| property_priority | INTEGER | Lien position |
| occupancy | TEXT | |
| appraised_value | REAL | |
| ltv | REAL | |
| percent_owned | REAL | |
| loan_type | INTEGER | |
| note_rate | REAL | Annual interest rate |
| original_balance | REAL | |
| principal_balance | REAL | |
| regular_payment | REAL | Investor's share |
| payment_frequency | TEXT | "Monthly" |
| maturity_date | TEXT | |
| next_payment_date | TEXT | |
| interest_paid_to | TEXT | |
| term_left_months | INTEGER | |
| is_delinquent | INTEGER | |
| is_active | INTEGER | DEFAULT 1 |
| last_synced_at | TEXT | |
| detail_synced_at | TEXT | |

### `tmo_account`

Singleton — auth info for The Mortgage Office API. PIN stored in macOS Keychain.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK | CHECK(id=1) |
| company_id | TEXT | "vci" |
| account_number | TEXT | "3589" |
| source_rec_id | TEXT | From login response |
| display_name | TEXT | |
| email | TEXT | |
| last_login_at | TEXT | |

### `portfolio_snapshot`

Daily point-in-time capture of TMO portfolio metrics for trend tracking.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK AUTO | |
| snapshot_date | TEXT UNIQUE | One per day |
| portfolio_value | REAL | |
| portfolio_yield | REAL | |
| portfolio_count | INTEGER | |
| ytd_interest | REAL | |
| ytd_principal | REAL | |
| trust_balance | REAL | |
| outstanding_checks | REAL | |
| service_fees | REAL | |
| synced_at | TEXT | |

### `sync_log`

Audit trail of sync operations.

| Column | Type | Notes |
|--------|------|-------|
| id | INTEGER PK AUTO | |
| started_at | TEXT | |
| finished_at | TEXT | |
| status | TEXT | 'running', 'success', 'error' |
| error_message | TEXT | |
| endpoints_hit | TEXT | Comma-separated list |
| events_upserted | INTEGER | |
| loans_upserted | INTEGER | |
| snapshots_created | INTEGER | |

## Key Indexes

```sql
-- Timeline queries
stream_event(stream_id, scheduled_date)
stream_event(expected_date)        -- "what lands on this day"
stream_event(actual_date)          -- past payment lookups
stream_event(status)

-- Schedules
stream_schedule(stream_id, is_active)

-- TMO
tmo_loan(stream_id)

-- Snapshots
portfolio_snapshot(snapshot_date DESC)

-- Sync
sync_log(started_at DESC)
```

## TMO API Reference

Base URL: `https://lvcprod.themortgageoffice.com`

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/api/login` | POST | `{companyId, account, pin}` → session + user info |
| `/api/overview?showPaidOffLoans=false` | GET | Portfolio aggregates |
| `/api/portfolio/getPortfolioData?request={json}` | GET | Paginated loan list |
| `/api/portfolio/getPortfolioYield` | GET | Single yield number |
| `/api/history?request={json}` | GET | Payment history (filterable by loan, date range) |
| `/api/history/getLoanAccounts` | GET | List of loan account numbers |
| `/api/loanDetail/getLoanDetail/{id}` | GET | Full loan detail |
| `/api/loanDetail/getLoanConversations/{id}` | GET | Loan messages |
| `/api/companyInfo/getCompanyInfo?companyId=vci` | GET | Servicer contact info |
| `/api/messages/getUnreadMessageCount` | GET | Unread count |

All responses use envelope: `{ data, success, errorType, error, errorStackTrace }`

Auth is session-based (cookies after login). Complex GET queries pass URL-encoded JSON in `request` query param.
