# PostgreSQL/Coolify to Turso/Vercel cutover

This is the operator runbook for moving Trust Deeds. It deliberately uses a
short maintenance window instead of dual writes.

## Topology and invariants

- Source: the Coolify application and PostgreSQL service on `gory`, reachable
  only over Tailscale. Repository documentation records PostgreSQL on
  `gory:5433`; verify the live container and mount rather than assuming it.
- Target: one newly imported Turso database, the NextRS Vercel application,
  and an S3-compatible object prefix for photos and inbound-email bytes.
- Run the relational exporter from this workstation over Tailscale unless a
  rehearsal shows that source size or latency requires running it beside
  PostgreSQL. Never expose PostgreSQL publicly.
- Keep `APP_ENCRYPTION_KEY` unchanged. The exporter proves that every stored
  provider credential decrypts before and after conversion.
- PostgreSQL rows and object bytes are different cutover artifacts. A valid
  database manifest does not prove that a referenced photo or attachment
  exists.
- The imported target starts with no sessions, `operation_control=read_only`,
  and its scheduler disabled. Authentication may create disposable target
  sessions during the read-only smoke test.
- The rollback boundary is the first target business/provider write or
  external side effect. Before that point, route traffic back to the old app.
  After it, PostgreSQL is stale and rollback requires replay or fix-forward.

Use a private, encrypted directory outside the repository for every artifact.
Both cutover tools require absolute output paths, mode `0600`, and refuse to
overwrite files.

## Rehearsal while the old app is live

1. Bring `gory` online and inspect, read-only, the running app/database
   containers, PostgreSQL size/schema, app and database mounts, and configured
   environment-variable **names**. Do not print secret values into a terminal
   log.
2. Identify the real object backend. A complete `S3_*` configuration means the
   retained bucket/prefix can be inventoried in place. Otherwise locate the
   Coolify volume that contains `static/loan-images` and any email objects.
3. Copy the current PostgreSQL URL, a known active login, and the unchanged
   encryption key into hidden shell prompts. Run the exporter inventory:

   ```sh
   cd tools/pg-to-turso
   read -r -s -p 'Source PostgreSQL URL: ' SOURCE_DATABASE_URL; printf '\n'
   export SOURCE_DATABASE_URL
   cargo run --locked --release -- inventory \
     --manifest /secure-volume/rehearsal-db-inventory.json
   ```

4. Resolve every inventory blocker. A `running` sync, unknown table/column,
   malformed recurrence, missing credential, or failed login/decryption canary
   is a stop, not a warning.
5. Produce a rehearsal artifact using the complete commands in
   [`tools/pg-to-turso/README.md`](tools/pg-to-turso/README.md). The exporter
   uses one `REPEATABLE READ, READ ONLY` PostgreSQL transaction, preserves IDs,
   flattens `intg.*` to `intg_*`, restores sequences, and validates every row
   after SQLite readback.
6. Run [`tools/object-cutover`](tools/object-cutover/README.md) against that
   artifact. For local-only objects, backfill to a fresh S3/R2 prefix and
   require downloaded SHA-256 verification. Missing, duplicate, unsafe, and
   orphan objects require operator review.
7. Import into a **new** rehearsal database:

   ```sh
   turso db create trust-deeds-rehearsal --from-file \
     /secure-volume/trust-deeds.sqlite --wait
   ```

   The exporter retains the import requirements in the file header: WAL mode,
   4096-byte pages, UTF-8, and auto-vacuum disabled. The
   [Turso CLI reference](https://docs.turso.tech/cli/db/create) currently
   documents a 2 GB limit for `--from-file`; record the Turso CLI version and
   exact command used in the rehearsal notes.
8. Point a protected Vercel preview at only that rehearsal database/object
   prefix. Keep writes and cron off. Verify login, balances/forecast totals,
   integrations, representative loan workspaces, inbox metadata and object
   reads, migration checksums, foreign keys, and credential decryption.
9. Rehearse the write-gate transitions and a pre-write rollback. Delete neither
   source data nor source objects.

Two consecutive clean rehearsals with reviewed manifests are the production
go/no-go prerequisite.

## Final source freeze and export

1. Put up a maintenance response and stop the old Coolify **application**
   container. Leave PostgreSQL and its volume running. Stopping the process is
   the source write lock: it removes the in-memory scheduler, browser writes,
   syncs, and webhook handler without backporting a second queue/gate system.
2. Confirm there are no other application clients or active provider syncs in
   PostgreSQL. The public Resend tunnel must return a retryable non-2xx during
   this window so an event is not acknowledged and lost.
3. Re-run the object inventory/backfill to catch changes since rehearsal.
4. Run a fresh relational export to new artifact/manifest names. Do not reuse
   or overwrite rehearsal files.
5. Compare source and artifact counts, key ranges, canonical digests, monetary
   bit totals, object hashes, login/decryption canaries, migration ledger,
   sequence probes, `integrity_check`, and `foreign_key_check`. Any unexplained
   drift aborts the cutover.
6. Import the artifact into a newly named production Turso database. Never
   import over the rehearsal database or a database that has accepted writes.
7. Configure Vercel with that database's URL/token plus the unchanged crypto,
   object-store, provider, session, webhook, and cron secrets. Keep the target
   read-only and scheduler off.

## Traffic, write enablement, and rollback

1. Smoke-test the production deployment while protected/read-only: health,
   login, critical financial reads, integration/workspace/inbox pages, and
   representative signed object redirects.
2. Move traffic to Vercel, still read-only. This is the last immediate rollback
   point: traffic can return to the stopped PostgreSQL app with no data merge.
3. At the explicit go/no-go, enable browser/manual writes with
   `trust-deeds-ops enable-writes`. This does **not** enable cron.
4. Switch the verified Resend webhook, process one controlled/replayed event,
   and verify its durable dedupe state.
5. Invoke one controlled manual TMO sync. Verify its durable `sync_log`, totals,
   and overlap rejection.
6. Set TMO's stored cadence to `daily`, enable cron separately with
   `trust-deeds-ops scheduler-on`, then monitor
   errors, database latency, stale/running executions, inbound-email failures,
   and financial totals through the observation window. Verify that the first
   authenticated page activity after an integration is over one hour stale
   performs one inline refresh and concurrent requests collapse onto its
   durable execution claim.
7. Keep PostgreSQL and the old object corpus retained and read-only until the
   rollback window is explicitly closed. Then document retention/deletion as a
   separate operation; cutover never destroys the source.

If a check fails before target writes, set the target read-only, scheduler off,
route traffic back, and restart the old app. If it fails after target writes,
leave both sides protected and classify/replay the target delta before choosing
rollback or fix-forward.
