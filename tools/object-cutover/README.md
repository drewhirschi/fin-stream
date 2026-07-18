# Object-data cutover

This standalone utility inventories the bytes referenced by the validated target
SQLite artifact and can backfill a verified local corpus to an S3-compatible
store. It is intentionally outside the application dependency graph, excluded
from the Vercel build by the repository-root `.vercelignore`, and never writes
to SQLite.

It accounts for:

- stored and external-only `intg_loan_workspace_photo.image_url` values;
- non-empty `intg_received_email.body_s3_key` values;
- non-null `intg_received_email_attachment.s3_key` values.

`/media/loan-workspace/...` and `/static/loan-images/...` photo URLs both map to
the canonical `loan-workspace/...` namespace. `s3://` URLs plus standard AWS S3
virtual/path-style and R2 endpoint URLs are also treated as stored objects;
unrecognized HTTP(S) photo hosts are kept as external-only for operator review.
The retained app physically stored those photos as `<loan-account>/<file>`
below its configured local root or S3 prefix. Inventory maps that legacy layout
to the canonical key; backfill writes only canonical `loan-workspace/...`
destination keys. Email objects already carry their `emails/...` namespace. A
source containing both physical forms of one photo is ambiguous and blocks.
Empty email body keys are treated as “no body object,” never as the bucket or
prefix root. Unsafe paths, duplicate references, missing objects, and orphan
objects make the manifest invalid and the process exits non-zero.

## Build and test

From this directory:

```sh
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

The checked-in `Cargo.lock` makes the cutover binary reproducible. Do not add
this package to the NextRS application workspace.

## Inventory a local corpus

All database, local source, and output paths must be absolute. Output is a new,
atomically published `0600` file and is never overwritten.

```sh
cargo run --locked --release -- inventory \
  --sqlite /absolute/private/cutover/target.db \
  --source local \
  --local-source /absolute/private/legacy-objects \
  --manifest /absolute/private/cutover/local-objects.json
```

The utility does not print or serialize any input path. Point `--local-source`
at the legacy storage root itself (normally `LOAN_IMAGE_STORAGE_DIR`, such as
`static/loan-images`), not at a synthetic `loan-workspace` subdirectory. Run it
against a quiesced, retained source directory; symlinks are rejected.

## Inventory retained S3/R2 data

S3-compatible connection details are accepted **only** through environment
variables. They are never command-line arguments or manifest fields. Avoid
shell history for the secret:

```sh
export OBJECT_CUTOVER_S3_ENDPOINT='https://…'
export OBJECT_CUTOVER_S3_REGION='auto'
export OBJECT_CUTOVER_S3_BUCKET='…'
export OBJECT_CUTOVER_S3_ACCESS_KEY_ID='…'
read -rsp 'S3 secret access key: ' OBJECT_CUTOVER_S3_SECRET_ACCESS_KEY; echo
export OBJECT_CUTOVER_S3_SECRET_ACCESS_KEY
export OBJECT_CUTOVER_S3_PREFIX='trust-deeds/prod' # optional, no leading slash

cargo run --locked --release -- inventory \
  --sqlite /absolute/private/cutover/target.db \
  --source s3 \
  --manifest /absolute/private/cutover/retained-s3-objects.json

unset OBJECT_CUTOVER_S3_SECRET_ACCESS_KEY OBJECT_CUTOVER_S3_ACCESS_KEY_ID
```

`OBJECT_CUTOVER_S3_ALLOW_HTTP=true` exists only for a local S3 emulator. HTTPS
is required by default. For retained legacy S3/R2, use the old application's
exact `S3_KEY_PREFIX`; inventory maps its physical photo layout to canonical
keys. It performs list/get operations only and does not copy, rewrite, delete,
or repair retained data.

## Backfill local bytes to S3/R2

Run and review a clean local inventory first. Then use the explicit write mode
with a fresh manifest path:

```sh
cargo run --locked --release -- backfill-local-to-s3 \
  --sqlite /absolute/private/cutover/target.db \
  --local-source /absolute/private/legacy-objects \
  --manifest /absolute/private/cutover/backfill-objects.json
```

The destination uses the same environment variables above. Backfill refuses to
start if the local inventory has blockers. Each object is created with its
canonical key, source/DB content type, and SHA-256 metadata. Conditional create
prevents overwrites. An existing object is accepted only when downloading it
produces the same SHA-256; newly uploaded objects are downloaded and hashed too.
The mode is idempotent, but a failed run may have safely created a subset of the
objects, so rerun with a new output path after correcting the blocker.

## Manifest and handoff

The JSON manifest contains only reference kind, database ID, canonical key,
content type, byte size, SHA-256, presence/backfill disposition, aggregate
counts, and non-sensitive blocker codes. It never includes database paths,
local paths, endpoints, bucket names, prefixes, credentials, or original
external photo URLs.

Before traffic cutover, require a valid manifest and independently retain it
beside the PostgreSQL-to-Turso manifest. Production execution still requires:

1. access to the validated SQLite artifact produced by the relational exporter;
2. read access to the legacy local directory or retained S3/R2 prefix;
3. for backfill, create/list/get access to the destination prefix;
4. an operator review of external-only photo references and all zero counts;
5. a protected NextRS preview smoke test that downloads representative photos,
   email bodies, and attachments from the destination.

Keep the old database and object corpus read-only through the rollback window.
This utility deliberately does not change database keys or application storage
configuration.
