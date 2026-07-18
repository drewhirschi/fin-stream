# PostgreSQL to Turso cutover exporter

This standalone, fail-closed utility turns one upgraded Trust Deeds PostgreSQL
snapshot into a validated SQLite file suitable for Turso's SQLite-file import
flow. It is intentionally outside the application dependency graph and is not
a member of a Cargo workspace.

The exporter applies the exact checked-in target migrations `0001` through
`0005`. It maps all 22 durable source relations, restores all 16 serial
sequences, validates a read-back copy, and publishes the artifact only after
every check passes. It does not connect to Turso or mutate PostgreSQL.

## Run it without exposing secrets

Use a quiesced, fully upgraded PostgreSQL database. Stop provider sync and
inbound-email writes first; a `sync_log` row still marked `running` is a hard
blocker. Both output paths must be absolute, outside the repository, and in an
existing directory.

```sh
cd tools/pg-to-turso

# Shell built-ins keep the credential-bearing URL out of argv and history.
read -r -s -p 'Source PostgreSQL URL: ' SOURCE_DATABASE_URL; printf '\n'
export SOURCE_DATABASE_URL

cargo run --locked -- inventory \
  --manifest /secure-volume/trust-deeds-inventory.json
```

Inspect the inventory manifest and resolve every blocker. Then provide a known
active login and, when provider credential rows exist, the same encryption key
used by the current application:

```sh
read -r -p 'Cutover login email: ' CUTOVER_LOGIN_EMAIL
read -r -s -p 'Cutover login password: ' CUTOVER_LOGIN_PASSWORD; printf '\n'
read -r -s -p 'Application encryption key: ' APP_ENCRYPTION_KEY; printf '\n'
export CUTOVER_LOGIN_EMAIL CUTOVER_LOGIN_PASSWORD APP_ENCRYPTION_KEY

# Only when inventory/canary review proves a row was written with a known
# prior key. Omit this for the normal path.
# read -r -s -p 'Legacy application encryption key: ' LEGACY_APP_ENCRYPTION_KEY; printf '\n'
# export LEGACY_APP_ENCRYPTION_KEY

cargo run --locked -- export \
  --output /secure-volume/trust-deeds.sqlite \
  --manifest /secure-volume/trust-deeds-validation.json

unset SOURCE_DATABASE_URL CUTOVER_LOGIN_EMAIL CUTOVER_LOGIN_PASSWORD APP_ENCRYPTION_KEY LEGACY_APP_ENCRYPTION_KEY
```

Files are created with mode `0600`, existing files are never overwritten, and
failed exports remove their incomplete SQLite artifact. The source URL, output
paths, login values, encryption key, decrypted secrets, ciphertext, nonces,
and row bodies are never written to logs or the manifest.

## What is copied

The source schema is an anti-corruption boundary. PostgreSQL's `intg.*` tables
are copied into the flattened SQLite `intg_*` names used by the NextRS app;
the target does not attempt to emulate PostgreSQL schemas.

- Authentication: `app_user`. Legacy `tower_sessions.session` is deliberately
  discarded, and target `app_session` must start empty.
- Forecasting: `account`, `stream`, `stream_view`, `stream_view_stream`,
  `stream_schedule`, and `stream_event`.
- Providers: integration connections, TMO overview/loan/payment/account data,
  TMO and Monarch credentials, and payment-to-event links.
- Operations: portfolio snapshots, settings, and durable `sync_log` execution
  history.
- Underwriting and inbox: loan workspaces/photos, received emails, and
  attachments, including external IDs, raw JSON, object-storage keys, and
  source/image URLs.

IDs, nullable values, provider payload text, ciphertext/nonces/key versions
(except an explicitly reviewed legacy-key rewrap), raw webhook bodies, object
keys, and source identities are preserved. Dates
are validated as real `YYYY-MM-DD` values. Text timestamps are normalized to
UTC with fixed milliseconds (`YYYY-MM-DDTHH:MM:SS.mmmZ`). Finite floating-point
values are copied exactly and compared by IEEE-754 bits, not display decimals.

Legacy public TMO/integration tables, a legacy
`stream_event.scheduled_date`, an unknown relation, a column/type/nullability
drift, an unsupported sequence, or any unclassified data blocks the export.
There is no `--force`, `--allow-partial`, or overwrite mode.

## Deliberate transforms

Several target fields do not exist in the old shape and therefore have an
explicit, test-covered rule:

- A known pre-iteration stream schema may omit both `direction` and
  `amount_certainty`. For that exact shape only, the exporter applies the
  legacy application's upgrade rules: manual expenses and credit cards flow
  out, everything else flows in; credit cards are estimated and everything
  else is known. Legacy signed event/schedule amounts become magnitudes with
  `abs`. A source missing only one column, or any other stream drift, blocks.
- If a credential cannot be decrypted by the current key, the export blocks by
  default. Supplying `LEGACY_APP_ENCRYPTION_KEY` explicitly allows only those
  mismatched rows to be decrypted in zeroizing memory and re-encrypted with a
  random nonce under the current key in the artifact. The manifest records
  provider counts and both non-reversible key fingerprints, never key material
  or plaintext. Rows already using the current key are copied byte-for-byte.
- A non-null account balance gets `balance_as_of_date` from its validated
  `balance_updated_at`; a balance without that timestamp blocks the export.
- A legacy received event gets `actual_amount` from its source amount.
- Legacy schedule-event identities are converted to the target's stable
  cadence slots. Schedule edits become explicit override columns. Missing or
  cross-stream schedules, malformed identities, cadence ambiguity, or two rows
  collapsing to one slot all block the export.
- Legacy `sync_log.scheduled_for` did not exist, so imported rows use `NULL`.
  Null counters become zero. A null legacy `connection_slug` maps to `tmo` only
  when exactly one TMO connection exists and the row provably predates every
  non-TMO connection. Running rows, terminal rows without `finished_at`,
  negative counters, and backwards timestamps block the export.
- Stored inbound attachments move from the legacy user-filename key to the
  target writer's deterministic `emails/<email provider id>/attachments/<attachment provider id>`
  key. The source key must exactly match the legacy writer contract (or already
  be canonical); unknown layouts block without exposing filenames in errors.

PostgreSQL has no durable tombstone for already hard-deleted generated schedule
occurrences. Their absence cannot safely be distinguished from a slot that was
never projected. Review those gaps during the freeze; do not synthesize
exclusions from absence alone.

## Credential and operational canaries

Every active `mortgage_office` connection must be the `tmo` connection and have
a TMO credential. Every active `monarch` connection must be the `monarch`
connection and have a Monarch credential. Any other active provider must be
explicitly marked `{"syncable":false}` in valid metadata. Credential rows may
not belong to a different provider.

When credentials exist, `APP_ENCRYPTION_KEY` is required. The exporter derives
the production AES-256-GCM key, decrypts every source credential, and repeats
the canary against every SQLite read-back credential. Plaintext is immediately
zeroized. The manifest records only row counts, pass booleans, and the approved
eight-hex-character derived-key fingerprint.

The login canary uses the target's email normalization and Argon2id verifier
against both source and read-back rows. The manifest records only pass
booleans. A malformed active hash, missing user, unknown email, or password
mismatch blocks publication.

The target-only `operation_control` table is validated as exactly one inert
row: `id=1`, `mode='read_only'`, and `scheduler_enabled=0`. Importing data can
therefore never enable provider writes or cron by accident.

## Validation and Turso handoff

Within one `REPEATABLE READ, READ ONLY` transaction, the exporter:

1. inventories every table, column, type, nullability, and owned sequence;
2. performs typed source conversion and semantic validation;
3. applies migrations `0001`–`0005` to a private temporary SQLite file;
4. loads rows in foreign-key order and restores effective next-ID values;
5. probes every restored sequence inside rolled-back savepoints;
6. reads every row back and compares count, key range, canonical digest, and
   financial bit totals;
7. runs storage-class, natural-key, foreign-key, migration-ledger,
   `integrity_check`, login, and credential checks; and
8. vacuums and checkpoints to a sidecar-free, single-file artifact, verifies
   Turso's import settings (`WAL`, 4096-byte pages, UTF-8, no auto-vacuum), and
   publishes it atomically.

For the remote handoff, create a **new** Turso database from the produced
SQLite file; do not import over a database already used by an application:

```sh
turso db create trust-deeds-staging --from-file /secure-volume/trust-deeds.sqlite --wait
```

The [Turso `db create` documentation](https://docs.turso.tech/cli/db/create)
currently limits `--from-file` imports to 2 GB. Record the CLI version and
command in the cutover manifest review. Configure a Vercel preview with that
database's URL/token and the same application encryption key. Before enabling
anything, verify remotely:

- the five migration ledger names/checksums and `PRAGMA foreign_key_check`;
- representative row counts from each domain and preserved object keys;
- the known login through the preview UI;
- provider credential decryption through read-only provider pages; and
- `operation_control` is still the inert singleton.

Keep the preview read-only while comparing golden account/forecast/provider,
workspace, and inbox pages. Only after sign-off should traffic move. Enable
manual operations first; scheduler enablement is a separate, explicit change.
