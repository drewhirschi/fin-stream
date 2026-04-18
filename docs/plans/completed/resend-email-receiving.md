# Receive Emails via Resend Webhooks

## Status (2026-04-16)

**Code: complete.** Landed in commit `10d62e0`. All files in the Files Modified / Files Created tables exist, migrations present, webhook handler wired into the public router, inbox templates rendering, Svix signature verification implemented at `src/routes/webhooks.rs:51-56`.

**Tunnel: complete.** Public endpoint `https://finstream-webhooks.hirschi.dev/webhooks/resend` reachable via Cloudflare Tunnel (see `coolify-network-hardening.md`). Verified: POST to that URL reaches the app (returns 400 on empty body — route exists). Non-webhook paths 404 at the tunnel edge.

**Remaining before real emails flow end-to-end:**
- [ ] Set `RESEND_WEBHOOK_SECRET` in Coolify env — **critical**. Signature verification is silently disabled when this env is unset (handler skips the check). Currently anyone who discovers the URL can POST fake events.
- [ ] Set `RESEND_API_KEY` in Coolify env — without it, the background fetch task can't pull email body/attachments from Resend, so rows land in `error` state.
- [ ] Add the webhook endpoint in the Resend dashboard and copy the signing secret.
- [ ] Send a test webhook from Resend → verify a row lands in `intg.received_email` with `processing_state = 'stored'` and body/attachments in S3.
- [ ] Navigate to `/inbox` in the app (once auth works) and confirm the email shows up linkable to a loan.

## Context

The app stores cron expressions for scheduled syncs and has a loan workspace, but has zero email infrastructure. The user wants to receive inbound emails via Resend webhooks, store them (metadata in Postgres, body + attachments in S3), view them in an "Inbox" page, and link them to specific loans. Emails start unlinked ("staging") until the user assigns them.

The user does not yet have a Resend account, so the API key will be optional — the app should start fine without it.

## Scope (first milestone)

1. Webhook endpoint to receive Resend `email.received` events
2. Background fetch of email body + attachments from Resend API, stored in S3
3. DB tables for email metadata and attachment metadata
4. Inbox page: list unlinked emails, view email detail, link/unlink to loans
5. Email media serving route (body HTML and attachment downloads)

---

## Implementation Steps

### 1. Config — `src/config.rs`

Add two accessors (both optional so the app starts without Resend):

```rust
pub fn resend_api_key() -> Option<String>      // RESEND_API_KEY
pub fn resend_webhook_secret() -> Option<String> // RESEND_WEBHOOK_SECRET
```

### 2. Dependencies — `Cargo.toml`

Add `hmac = "0.12"` for SVIX webhook signature verification (pairs with existing `sha2 = "0.10"`).

### 3. DB Schema — `src/db/mod.rs` (add to `run_migrations()`)

**`intg.received_email`** — one row per inbound email:
- `id BIGSERIAL PK`, `resend_email_id TEXT UNIQUE`, `from_address`, `to_addresses` (JSON array), `subject`, `received_at`
- `body_s3_key TEXT` (null until fetched), `body_content_type TEXT`
- `loan_account TEXT` (null = unlinked/staging)
- `processing_state TEXT` (pending → stored → error)
- `error_message`, `raw_webhook_payload`, `created_at`, `updated_at`

**`intg.received_email_attachment`** — one row per attachment:
- `id BIGSERIAL PK`, `email_id BIGINT FK → received_email ON DELETE CASCADE`
- `resend_attachment_id TEXT`, `filename`, `content_type`, `size_bytes`
- `s3_key TEXT` (null until fetched), `processing_state TEXT`
- `UNIQUE(email_id, resend_attachment_id)`

**Indexes:** unlinked emails by date, emails by loan_account, attachments by email_id.

### 4. Models — `src/models/mod.rs`

Add `ReceivedEmailView` and `ReceivedEmailAttachmentView` structs with `sqlx::FromRow`.

### 5. DB Queries — new `src/db/emails.rs`

Register in `src/db/mod.rs` as `pub mod emails;`.

Key functions:
- `insert_received_email(...)` — INSERT ON CONFLICT DO NOTHING (idempotent for webhook retries)
- `insert_attachment_row(...)`
- `mark_email_body_stored(...)`, `mark_attachment_stored(...)`, `mark_email_error(...)`
- `list_unlinked_emails(pool)` — WHERE loan_account IS NULL
- `list_emails_for_loan(pool, loan_account)`
- `get_email_by_id(pool, id)`
- `list_attachments_for_email(pool, email_id)`
- `link_email_to_loan(pool, email_id, loan_account)`
- `unlink_email(pool, email_id)`

### 6. Resend API Client — new `src/resend.rs`

Register in `src/lib.rs`. Uses reqwest (already a dep) with `Authorization: Bearer {key}`.

Two methods:
- `get_received_email(email_id)` → returns body (html/text), metadata
- `get_attachment(email_id, attachment_id)` → returns raw bytes

### 7. Webhook Route — new `src/routes/webhooks.rs`

Register in `src/routes/mod.rs` and merge in `src/main.rs`.

**`POST /webhooks/resend`:**
1. Optionally validate SVIX signature (if `RESEND_WEBHOOK_SECRET` is set) using hmac + sha2
2. Parse JSON body; ignore events where type != `email.received`
3. Insert email + attachment rows into DB (ON CONFLICT for idempotency)
4. `tokio::spawn` background task to fetch body + attachments from Resend API and store in S3
5. Return 200 immediately

**Background task (`fetch_and_store_email`):**
- Calls `ResendClient::get_received_email()` for the body
- Stores body in S3 at key `emails/{resend_email_id}/body.html` via `MediaStorage::store()`
- For each attachment: fetches bytes, stores at `emails/{resend_email_id}/attachments/{filename}`
- Updates DB rows with s3_keys and processing_state

### 8. Email Media Route — `src/routes/media.rs`

Add `/media/emails/{*key}` route, reusing the same `MediaStorage::get()` pattern as the existing `/media/loan-workspace/{*key}` handler. Extract shared helper to avoid duplication.

### 9. Inbox Page — `src/routes/pages.rs`

New routes:
- `GET /inbox` — list unlinked emails with link-to-loan dropdown per row
- `GET /inbox/{email_id}` — email detail: metadata, body iframe, attachment list
- `POST /inbox/{email_id}/link` — link email to a loan (form with loan_account select)
- `POST /inbox/{email_id}/unlink` — unlink email back to staging

### 10. Templates

**`src/templates/mod.rs`** — add `InboxTemplate` and `InboxEmailDetailTemplate`, register in `impl_into_response!` macro.

**`templates/inbox.html`** — extends `base.html`:
- Table of unlinked emails (date, from, subject, attachment count, status)
- Each row has a loan-select dropdown + link button (HTMX form)
- Rows link to `/inbox/{id}` for detail

**`templates/inbox_email_detail.html`** — extends `base.html`:
- Email metadata card
- Body rendered in iframe pointing to `/media/emails/{resend_email_id}/body.html`
- Attachment list with download links
- Link/unlink form

### 11. Navigation — `templates/base.html`

Add "Inbox" nav item with envelope icon between Integrations and Timeline. Use `{% block nav_inbox %}` pattern.

---

## Files Modified

| File | Change |
|---|---|
| `Cargo.toml` | Add `hmac = "0.12"` |
| `src/config.rs` | Add `resend_api_key()`, `resend_webhook_secret()` |
| `src/lib.rs` | Add `pub mod resend;` |
| `src/db/mod.rs` | Add `pub mod emails;`, migration SQL for 2 tables + indexes |
| `src/models/mod.rs` | Add `ReceivedEmailView`, `ReceivedEmailAttachmentView` |
| `src/routes/mod.rs` | Add `pub mod webhooks;` |
| `src/routes/media.rs` | Add `/media/emails/{*key}` route |
| `src/routes/pages.rs` | Add inbox handlers (list, detail, link, unlink) |
| `src/templates/mod.rs` | Add template structs, register in macro |
| `src/main.rs` | Merge `routes::webhooks::router()` |
| `templates/base.html` | Add Inbox nav item |

## Files Created

| File | Purpose |
|---|---|
| `src/db/emails.rs` | DB queries for received emails |
| `src/resend.rs` | Resend API client (get email body, get attachment) |
| `src/routes/webhooks.rs` | Webhook POST handler + background fetch task |
| `templates/inbox.html` | Inbox list page |
| `templates/inbox_email_detail.html` | Email detail page |

## Key Design Decisions

- **No resend-rs crate** — reqwest is already a dep; Resend API is just 2 GET endpoints
- **`loan_account TEXT` nullable** — null = unlinked staging; set = linked. Simple, queryable, matches existing loan identifier pattern
- **ON CONFLICT DO NOTHING** — Resend may retry webhooks; idempotent insert prevents duplicates
- **tokio::spawn for fetch** — webhook must return 200 fast; matches the existing sync pattern in `src/routes/sync.rs:120`
- **Reuse MediaStorage** — same S3 bucket, key prefix scoped under `emails/`
- **Optional config** — app starts without RESEND_API_KEY; webhook returns error if key missing

## Verification

1. `cargo build` — compiles clean
2. `cargo run` — app starts without RESEND_API_KEY set (no panic)
3. Send test POST to `/webhooks/resend` with sample payload — inserts DB row, spawns fetch (will fail without real API key, but row appears with error state)
4. Navigate to `/inbox` — page renders, shows test email
5. Once Resend is configured: send real email, verify body + attachments appear in S3 and inbox detail page
