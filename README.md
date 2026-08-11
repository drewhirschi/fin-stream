# Trust Deeds

The main application is a NextRS 0.4.2 app using idiomatic React `page.tsx` and `layout.tsx`
conventions with a persistent application shell, shadcn-style components, and
Rust `route.rs` JSON/action boundaries. It has revocable libSQL authentication plus the
manual Streams/Forecast vertical slice: accounts, an explicit dated cash
anchor, streams, multiple recurrence schedules, views, durable event
overrides/deletions, reconciliation, and deterministic timeline forecasts.
The local server and Vercel function construct the same generated route graph
and middleware stack.

## Run locally and sign in

```sh
cp .env.example .env
# Change ADMIN_PASSWORD in .env before using it.
npm ci --prefix client --no-audit --no-fund
# Generates OpenAPI, the typed client, and CSS, then typechecks the client.
npm run --prefix client build
cargo install cargo-nextrs-dev # once per machine
cargo dev
```

Open <http://localhost:3003>. An anonymous request redirects to `/login`. Sign
in with the `ADMIN_EMAIL` and `ADMIN_PASSWORD` from `.env`.
`cargo dev` rebuilds and restarts the server when application files change,
then triggers a full-page browser reload. Restart it after editing `.env`.
The default local server uses the Turso database selected by
`TURSO_DATABASE_URL` and `TURSO_AUTH_TOKEN`, so local mutations affect that
database.

For isolated SQLite development, run the server and bootstrap command with
`--no-default-features --features local-db`. The explicit bootstrap command creates the
local database, applies the
checksum-verified migrations, creates the admin only when its normalized email
is absent, idempotently creates the initial account/default view/streams, and
enables browser/manual writes with scheduled work still off.
Starting the server does not create users or defaults and fails closed when no
active login exists. Later bootstrap runs do not change an existing password.
Delete the local database only when you explicitly want to reset local data.

```sh
cargo test --locked --no-default-features --features local-db
cargo clippy --locked --no-default-features --features local-db --all-targets -- -D warnings
npm ci --prefix client --no-audit --no-fund
npm test --prefix client
npm run --prefix client build
cargo build --release --locked --no-default-features --features local-db --bin trust-deeds

# Inventory the upgraded PostgreSQL source. This is a read-only preflight;
# final export/traffic cutover still waits for the complete parity rehearsal.
cd tools/pg-to-turso
read -r -s -p 'Source PostgreSQL URL: ' SOURCE_DATABASE_URL; printf '\n'
export SOURCE_DATABASE_URL
cargo run --locked -- inventory \
  --manifest /absolute/path/on-an-encrypted-volume/inventory.json
unset SOURCE_DATABASE_URL
```

The source-host, relational export, object-byte backfill, freeze, validation,
and rollback sequence is in [`CUTOVER.md`](CUTOVER.md). The old Coolify app is
stopped for the final snapshot; there is no dual-write bridge.

## Operational write gate

A fresh database and a production import start in `read_only` mode with the
scheduler disabled. Every non-session `POST`, `PUT`, `PATCH`, and `DELETE`
passes through the shared database-backed gate before its handler runs. Use the
operator CLI for cutover transitions:

```sh
# Local database selected by LIBSQL_LOCAL_PATH:
cargo run --locked --no-default-features --features local-db --bin trust-deeds-ops -- status
cargo run --locked --no-default-features --features local-db --bin trust-deeds-ops -- enable-writes
cargo run --locked --no-default-features --features local-db --bin trust-deeds-ops -- scheduler-on
cargo run --locked --no-default-features --features local-db --bin trust-deeds-ops -- scheduler-off
cargo run --locked --no-default-features --features local-db --bin trust-deeds-ops -- read-only

# Turso selected by TURSO_DATABASE_URL and TURSO_AUTH_TOKEN:
cargo run --locked --no-default-features --features remote-db \
  --bin trust-deeds-ops -- status
```

`enable-writes` deliberately leaves the scheduler off. `scheduler-on` fails
while the database is read-only, and `read-only` atomically turns the scheduler
off. The CLI emits only the control record as JSON and never prints database
URLs or authentication tokens. Remote operation verifies the exact migration
ledger and never applies migrations.

## Vercel and Turso

Use the repository root as the Vercel project Root Directory. Configure these project
environment variables:

- `TURSO_DATABASE_URL`
- `TURSO_AUTH_TOKEN`
- `APP_ENCRYPTION_KEY` (the unchanged key used to encrypt the PostgreSQL provider credentials)
- `CRON_SECRET` (a separate long random bearer secret)
- `S3_ENDPOINT` (the HTTPS S3/R2 API endpoint, with no path)
- `S3_REGION` (`auto` for R2)
- `S3_BUCKET`
- `S3_ACCESS_KEY`
- `S3_SECRET_KEY`
- `S3_KEY_PREFIX` (must match the object-cutover destination prefix)
- `RESEND_WEBHOOK_SECRET` (the Svix-format signing secret for the Resend webhook)
- `RESEND_API_KEY` (used only for fetching received bodies and attachments)
- `SESSION_COOKIE_SECURE=true`

Production users and password hashes are imported from PostgreSQL; old login
sessions are intentionally discarded. Do not put `ADMIN_PASSWORD` in Vercel.

`vercel.json` installs the React client, regenerates the typed API client and
Tailwind/shadcn CSS, typechecks every route component, then builds
only the remote libSQL client. Normal Cargo commands and `cargo dev` use the
remote libSQL client while serving static assets from disk. The Vercel build
also selects `remote-db` explicitly without the `local-server` feature, so
neither the embedded SQLite core nor local static-file services are linked into the
serverless function:

```sh
cargo build --release --locked --no-default-features --features remote-db --bin index
```

Vercel cold starts never migrate or bootstrap. They verify the exact ordered
`_schema_migrations` versions and BLAKE3 checksums before serving requests.
Production startup also rejects an explicitly insecure session cookie. Cookies are
host-only, HttpOnly, SameSite=Strict, scoped to `/`, and expire after seven
days. Browser mutations are restricted to the request's own origin.

## Private media and direct photo uploads

Vercel never stores authoritative media on its local filesystem and the Rust
function never proxies photo bytes. Authenticated `/media/loan-workspace/**`
and `/media/emails/**` requests first prove that the canonical key is referenced
by libSQL, then return a 60-second signed redirect. Both legacy photo URL forms
(`/media/loan-workspace/**` and `/static/loan-images/**`) map to the canonical
`loan-workspace/**` object namespace without rewriting imported rows.

Manual photo upload is a three-step flow: the authenticated browser requests a
five-minute intent, uploads the file directly to S3/R2 with the signed content
type, length, and random metadata marker, then calls finalize. Finalize performs
a server-side HEAD and commits the photo row only when all three values match.
The function request therefore remains small even for the 25 MB application
limit. A failed final DB commit can leave an unreferenced random-key object, but
never a row pointing at missing bytes; retry the same unexpired intent and use
the object manifest/lifecycle policy to reconcile abandoned uploads.

Keep the bucket private and configure CORS for every deployed app origin. The
provider-equivalent policy must allow `PUT` with `Content-Type`,
`Content-Length`, and `x-amz-meta-trust-deeds-upload`; for example:

```json
[
  {
    "AllowedOrigins": ["https://trust-deeds.example"],
    "AllowedMethods": ["PUT"],
    "AllowedHeaders": [
      "Content-Type",
      "Content-Length",
      "x-amz-meta-trust-deeds-upload"
    ],
    "ExposeHeaders": ["ETag"],
    "MaxAgeSeconds": 300
  }
]
```

Remote builds require the complete S3 configuration and HTTPS; partial config
fails startup. Local development with no S3 variables leaves media disabled
instead of silently writing to ephemeral disk. A local MinIO-style emulator is
explicitly opt-in with `S3_ALLOW_HTTP=true` and the other S3 variables set.
Removing a photo transactionally removes only its database reference. The
private object remains through the rollback window and is reclaimed later by a
versioned lifecycle/object-manifest reconciliation, so the UI does not pretend
that S3 and libSQL share an atomic delete.

## Resend inbound email

Configure the [Resend `email.received` webhook](https://resend.com/docs/webhooks/emails/received)
to send `POST` requests to `/webhooks/resend`. `RESEND_WEBHOOK_SECRET` and `RESEND_API_KEY` must be set
together; remote production startup fails closed when either is missing. The
webhook verifies the exact raw body against all supplied `v1` Svix signatures
and rejects timestamps outside a five-minute window before sessions or the
database-backed write gate can run. A valid request still requires the central
write gate to be enabled.

Received metadata is deduplicated by Resend email ID. A short libSQL lease
allows only one invocation to fetch content at a time, fences a stale owner,
and makes provider redelivery safe. Body and attachment work finishes inline
within the request—there is no detached task or process-local queue. Content is
written to deterministic `emails/**` object keys with conditional create,
SHA-256 metadata, and a confirming HEAD, so a replay accepts identical bytes
but never overwrites a mismatch. Transient failures return a non-2xx response
with `Retry-After`; stored, previously stored, and irrelevant events return
2xx.

The application caps one inbound processing attempt at 45 seconds. Before
switching the production webhook, prove a representative maximum-size message
finishes below the deployed Vercel function's hard duration with network and
shutdown margin; do not infer that duration from local execution. Attachment
downloads also fail closed unless Resend returns an HTTPS/default-port URL on
`inbound-cdn.resend.com`, matching the current
[received-attachment API contract](https://resend.com/docs/api-reference/emails/list-received-email-attachments),
so the rehearsal must prove the live provider URL contract.

Authenticated users can retry an errored message with
`POST /inbox/{email_id}/retry`. Stored bodies and attachments are opened through
the authenticated `/media/emails/**` route, which verifies the libSQL reference
before issuing a short-lived signed redirect. The legacy delete action remains
intentionally unrouted until object lifecycle reconciliation owns physical
cleanup.

## Scheduled TMO synchronization

`GET /internal/cron` replaces the legacy process-local forever loop. Invoke it
at least hourly via Vercel Cron or another trusted scheduler and send
`Authorization: Bearer $CRON_SECRET`. Browser sessions do not authorize this
endpoint. A remote production deployment fails startup when `CRON_SECRET` is
absent, and the durable operator control must have both writes and the scheduler
enabled before an authenticated invocation can run.

The handler computes only the latest deterministic TMO slot: hourly, every six
hours, every twelve hours, or daily at 06:00 UTC. It records the next slot for
display, claims the current slot in `sync_log`, and performs at most one TMO
sync inline before returning. Duplicate delivery is a durable no-op. A failed
scheduled slot is not retried automatically; it waits for the next slot or an
explicit manual sync. The repository does not add an unverified Vercel cron
configuration—deployment setup must prove the bearer header and invocation
timing before enabling the scheduler.

Do not enable production cron until a representative full TMO sync fits below
the deployed function's hard duration with clock/network margin. The current
stale-run cutoff is twenty minutes and must remain strictly greater than that
configured platform limit plus skew.

The default TMO cadence is daily. Authenticated pages also make a small,
same-origin freshness request using the browser's local calendar date. If TMO
or Monarch has not completed successfully in the prior hour, that request runs
the existing durable integration action inline. Concurrent browser requests
collapse through the same database-backed execution claims; there is no
process-local timer or fire-and-forget work for Vercel to lose. The browser
checks at most once every five minutes per tab, while the database's
`last_synced_at` remains the authoritative one-hour freshness decision.

The database-backed Inbox list, detail, HTMX panel, loan link/unlink, inbound
delivery, private body/attachment access, and retry actions are available. The
legacy delete action is intentionally not routed yet: deleting libSQL metadata
before object cleanup is coordinated would leave body and attachment objects
dangling.
