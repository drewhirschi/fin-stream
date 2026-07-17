# NextRS + Turso + Vercel Port

**Status:** In progress — isolated NextRS shell implemented; owner decisions called out below<br>
**Date:** 2026-07-11<br>
**Source app:** Trust Deeds on Axum/PostgreSQL/Coolify<br>
**Target app:** NextRS 0.3.6 on Vercel Rust Functions with remote libSQL/Turso

## Working assumption

This plan treats the request as a port to **NextRS**, the Rust framework in
`/home/drew/work/nextrs`, deployed on Vercel. The follow-up reference to
“Next.js” is assumed to mean NextRS's Vercel-oriented deployment model, not a
rewrite from Rust to TypeScript/Next.js. A literal Next.js rewrite would be a
different plan.

The target is a standalone Rust package under `nextrs-app/`. Vercel's Root
Directory is `nextrs-app`, so every path used to build or deploy the target is
relative to that directory. The existing repository-root Cargo package remains
the independently buildable PostgreSQL/Axum source application while behavior is
ported in slices. During the migration, do not make the root package a Cargo
workspace and do not add a repository-root `build.rs`; either `cd nextrs-app` or
use `cargo --manifest-path nextrs-app/Cargo.toml ...` for target commands.

## Recommendation

Do the migration in two milestones:

1. **Platform and persistence port:** NextRS file routing, the existing
   Rust/Askama/HTMX/Alpine UI, Turso, database-backed execution tracking,
   object storage, and Vercel. Preserve URLs, page behavior, and the visual
   surface.
2. **Optional client modernization:** after production is stable, move selected
   pages to NextRS TSX/React/TanStack Query one at a time.

NextRS supports Rust pages and Askama without React. Using that path first keeps
this from becoming a simultaneous framework, database, frontend, authentication,
execution-coordination, and infrastructure rewrite. Do not combine a React
conversion with the PostgreSQL-to-Turso cutover.

The persistence change is broad but mechanical: the app has 233 SQLx query
calls, 121 `PgPool` references, and 17 `FromRow` derives. SQLx cannot use Turso's
remote libSQL protocol, so the data-access layer must move to the `libsql` crate;
the domain model and most provider/business logic can remain Rust as-is.

## Success criteria

The port is complete when:

- Every current route has an explicit parity decision and all retained routes
  pass an HTTP contract suite.
- The production data export validates locally and remotely by count, key range,
  canonical row digest, foreign keys, and financial/domain totals.
- Forecast, streams, canvas, integrations, loan workspace, inbox, authentication,
  sync, webhook, media, and health flows pass browser and API tests on a Vercel
  preview backed by staging Turso.
- A clean checkout performs the exact Vercel production build; no generated
  frontend bundle is treated as source.
- No request depends on process memory, a local writable filesystem, or a
  detached `tokio::spawn` task for correctness.
- Production has the owner-approved network posture and mandatory application
  auth; if the existing outer gate is retained, it uses Vercel's paid All
  Deployments protection. The webhook is cryptographically verified and
  replay-safe either way.
- PostgreSQL remains an untouched rollback artifact through the cutover window,
  with the point after which rollback requires data replay documented and
  acknowledged.

## Not in scope

- A TypeScript/Next.js rewrite.
- A visual or product redesign.
- React/TSX in the first production cutover.
- Multi-user roles, organizations, or a new permissions model.
- Converting money from `f64`/`REAL` to integer cents. That should be a separate,
  carefully validated data-model migration.
- Turso embedded replicas or local-first synchronization. Vercel's ephemeral
  filesystem makes direct remote libSQL the appropriate first target.
- A generic abstraction capable of running PostgreSQL and Turso in production at
  the same time.
- Dual writes or a zero-downtime database migration.
- Moving S3/R2 objects to Vercel Blob.
- Changing TMO, Monarch, Resend, property-media, or forecast business rules except
  where characterization exposes an existing correctness or security defect.

## Ground truth discovered

The checked-in descriptive docs are stale in several important places. The live
application is PostgreSQL, not SQLite; it uses build-time Tailwind, session auth,
and a Coolify/Tailscale deployment. The effective database schema is the final
state produced by `src/db/mod.rs::run_migrations`, not `schema.sql`.

Current runtime responsibilities are concentrated in:

- `src/main.rs`: initialization, PostgreSQL session storage, router assembly,
  static serving, and the forever scheduler.
- `src/routes/`: full pages, JSON APIs, HTMX fragments, form actions, sync,
  auth, webhook, health, and media proxy endpoints.
- `src/db/`: imperative PostgreSQL schema history and all persistence.
- `src/tmo/`, the Monarch and Resend clients, forecast/schedule logic, crypto,
  S3 media, and display filters: mostly reusable domain/provider code.
- `templates/` and `static/`: the current UI and vendored browser dependencies.

The streams/forecast iteration is checkpointed at `2ec7a87`. Treat that as the
candidate source baseline, but complete its Phase 0 PostgreSQL-backed tests,
validate the irreversible amount-sign backfill, constrain the new direction and
certainty fields, and resolve broad event delete/reconcile behavior before the
port consumes it. Do not translate a moving or only vacuously tested schema.

## What already exists

- `/home/drew/work/nextrs` provides the file router, generated registry,
  Vercel adapter, Rust/Askama page path, and optional React toolchain.
- `nextrs-app/` already provides the isolated HTML-first NextRS 0.3.6 package,
  generated registry, shared local/Vercel router, Askama probe page, liveness
  route, disabled speculative prefetch, exact Vercel runtime configuration, and
  Rust 1.88.0 toolchain. Vercel must use this directory as the project Root
  Directory.
- `/home/drew/work/onenote-extractor` already demonstrates NextRS + remote
  libSQL/Turso + Vercel, including lazy process-global database initialization,
  cookie/auth middleware mechanics, Vercel entrypoint, and deployment layout.
  Reuse the deployment and middleware structure, but retain server-side session
  revocation as noted below.
- The current Trust Deeds templates are a behavioral and visual specification.
- Existing pure forecast, cadence, schedule-occurrence, provider-client, crypto,
  S3, and formatting code should be moved with focused adapter changes rather
  than rewritten.
- NextRS's `react-todos` example is the reference only for the later TSX/client
  milestone, including generated client code and self-contained Vercel builds.

## Target architecture

```text
Browser
  |
  | HTTPS
  v
Vercel access layer (Advanced Protection if chosen; app auth always)
  |
  v
Vercel CDN ----------------------------> /static/*
  |
  v
nextrs-app/api/index.rs / StreamingVercelLayer
  |
  v
outer Axum/Tower middleware: request ID, headers, auth, CSRF/origin
  |
  v
NextRS generated Axum router
  |
  +--> nextrs-app/app/**/route.rs --> fallible Askama HTML, JSON, HTMX, forms, sync/cron
  |
  +--> nextrs-app/app/**/page.rs  --> only genuinely infallible 200-only HTML
  |
  +--> Rust domain/services
          |
          +--> remote libSQL/Turso (canonical app data + sync/email state)
          +--> TMO / Monarch / Resend APIs
          +--> S3/R2 (media bytes)
```

The production request path must be reconstructible from configuration and
durable external state. A warm Vercel instance may hold only disposable cached
handles and rebuildable provider clients, such as a cached `libsql::Database`
handle. No process-global value is authoritative mutable application state:
sync status, scheduling decisions, sessions, email state, and operation control
all live in Turso or another named durable service, and correctness is unchanged
when an isolate disappears.

The target and source packages remain deliberately separate during the port.
The root `Cargo.toml`, `Cargo.lock`, `Dockerfile`, absence of a root `build.rs`,
source tree, and Coolify build are not target scaffolding and must not be
repurposed for Vercel. Keep the legacy root application buildable as the
comparison/source implementation until an explicitly scoped vertical slice
moves. Do not add
`nextrs-app` as a root Cargo workspace member or make the target depend on the
root application crate: either would couple Vercel builds to the PostgreSQL/AWS
legacy dependency graph. Copy or extract reusable pure modules into
`nextrs-app/src/` only as their owning slice is ported.

### Proposed repository layout

```text
Cargo.toml                     legacy Axum/PostgreSQL package; unchanged here
Cargo.lock                     legacy lockfile; unchanged here
src/, templates/, static/      comparison/source app until slices move
Dockerfile                     legacy Coolify build; unchanged here

nextrs-app/                    standalone Vercel Root Directory and Cargo package
  Cargo.toml                   exact NextRS/libSQL/Vercel pins
  Cargo.lock                   target lockfile, independent of the legacy lockfile
  build.rs                     emit_registry("app", "src/lib.rs", "nextrs_routes.rs")
  api/index.rs                 Vercel entrypoint + StreamingVercelLayer
  rust-toolchain.toml          exact Rust 1.88.0 HTML-first toolchain
  app/
    page.rs                    implemented infallible Askama probe
    route.rs                   later dashboard GET with real response/result
    login/route.rs
    streams/route.rs
    forecast/route.rs
    canvas/route.rs
    inbox/route.rs
    integrations/route.rs
    api/**/route.rs            current JSON and mutation endpoints
    sync/**/route.rs           inline manual sync + durable status endpoints
    webhooks/resend/route.rs   verified persistence + inline processing
    internal/cron/route.rs     authenticated due-sync/session cleanup endpoint
    healthz/route.rs           implemented process-liveness probe
    ready/route.rs
  public/static/               copied target assets; legacy static/ stays intact
  src/
    lib.rs                     generated registry include + shared router
    main.rs                    local listener using the shared router
    auth.rs                    Turso-backed session auth + CSRF policy
    http/middleware.rs         outer global allowlist/auth/headers/request IDs
    db/
      mod.rs                   disposable remote/local libSQL handle wrapper
      row.rs                   explicit checked row decoders
      migrations.rs            migration runner used only by CLI/tests
      ...                      repositories by current domain
    services/
      sync.rs                  sync_log claim, inline execute, and finalize logic
      ...                      provider clients and reusable domain code
    templates/                 Askama structs
  templates/                   target templates copied a slice at a time
  migrations/
    0001_initial.sql           canonical final SQLite/libSQL schema
    0002_*.sql                 forward-only versioned changes
  tools/
    pg-to-turso/               standalone one-shot exporter; not deployed
    check_intg_boundary.sh     target intg_* ownership check
  assets/app.input.css         target Tailwind/DaisyUI source
  package.json
  package-lock.json
  README.md
  vercel.json
  .vercelignore
  .cargo/config.toml           must contain a `[build]` table, even if empty
```

Do not introduce NextRS `loading.*` files in milestone one. Rust page streaming
can otherwise leave a loading shell visible after a render error. Disable browser
speculation-rule GET prefetch initially with `PrefetchConfig::OFF`; re-enable it
only after every browser-reachable GET is proven side-effect-free. The unlinked,
secret-authenticated cron GET is an intentional exception and must never appear
in navigation or speculation rules.

## NextRS integration choices

Pin the known-good deployment stack exactly for the first port:

- `nextrs = "=0.3.6"`
- start Phase 1 with `libsql = { version = "=0.9.30", default-features = false,
  features = ["remote", "tls"] }` for production, enabling `core` only for the
  local test/export build; retain the exact version that passes all three paths
- `vercel_runtime = { version = "2", features = ["axum"] }` in Cargo
- `vercel-rust@4.0.11` as the `nextrs-app/api/index.rs` runtime in
  `nextrs-app/vercel.json`
- `[[bin]]` named `index` with `path = "api/index.rs"` in
  `nextrs-app/Cargo.toml`
- Rust 1.88.0 in `nextrs-app/rust-toolchain.toml` for local CI and Vercel; the
  resolved Askama 0.15.6 requires at least Rust 1.88
- NextRS build support in `build-dependencies`
- NextRS `vercel` support at runtime

Reserve Rust 1.96 for the optional TSX milestone, where the rolldown/oxc
toolchain may require it. Do not raise the HTML-first port or the legacy root
package to 1.96 preemptively.

Do not enable libSQL `core`, replication, sync, or embedded-replica features in
the Vercel artifact. The test/dev feature set and standalone exporter add `core`
for local SQLite while production uses only direct remote connections.

The Vercel Rust runtime is Beta and NextRS is pre-1.0. This is not the same
maturity or first-party path as deploying Next.js to Vercel. Phase 1 is a real
go/no-go gate: if clean deploys, response semantics, build time, or function
behavior fail the spikes, stop before the 233-query conversion and choose a
serverful Rust target or a separately planned first-party Next.js rewrite.
NextRS upgrades must be isolated follow-up changes with a clean build and
route-contract run.

NextRS 0.3.6 Rust pages return only `String`; the router turns that into 200 HTML.
Every database-backed Trust Deeds page is fallible, so serve it from a thin GET
function in `nextrs-app/app/**/route.rs` that calls the ported view
construction/Askama rendering and can return a real redirect, status, headers,
or typed error. Reserve `nextrs-app/app/**/page.rs` for genuinely infallible
200-only content. Do not convert database failures into an error-looking page
with status 200, and do not fork/vendor NextRS merely to add a fallible-page
contract during this port.

The generated route methods currently assume Axum state `()`, but Axum
`Extension<T>` extracts from request extensions and does not require router
state. Construct immutable `RuntimeHandles` containing disposable database and
provider handles plus immutable configuration in each entrypoint, wrap them in
`Arc`, and layer them onto the shared router with `Extension`. Do not introduce
an authoritative process-global service container, mutable singleton, or
instance-affinity assumption. Initialization failures remain retryable because a
new isolate/request runtime can reconstruct every handle from configuration and
durable state.

For target local development, run commands from `nextrs-app/` (for example,
`cargo watch -x 'run --bin nextrs-probe'`) because the NextRS dev watcher does
not cover all copied `templates/`, `public/`, and `.env` inputs. The legacy
root-package `cargo watch -x run` command remains independent.

### CSS and deploy build

The existing UI is not static CSS alone. The root `package.json` runs Tailwind 4
and DaisyUI 5 over the legacy `templates/` and `src/**/*.rs` to generate
`static/app.css`. Copying only the already-generated file into
`nextrs-app/public/` would ship stale CSS as soon as a target class changes.

Keep `nextrs-app/package-lock.json` authoritative for the target and make the
Vercel build self-contained:

1. Add the target source entry point at `nextrs-app/assets/app.input.css`,
   initially copied from the legacy source, with Tailwind `@source` paths
   relative to `nextrs-app/` covering `templates/`, `src/**/*.rs`, and
   `app/**/*.rs`. Keep the root CSS inputs untouched for the legacy deployment.
2. Add target-local `nextrs-app/package.json` and `package-lock.json`; from the
   Vercel Root Directory, make their CSS script emit `public/static/app.css`
   (`nextrs-app/public/static/app.css` from the repository root).
3. With Vercel Root Directory set to `nextrs-app`, set its install step to
   `npm ci --no-audit --no-fund` and its build step to run the CSS build before
   `cargo build --release --locked --bin index`.
4. Run the same commands from a clean checkout in CI and in Phase 6.
5. Exclude `.git`, `target`, `node_modules`, worktree metadata, and local data
   from Vercel uploads without excluding required source inputs.

The vendored HTMX, Alpine, fonts, and other browser dependencies are copied into
`nextrs-app/public/static/vendor/`; the production page must not acquire CDN
dependencies. The source copies under root `static/` remain in place while the
legacy application is the comparison deployment.

## Route-port strategy

Create a checked-in contract matrix during Phase 0 with path, method, auth,
request type, success response, errors, side effects, and target file. Use these
default mappings:

| Current surface | First-port target | Notes |
|---|---|---|
| `/`, `/streams`, `/forecast`, `/canvas`, `/inbox*` | GET in `nextrs-app/app/**/route.rs` | Render Askama with real error statuses; preserve Alpine behavior. |
| `/integrations*`, `/loans`, `/payments` | GET in `nextrs-app/app/**/route.rs` | Preserve real redirects/aliases until telemetry proves removable. |
| `/api/**` | `nextrs-app/app/api/**/route.rs` | Preserve JSON field names, status codes, and content types. |
| `/sync/**` | `nextrs-app/app/sync/**/route.rs` | Claim a `sync_log` row atomically, execute before returning, and expose durable status. |
| `/login`, `/logout` | GET/POST in `nextrs-app/app/**/route.rs` | Deliberately invalidate PostgreSQL sessions at cutover. |
| `/health`, `/healthz`, `/ready` | `nextrs-app/app/**/route.rs` | Separate process liveness from remote dependency readiness. |
| `/bench/render`, `/health/crypto` | protected `nextrs-app/app/**/route.rs` | Do not expose diagnostics publicly. |
| `/media/**` | `nextrs-app/app/media/**/route.rs` | Return authenticated signed redirects; never stream object bytes through the Rust function in production. |
| `/webhooks/resend` | `nextrs-app/app/webhooks/resend/route.rs` | Persist/dedupe the received email, then process it inline; return 200 only after success. |
| static files | `nextrs-app/public/static/**` | Vercel CDN serves before catch-all rewrite. |

Copy the existing full-page templates into `nextrs-app/templates/` a vertical
slice at a time rather than forcing them through a new shared NextRS layout. The
legacy root templates remain the comparison source. If a shared target layout is
introduced later, Askama child HTML must be rendered with `|safe` and covered by
escaping tests.

## Turso/libSQL persistence design

### Driver and connection lifecycle

Replace SQLx/PostgreSQL in the deployed crate with a small cloneable wrapper over
`Arc<libsql::Database>`:

- Production: `libsql::Builder::new_remote(TURSO_DATABASE_URL,
  TURSO_AUTH_TOKEN)`.
- Local development/tests: `Builder::new_local(path)` or an isolated temporary
  database running the same migrations.
- Initialize the database object once per warm Vercel instance; vend a connection
  per operation or transaction.
- On **every connection**, enable and verify `PRAGMA foreign_keys = ON` because it
  is connection-scoped.
- Expose `query_one`, `query_optional`, `query_all`, and transaction helpers with
  explicit row mapping and checked integer/bool conversion.
- Return `Result` from reads. A remote or row-shape error must not become an empty
  dashboard.

Gate the local builder and `DatabaseConfig::Local` code with an application
feature such as `local-db = ["libsql/core"]`; use `cargo run --features local-db`
and `cargo test --features local-db` from `nextrs-app/` locally (or use the
equivalent `--manifest-path nextrs-app/Cargo.toml` commands). The default Vercel
build contains only remote/TLS support. CI inspects `cargo tree -e features`
from the target package for the release target and fails if libSQL core,
replication, or sync enters the function graph.

Remote query latency changes the cost model. Instrument query count and database
time by route, combine related reads, and keep multi-statement writes in one
short transaction. Do not hold a transaction across TMO, Monarch, Resend, S3, or
other network calls.

### Canonical schema

Do not translate the historical PostgreSQL boot migration statement by statement.
Derive the final upgraded source schema, then express it as a clean
`nextrs-app/migrations/0001_initial.sql`. Subsequent changes are versioned,
forward-only migrations recorded in
`_schema_migrations(version, name, checksum, applied_at)`.

Run migrations through an explicit CLI/deploy operation. A Vercel cold start only
opens the remote client and asserts the expected schema version. Admin creation
and default stream/configuration seeding also become explicit bootstrap commands;
they must not hash a password or mutate schedules on every cold start.

SQLite/libSQL does not have PostgreSQL-style application schemas. The canonical
Turso database must contain ordinary tables with flattened `intg_` names; no
target migration or runtime query may contain `CREATE SCHEMA`, an `intg.`
qualifier, or a schema-search-path assumption:

| PostgreSQL | libSQL |
|---|---|
| `intg.integration_connection` | `intg_integration_connection` |
| `intg.tmo_import_overview` | `intg_tmo_import_overview` |
| `intg.tmo_import_loan` | `intg_tmo_import_loan` |
| `intg.tmo_import_payment` | `intg_tmo_import_payment` |
| `intg.tmo_account` | `intg_tmo_account` |
| `intg.tmo_credential` | `intg_tmo_credential` |
| `intg.monarch_credential` | `intg_monarch_credential` |
| `intg.loan_workspace` | `intg_loan_workspace` |
| `intg.loan_workspace_photo` | `intg_loan_workspace_photo` |
| `intg.tmo_payment_event_link` | `intg_tmo_payment_event_link` |
| `intg.received_email` | `intg_received_email` |
| `intg.received_email_attachment` | `intg_received_email_attachment` |

Flattening the physical names does not flatten the architectural boundary.
Application/domain code continues to call Rust repository APIs such as
`db::integrations`, `db::emails`, and provider-neutral view-model adapters. Only
the integration repositories, canonical migrations, and exporter mapping may
mention `intg_*` table identifiers or TMO-shaped columns. Add
`nextrs-app/tools/check_intg_boundary.sh` during the port so its `\bintg[._]`
rule covers both source `intg.` and target `intg_` storage names, give it an
allowlist for the target repository layout, and run it in target CI. Leave the
root legacy check intact while both applications coexist. Do not leak the
physical rename into route, template, forecast, or general stream APIs.

Do not emulate PostgreSQL schemas with SQLite `ATTACH` or a separate integration
database. Turso's `ATTACH` support is deprecated and read-only, and splitting the
tables would prevent ordinary cross-boundary foreign keys and atomic
transactions between integration rows, link rows, and canonical stream events.
Use one Turso database with the flattened names and preserve isolation in Rust
module/repository ownership instead.

### SQL conversion rules

| PostgreSQL construct | libSQL form |
|---|---|
| `BIGSERIAL PRIMARY KEY` | `INTEGER PRIMARY KEY AUTOINCREMENT`; preserve IDs and PostgreSQL sequence position. |
| `BIGINT` | `INTEGER`, decoded as `i64`. |
| `DOUBLE PRECISION` | `REAL`; reject NaN/infinity during export. |
| `DATE` | validated `YYYY-MM-DD` `TEXT` for storage; shared filters still control display. |
| timestamp strings | canonical UTC RFC3339 with milliseconds in `TEXT`. |
| `BOOLEAN` | `INTEGER CHECK (value IN (0,1))`. |
| `JSONB` | canonical JSON `TEXT`, parsed/validated in Rust. |
| `$1`, `$2`, casts | `?1`, `?2`; remove PostgreSQL casts. |
| `ILIKE`/`btrim` | normalized `lower(...) LIKE`/`trim()`. |
| `ANY(array)` | safely generated parameterized `IN` list. |
| `LEFT JOIN LATERAL` | CTE/window function or join on selected key. |
| `TO_CHAR(NOW()...)` | one app-generated UTC value or `strftime`. |
| `ON CONFLICT`, `RETURNING` | Retain; both are supported, but handle zero-row conflicts. |

Historical `CREATE SCHEMA`, `DO $$`, `information_schema`, column-type repairs,
and legacy-table moves do not belong in Turso migrations. The exporter requires
an already-upgraded PostgreSQL source and fails if legacy schema remnants exist.

### Transaction corrections required during the port

- Load TMO/provider data before opening a write transaction. If the measured
  import fits the remote transaction/function budget, replace imported payments,
  events, links, and normalization state through one short libSQL transaction.
  If it does not, write generation-tagged staging rows in bounded sync phases,
  then
  atomically flip an active-generation pointer and clean the old generation
  later. Readers must never observe a half-imported generation. In either branch,
  remove the current mix of pool and transaction calls, which can lock SQLite and
  is not atomic today.
- Make primary-account switching and related settings changes atomic.
- Make stream plus schedule/view/projection changes atomic.
- Make view-row and membership replacement atomic.
- Retain the existing atomic payment, photo, and featured-photo replacement
  behavior using one libSQL transaction.
- Fix attachment `ON CONFLICT DO NOTHING RETURNING id`: a conflict returns no
  row. Use `DO UPDATE ... RETURNING id` or select the existing ID.

## Authentication and deployment perimeter

The existing production guidance says not to expose the dashboard publicly. A
Vercel move changes that network topology, so the first deployment must preserve
an equivalent outer gate rather than relying only on the login form.

Vercel's current plan boundary matters here. Pro supports the required cron
frequency, but Standard Protection still leaves the production domain public.
Protecting the production domain with Vercel Authentication requires Enterprise
or the Advanced Deployment Protection add-on for Pro (currently listed at
$150/month). This cost must not be hidden inside the word “Vercel.”

Topology that preserves the current outer gate:

1. A **Vercel Pro** project for cron, plus Advanced Deployment Protection with
   All Deployments enabled for production and previews.
2. Existing application login behind that perimeter, retaining a revocable
   server-side session in Turso.
3. A Vercel automation-bypass query secret only on the Resend webhook URL; the
   webhook independently requires valid Svix signature/timestamp verification.

The app stores only `user_id` in its current server session. Preserve the current
logout/revocation behavior with `tower-sessions` and an `app_session` table in
Turso (`id`, serialized data, expiry), using a small store implementation over
the shared libSQL wrapper. Pin and compatibility-test the available
`tower-sessions-libsql-store` first, but do not adopt its current Beta release if
error paths can panic or it cannot share the configured connection behavior; the
internal `SessionStore` implementation is only the trait's create/save/load/delete
surface and must reuse the production error handling and tests.

Use a seven-day expiry, opaque high-entropy cookie ID, `HttpOnly`, `Secure`, and
an appropriate strict same-site policy. A store error returns a typed service
failure, not a false “logged out” response. Expired rows are deleted inline by
the authenticated cron endpoint, never a process-local task. Do not migrate
PostgreSQL sessions; one login after cutover is intentional. Login creates the Turso row and
logout deletes it before clearing the cookie, so a copied old token is no longer
valid. Never use `MemoryStore` on Vercel.

Do not rely on `nextrs-app/app/middleware.rs` as a global security boundary.
NextRS 0.3.6 renders unmatched/not-found paths without running per-directory
target app middleware. Wrap the generated router in the same outer Axum/Tower
middleware in both `nextrs-app/src/main.rs` and `nextrs-app/api/index.rs`, so
auth, security headers, and request IDs also cover 404s. The outer layer uses a
fail-closed allowlist for login assets, health, webhook, and cron routes. Before
public cutover it must also add:

- CSRF tokens or strict same-origin enforcement on browser mutations.
- Origin/content-type validation for JSON mutations.
- Distributed login rate limiting and uniform authentication errors. Use a
  tested Vercel Firewall rule where the chosen plan supports it, otherwise a
  short-lived Turso-backed attempt bucket keyed by privacy-preserving client and
  account hashes; never use an isolate-local counter.
- Security headers and request IDs.
- Mandatory secure-cookie behavior; do not infer it only from build mode.
- Authorization/ownership checks on event and stream mutations.
- No raw unescaped sync error interpolation into HTML.

If the Advanced Protection cost is disproportionate, the honest alternative is
Vercel Pro with a public network endpoint protected by hardened application auth;
that is a deliberate relaxation of the old Tailscale-only posture and requires
owner approval. Standard Protection alone is not an equivalent outer gate.

A two-project topology—a protected dashboard project plus a tiny public webhook
ingress project—avoids sharing a Vercel bypass token with Resend but does not
remove the paid-production-protection requirement for the dashboard. It adds
deployment and observability overhead, so use it only if sharing an automation
bypass URL is unacceptable.

### Diagrams to keep next to implementation

Add compact maintained ASCII comments where the behavior is otherwise easy to
misread:

- The TMO sync coordinator: stale-row interruption -> atomic `sync_log` claim ->
  inline provider calls -> conditional completion, with provider calls outside
  the short persistence transaction.
- `nextrs-app/tools/pg-to-turso/`: snapshot -> conversion -> local validation ->
  remote import/validation pipeline and the hard abort points.

Do not copy the high-level deployment diagram into every module. These comments
explain local invariants that a future edit could actually violate.

## Inline sync and integration execution

Current forever loops, process-local sync status, and post-response
`tokio::spawn` calls cannot provide reliable work on Vercel. The minimum safe
replacement is not a general-purpose execution subsystem: use the existing
`sync_log` as the durable execution record and one-running guard, and run
provider work inline in the authenticated manual or cron invocation. NextRS
0.3.6's Vercel adapter also
discards Vercel `AppState`, so `waitUntil` is not an escape hatch for
post-response work.

```text
manual POST or deterministic cron slot
                  |
                  v
 mark stale running rows interrupted
                  |
                  v
 atomic INSERT ... RETURNING into sync_log
                  |
          +-------+-------+
          |               |
      no row          claimed row
  already covered/        |
  already running         v
                    execute inline
                          |
                          v
                conditional success/error
```

The canonical target schema extends the current log rather than introducing a
second execution table:

```sql
CREATE TABLE sync_log (
  id                 INTEGER PRIMARY KEY,
  connection_slug    TEXT NOT NULL,
  scheduled_for      TEXT,
  started_at         TEXT NOT NULL,
  finished_at        TEXT,
  status             TEXT NOT NULL
    CHECK (status IN ('running', 'success', 'error')),
  error_message      TEXT,
  endpoints_hit      TEXT,
  events_upserted    INTEGER NOT NULL DEFAULT 0,
  loans_upserted     INTEGER NOT NULL DEFAULT 0,
  snapshots_created  INTEGER NOT NULL DEFAULT 0,
  CHECK (
    (status = 'running' AND finished_at IS NULL) OR
    (status IN ('success', 'error') AND finished_at IS NOT NULL)
  )
);

CREATE UNIQUE INDEX sync_log_one_running_per_connection
  ON sync_log(connection_slug)
  WHERE status = 'running';

CREATE UNIQUE INDEX sync_log_one_scheduled_slot
  ON sync_log(connection_slug, scheduled_for)
  WHERE scheduled_for IS NOT NULL;

CREATE INDEX sync_log_connection_started
  ON sync_log(connection_slug, started_at DESC);
```

All timestamps are app-generated, fixed-width UTC RFC3339 strings;
`scheduled_for IS NULL` identifies a manual or legacy run. Before any claim,
mark a run left behind by a terminated invocation:

```sql
UPDATE sync_log
SET status = 'error',
    finished_at = ?1,
    error_message = 'execution exceeded platform maximum duration'
WHERE status = 'running'
  AND started_at < ?2;
```

The stale age used to derive `?2` must be strictly greater than the configured
Vercel hard execution limit plus clock skew. This hard-cutoff transition is safe
only because Vercel cannot still be running the old invocation after that bound.

Claim a manual run with one atomic statement:

```sql
INSERT INTO sync_log (
  connection_slug, scheduled_for, started_at, status
)
VALUES (?1, NULL, ?2, 'running')
ON CONFLICT DO NOTHING
RETURNING id;
```

No returned row means the connection already has a running sync; return `409`
with that durable run ID. Otherwise execute the complete sync before returning
the HTTP response. The status endpoint reads `sync_log`, never an isolate-local
mutex.

The cron endpoint computes the deterministic most-recent due slot for each
enabled TMO connection and collapses missed intervals into one full refresh. Its
claim also treats a successful manual run after the slot as coverage:

```sql
INSERT INTO sync_log (
  connection_slug, scheduled_for, started_at, status
)
SELECT ?1, ?2, ?3, 'running'
WHERE NOT EXISTS (
  SELECT 1
  FROM sync_log
  WHERE connection_slug = ?1
    AND status = 'success'
    AND started_at >= ?2
)
ON CONFLICT DO NOTHING
RETURNING id;
```

The partial one-running index prevents overlap, while the scheduled-slot index
makes duplicate cron delivery a no-op even after completion. A failed scheduled
slot waits for the next slot or an explicit manual run; automatic retry/backoff
is not part of the current product behavior.

Complete with a conditional update so an unexpected state transition is visible:

```sql
UPDATE sync_log
SET status = 'success',
    finished_at = ?2,
    endpoints_hit = ?3,
    events_upserted = ?4,
    loans_upserted = ?5,
    snapshots_created = ?6
WHERE id = ?1 AND status = 'running'
RETURNING id;
```

Use the analogous `error` update and treat zero returned rows as a coordination
fault. The authenticated cron invocation performs stale-row interruption,
scheduled-slot claim, at most one inline TMO sync total, and expired session
deletion before it returns. Another due connection waits for the next cron
invocation. Monarch's existing user-triggered balance refresh remains a direct
inline request.

Do not add an intermediate pending state, attempt counters, run-after timestamps,
ownership tokens, heartbeats, fencing, a renewable lease, or a generic
payload/kind table for the first port. First measure a full TMO sync in Phase 1
and configure its function duration. If it cannot reliably finish inside the hard
limit, stop the port at that gate and design bounded sync phases or a long-running
external runner from evidence; additional dispatch machinery would not make
overlong work fit a Vercel invocation. Heartbeat/fencing/renewal semantics become
justified only if execution can remain alive past the stale cutoff or later
requirements introduce resumable multi-consumer work.

Resend uses its existing durable domain rows rather than a generic webhook or
execution table:

1. Require Svix signing configuration in production.
2. Capture the untouched request bytes and required Svix headers, then verify
   signature and timestamp **before** parsing or reserializing JSON.
3. In one short transaction, insert or load the uniquely constrained
   `intg_received_email` row by `resend_email_id`, persist the raw payload, and
   upsert its `intg_received_email_attachment` metadata.
4. Fetch the body and attachments inline in that same HTTP invocation, outside
   the database transaction. Use deterministic object keys and idempotent row
   updates so replay after a partial S3/database failure is safe.
5. Mark the email `stored` and return 200 only after processing succeeds. On
   failure, mark it `error` and return a retryable non-2xx; a Resend retry loads
   the existing row and re-enters the same idempotent processing path. A duplicate
   whose state is already `stored` returns a safe 200.
6. The authenticated UI retry endpoint invokes the same processing function
   inline and returns only after `stored` or `error`; it does not spawn work.

This preserves the received-email persistence/dedupe already present in the app
without adding another webhook table or a separate dispatch mechanism. Measure
the largest representative email in Phase 1 alongside the TMO sync.

## Media and uploads

Vercel Functions cap request and response bodies at 4.5 MB, while the current app
buffers uploads up to 25 MiB and may silently fall back to local disk. Production
must require a complete S3/R2 configuration and fail startup/config validation if
it is partial.

- Browser requests a short-lived, content-type/size-constrained presigned PUT.
- Browser uploads directly to S3/R2.
- Browser calls a finalize API that validates object metadata and commits the DB
  row.
- Authenticated media reads return a short-lived signed redirect instead of
  proxying bytes through the function.
- Local filesystem storage remains an explicit development-only adapter.
- Existing object keys and encrypted provider credentials are preserved exactly
  during data migration.

Phase 0 must inventory the **actual** production backend and local media
directory, not infer it from environment-variable names. If any referenced bytes
exist only on local disk, add a one-shot local-to-S3/R2 backfill before the app
port: map each path to its canonical object key, upload with a stored SHA-256,
verify by HEAD plus downloaded hash, reconcile DB references, and emit
referenced/missing/orphan object manifests. A database-only migration is not
complete while a loan photo or email attachment still lives on the Coolify
filesystem. After backfill, make the legacy deployment require S3/R2 too, so it
cannot create fresh local-only files during the port. Re-run the object manifest
under the final Phase 7 freeze to catch any delta.

## Operational write gate and source freeze

“Read-only” must be code, not a sentence in the runbook. Add one centralized
operation-control record and guard used by both the legacy app and the NextRS
app. Its modes are `read_only` and `enabled`; scheduled sync execution has a
separate enabled flag. Every business-data mutation passes through it:

- browser/API/form `POST`/`PUT`/`PATCH`/`DELETE` handlers;
- upload intent/finalization and object-deleting operations;
- manual/scheduled sync claim and inline execution;
- Resend ingestion;
- the intentionally mutating cron GET and its stale-row, claim, sync, and
  cleanup paths.

Login may remain available for read-only smoke, but anything that changes
canonical application/provider data returns a clear maintenance 503. Automation
and webhook responses include `Retry-After` and are not acknowledged. Tests keep
an explicit allowlist so a newly added mutation cannot bypass the gate.

Session rows, the operation-control row, and rolled-back health canaries are
classified as disposable operational state. They may be created or changed
during the read-only soak and are discarded if traffic returns to PostgreSQL.
The irreversible rollback boundary is the first committed **canonical
business/provider write or external side effect**, not a login session or the
operator's control flip.

Provide a small authenticated/out-of-band `trust-deeds ops write-mode` command
that reads and atomically changes the durable control row. The production import
starts in `read_only`; the Phase 7 go/no-go command flips it to `enabled` without
depending on an individual warm Vercel instance or a new code build. Disabling it
is the first incident action.

The legacy freeze is concrete:

1. Set its durable operation control to `read_only` and scheduler disabled.
2. All existing detached/sync work must register an active operation before it
   starts and clear it after its last database write.
3. Wait until the active-operation set is empty and no new operation can claim.
4. Stop the old app process before opening the final PostgreSQL snapshot. Leave
   a proxy/static maintenance response in front; the old Resend path returns a
   retryable failure.
5. Confirm the source database receives no writes while export/import/validation
   runs.

Phase 0 adds this mechanism to the old app, and Phase 6 rehearses it. Relying on
“wait a bit and hope the scheduler is done” is not an acceptable snapshot lock.

## PostgreSQL-to-Turso migration tool

Create a standalone, one-shot tool under `nextrs-app/tools/pg-to-turso/` with its
own manifest. Keep it outside the deployed target package's dependency graph so
SQLx/PostgreSQL does not enter the Vercel artifact; do not add either package to
a root Cargo workspace.

```text
upgraded PostgreSQL
  |
  | REPEATABLE READ, READ ONLY snapshot
  v
typed exporter
  |
  | explicit IDs/columns + validated conversions
  v
fresh local SQLite file using production migrations
  |
  | integrity/FK/count/hash/domain validation
  v
Turso import into a new database
  |
  | repeat validation via remote libSQL
  v
Vercel preview / production
```

The exporter must:

1. Inventory every source relation, including the PostgreSQL session-store table,
   and classify it in the manifest as mapped/transformed, target-only, or
   intentionally discarded. PostgreSQL sessions are explicitly discarded and
   `app_session` starts empty; only mapped tables require count/digest parity.
2. Require a current source schema and fail on legacy public TMO tables,
   `scheduled_date`, or other known remnants.
3. Read all mapped tables from one repeatable-read, read-only transaction.
4. Create a fresh local database with the exact target migrations and foreign
   keys enabled.
5. Load explicit typed rows in dependency order while preserving all IDs.
6. Convert dates, timestamps, bools, numbers, and JSON deliberately. Reject
   invalid dates, invalid JSON, NaN/infinity, overflow, and lossy conversions.
7. Preserve password hashes, encrypted credentials/nonces, raw provider payloads,
   storage keys, and source identifiers byte-for-byte.
8. Use the same `APP_ENCRYPTION_KEY` in Vercel and decrypt both TMO and Monarch
   credentials as a migration canary.
9. Export each PostgreSQL sequence's effective next value, set the corresponding
   `sqlite_sequence.seq` to one less than that value (never below imported max),
   and verify the next rolled-back insert receives the expected ID.
10. Finish with a single-file, checkpointed SQLite artifact suitable for Turso
   import; do not leave an uncheckpointed WAL.
11. Emit a machine-readable validation manifest.

The SQLite export is sensitive production data even though credential fields are
ciphertext. Create it outside the repository on an encrypted local volume with
mode `0600`; manifests and logs contain only schema metadata, counts, key ranges,
and hashes, never row contents or secrets. Exclude the export directory from Git,
CI, Vercel uploads, shell history, and generic build artifacts. The cutover
runbook names its custodian and deletion deadline; after the retention window,
delete the artifact and destroy the temporary-volume key. Never upload the SQLite
file as a Vercel build artifact.

Turso imports a SQLite file, not PostgreSQL directly. Create a **new** staging or
production Turso database from the validated file instead of importing into an
already-mutating target.

### Required migration validation

For every mapped table, compare source and destination:

- row count;
- minimum/maximum primary key;
- BLAKE3 digest of canonical rows in primary-key order;
- unique/natural key duplicates;
- foreign-key/orphan checks;
- exact `f64` bits plus business totals for financial data.

For target-only tables, assert the expected bootstrap rows and emptiness; for
discarded tables, assert that no source row was silently treated as mapped.

Also run:

- `PRAGMA integrity_check` and `PRAGMA foreign_key_check` locally;
- remote `PRAGMA foreign_keys`, plus a rolled-back invalid-FK write proving it is
  enforced;
- type checks with SQLite `typeof()`;
- date/timestamp parsing over every stored value;
- next-ID tests against each source sequence's effective next value, including a
  sequence advanced beyond the current table maximum;
- credential decryption canaries;
- golden-output comparisons for forecast, integration overview, loan detail,
  inbox, sync logs, and current balance;
- one reversible settings mutation and event create/update/delete against remote
  Turso.

## Existing defects that affect the migration

Do not turn the framework port into an open-ended cleanup, but the following are
cutover blockers because the new driver or public deployment makes them unsafe:

- `get_recent_payments` selects fewer fields than `PaymentView` requires and
  swallows the row error into an empty result.
- Attachment conflict handling requires a `RETURNING` row after `DO NOTHING`.
- Database reads commonly turn errors into empty collections.
- Several multi-step account/stream/TMO writes are non-atomic.
- Background work can be lost after the response.
- Resend verification is optional and lacks replay/timestamp enforcement.
- Browser mutations lack CSRF protection and some mutation authorization and
  validation is too broad.
- Local media fallback and byte proxying are unsafe on Vercel.
- Inbox delete is referenced by templates without a mounted route; the parity
  matrix must decide whether to restore or remove it.
- Cash-balance persistence failure can return success.

Fix these in the owning port slice and add a regression test. Other behavior
changes discovered by characterization become separately approved follow-ups.

## Implementation phases

### Phase 0 — Freeze and characterize

1. Confirm `2ec7a87` plus its PostgreSQL-backed migration/behavior follow-ups as
   the named streams/forecast source baseline.
2. Capture the actual production PostgreSQL schema after boot migrations and
   compare it to the code-derived final state.
3. Check in the route contract matrix and representative HTML/JSON golden
   fixtures with secrets and personal data scrubbed.
4. Record current page/query counts, warm latency, sync durations, export volume,
   object counts/sizes, and maximum webhook/upload payloads.
5. Make database-backed CI tests mandatory. Current forecast tests silently skip
   when `TEST_DATABASE_URL` is absent; a skipped DB suite must fail CI.
6. Add and test the legacy operation-control/active-operation freeze path.
7. Inventory local versus S3/R2 media, reconcile every DB reference, and complete
   any checksum-verified local-object backfill.
8. Document the current deploy, env vars, secrets, webhook endpoint, scheduler,
   and DNS so cutover/rollback is executable by someone other than the author.

**Exit:** one frozen behavioral/schema baseline, no vacuous database tests, and
measured function-duration/size inputs, plus a proven source-write freeze and no
unaccounted local-only media.

### Phase 1 — Prove the risky seams

The first implementation slice is already present under `nextrs-app/`: an
isolated NextRS 0.3.6/Askama shell, shared local and Vercel entrypoints, generated
route registry, liveness route, disabled speculative prefetch, independent
lockfile/toolchain, and Vercel configuration. Continue the risky-seam spikes in
that package before the bulk port:

1. Deploy the implemented NextRS Rust page and liveness route through the Vercel
   adapter, then add one representative JSON route under `nextrs-app/app/`.
2. Warm/cold remote Turso read, transaction, foreign-key enforcement, and schema
   version check from the chosen Vercel region.
3. Revocable Turso-backed login session through the outer Axum/Tower middleware,
   including protected/public, logout replay, store-failure, and unmatched-path
   tests.
4. The `sync_log` partial-unique claim, scheduled-slot dedupe, stale interruption,
   and verified inline Resend path under concurrent real Vercel invocations.
   Prove cron reaches the chosen protected-production topology and validate
   `Authorization: Bearer $CRON_SECRET` with constant-time comparison.
5. Direct presigned object upload above 4.5 MB and signed media read redirect.
6. Measure a representative/full TMO sync, a large representative inbound email,
   and the release artifact size.

Delete spike-only code or promote it behind production tests. Resolve function
duration, build, region, or auth incompatibilities before converting 233 queries.

Run target checks without involving the root package:

```sh
cargo fmt --manifest-path nextrs-app/Cargo.toml --check
cargo check --manifest-path nextrs-app/Cargo.toml --locked --all-targets
cargo test --manifest-path nextrs-app/Cargo.toml --locked
cargo build --manifest-path nextrs-app/Cargo.toml --release --locked --bin index
```

For Vercel, set Root Directory to `nextrs-app`; its build command remains
`cargo build --release --locked --bin index` because it executes inside that
directory.

**Exit:** deployed preview evidence for every serverless-specific seam, a
go/no-go decision on the Beta Rust/NextRS deployment path, and evidence that the
inline TMO/Resend paths fit the configured hard duration or require a separately
designed bounded/external execution path.

### Phase 2 — Establish the NextRS shell

1. Retain the implemented exact NextRS/Vercel pins, Cargo `index` bin,
   `nextrs-app/build.rs`, generated registry, `nextrs-app/api/index.rs`,
   `nextrs-app/rust-toolchain.toml`, `nextrs-app/vercel.json`, and
   `nextrs-app/.cargo/config.toml`. Do not duplicate any of them at repository
   root.
2. Add immutable runtime-handle construction, config validation, request IDs,
   error mapping, and structured logs under `nextrs-app/src/`; layer disposable
   handles with Axum `Extension` in both entrypoints.
3. Copy static assets into `nextrs-app/public/static/`, wire the target-local
   Tailwind/DaisyUI source build into Vercel/CI, and preserve URLs. Keep the root
   assets and build unchanged for the legacy app.
4. Add the outer request-ID/security middleware framework around the generated
   router in both entrypoints. Keep the proven Phase 1 auth spike ready for
   integration after the canonical DB/session tables land in Phase 3.
5. Port health/readiness and prove one representative Askama GET/rendering path
   with a fixture-backed view model. Database connectivity remains a separate
   Phase 1 probe until the canonical repositories land in Phase 3.
6. Keep the repository-root Axum binary/router independently buildable as the
   source/comparison application while slices move. Do not create a Cargo
   workspace, a root `build.rs`, a path dependency between packages, or a
   permanent dual-framework abstraction.

**Exit:** clean local and Vercel builds, chosen perimeter behavior, built static
assets, global headers/request IDs (including 404s), health/readiness, and one
production-shaped HTML rendering path.

### Phase 3 — Build canonical data and migration layers

1. Write clean versioned libSQL migrations under `nextrs-app/migrations/` from
   the final live schema, including the canonical `sync_log` claim/dedupe indexes
   and flattened `intg_*` tables.
2. Add the connection wrapper, checked row mappers, transactions, schema-version
   gate, and local test database factory under `nextrs-app/src/db/`.
3. Add `app_session`, operation control, the libSQL session store, outer auth
   enforcement, login/logout, and the centralized write guard before any
   canonical mutation route is ported.
4. Port repositories by domain: users/settings/accounts, streams/events/views,
   forecasts/snapshots, integrations/TMO, workspaces/media, email/inbox, logs.
5. Port direct route SQL behind repositories; add and enforce
   `nextrs-app/tools/check_intg_boundary.sh`, and assert target migrations
   contain neither PostgreSQL schema qualifiers nor `ATTACH`.
6. Add local integration tests for every repository and transaction boundary.
7. Build the standalone exporter and validation manifest under
   `nextrs-app/tools/pg-to-turso/`, outside the deployed package graph.
8. Rehearse import into staging Turso and compare golden domain outputs.

**Exit:** no deployed SQLx/PostgreSQL dependency, all repository tests run on the
production migration set, a real read-only page is backed by staging Turso, and
the staging import is repeatable with zero validation errors.

### Phase 4 — Port behavior in vertical slices

Port in this order so each slice can be reviewed and browser-tested:

1. Dashboard/read-only summaries and shared display filters.
2. Streams, events, views, and forecast/cash behavior.
3. Integration overview, credentials, TMO/Monarch reads and mutations.
4. Loan workspace, photos, and media.
5. Inbox, received email, attachments, and payment links.
6. Canvas and remaining JSON/form/HTMX surfaces.
7. Legacy aliases, diagnostics, and explicit removal decisions.

For each slice: map old routes, port repository calls into `nextrs-app/`,
preserve display filters, run contract tests, run desktop/mobile browser flows,
compare screenshots, and inject database/provider errors. Keep the legacy root
surface available as the source/comparison implementation until that slice's
explicit migration boundary; do not silently rewrite root files merely to share
target scaffolding.

**Exit:** every retained route is served by NextRS and every contract-matrix row
is green or has an owner-approved change.

### Phase 5 — Make operations serverless-safe

1. Replace scheduler and detached sync with atomic `sync_log` claims and inline
   manual/cron execution.
2. Replace process-local sync status with durable log reads, the one-running
   partial index, scheduled-slot dedupe, conditional completion, and hard-limit
   stale interruption.
3. Make Resend ingestion verified, persisted/deduplicated in the existing email
   tables, processed inline, and synchronously replayable.
4. Move all production uploads/read delivery to direct object-storage flows.
5. Add interrupted/error sync visibility plus received-email error visibility
   and operator retry controls.
6. Add the chosen Vercel protection posture, cron secret, staging/prod Turso
   separation, and env validation.

**Exit:** termination/overlap tests prove no two syncs run for one connection,
stale executions become visible errors after the hard bound, and Resend never
receives a success response before its idempotent processing completes.

### Phase 6 — Rehearse cutover

1. Start from a clean checkout, set the Vercel Root Directory to `nextrs-app`,
   and run `cargo build --release --locked --bin index` from that directory
   exactly as Vercel will. Separately run the legacy root-package checks to prove
   the isolated target has not changed its build.
2. Run at least two timed PostgreSQL exports into fresh staging Turso databases.
3. Deploy Vercel previews against staging only; never point previews at prod.
4. Run the full unit, repository, HTTP, browser, sync-execution, webhook, and
   migration suites.
5. Rehearse maintenance mode, scheduler drain, webhook retry/replay, DNS/project
   promotion, read-only smoke, write enablement, and pre-write rollback.
6. Configure the initial Turso backup/PITR or export policy and S3/R2 versioning,
   then restore both database and representative objects into a fresh test
   environment. Record and accept the recovery-point objective; Turso currently
   documents a possible PITR gap of up to 15 seconds. Do this before Turso can
   become the only current writable copy.
7. Write the command-by-command runbook with named operator, expected output,
   abort thresholds, and secret locations.

**Exit:** two consecutive clean rehearsals inside the accepted maintenance
window, with identical manifests and a demonstrated rollback before writes.

### Phase 7 — Production cutover

1. Announce and enter maintenance/read-only mode on the old app.
2. Disable manual mutations and the old scheduler; wait for active sync work to
   finish.
3. Have the old Resend endpoint return a retryable non-2xx during the final
   snapshot window; record the event range for replay.
4. Take the final repeatable-read export, validate locally, create a fresh
   production Turso database from it, and validate remotely.
5. Deploy Vercel production with the chosen protection posture, unchanged
   encryption/storage/provider secrets, and the new Turso/session/cron secrets.
6. Keep the new app read-only and run smoke tests and credential canaries.
7. Flip the custom domain/traffic, but leave browser mutations, cron, and the new
   Resend endpoint disabled.
8. Verify login, critical reads, object redirects, health, and financial totals.
   This is the final point where an immediate traffic rollback is safe.
9. Hold an explicit go/no-go. Crossing it enables browser mutations, switches
   Resend to the new URL (with a bypass token if required), and enables cron.
   PostgreSQL becomes stale after the first canonical business/provider write or
   external side effect; disposable session/control state does not count.
10. Send/replay a controlled Resend event, run one controlled inline sync, verify
    deduplication/overlap prevention, then monitor error rate, database latency,
    query counts, running/interrupted sync age, received-email errors, and core
    financial totals through the observation window.

**Exit:** stable production, no unexplained validation drift, no stale running
sync or unprocessed received email, and an explicitly closed rollback window.

### Phase 8 — Stabilize, document, then consider React

1. Keep PostgreSQL read-only for the agreed retention period.
2. Remove old Coolify/scheduler/runtime wiring only after production sign-off.
3. Update `AGENTS.md`, `CLAUDE.md`, `docs/data-model.md`, manifest/vision/deploy
   docs, and remove or replace stale `schema.sql`.
4. Schedule and monitor recurring restore drills for the Turso and S3/R2 backup
   policy proven before cutover.
5. Upgrade NextRS only as a separate tested change.
6. If desired, start React/TSX with a read-only page, then integrations,
   streams, forecast, canvas, and inbox. Preserve APIs and migrate page by page.

## Cutover and rollback boundary

```text
old app writable
      |
      v
maintenance + drain
      |
      v
snapshot -> import -> validate
      |
      v
new app READ ONLY ---- failure ----> traffic back to old app (safe)
      |
      v
enable new writes
      |
      +---- failure ----> fix forward, or stop and deliberately replay deltas
                          PostgreSQL is now stale; blind traffic flip is unsafe
```

Do not promise an instant rollback after Turso accepts writes. The preferred
strategy is a conservative read-only soak followed by fix-forward. If the owner
requires reversible writes during the observation window, add a durable,
ordered `cutover_change_log` for every app mutation and build/test the reverse
replayer before cutover. Provider data can often be resynced and Resend events
replayed, but user edits and deletes cannot be reconstructed without such a log.

## Test strategy

```text
                         production cutover
                                ^
                                |
                     Vercel preview smoke/load
                                ^
                                |
             browser journeys + screenshot comparisons
                                ^
                                |
          HTTP route contracts + auth/error/security matrix
                                ^
                                |
       remote Turso critical suite + migration reconciliation
                                ^
                                |
        local libSQL repositories/transactions/migrations
                                ^
                                |
       pure forecast/schedule/sync/security/format unit tests
```

Required gates:

- Unit: forecast parity, schedule edge cases, deterministic due-slot calculation,
  sync idempotency, auth cookie tamper/expiry, CSRF/origin policy, display
  filters, and row conversion boundaries.
- Local libSQL: every repository, migration from empty/current versions,
  constraints, transactions, concurrent `sync_log` claims, scheduled-slot
  dedupe, stale interruption, and conditional completion.
- Remote Turso: critical reads/writes, foreign-key enforcement, transaction
  behavior, parallel sync claims, latency/query instrumentation, and credential
  decrypt.
- HTTP: all old methods/paths/statuses/headers/content types/JSON shapes, auth
  redirects, invalid input, provider failure, and DB failure. GET side effects are
  explicitly forbidden before prefetch is enabled.
- Browser: desktop/mobile login; dashboard; stream/event CRUD; forecast scrubber
  and cash updates; integrations/sync status; workspace/photos; inbox/attachments;
  canvas; error and empty states.
- Sync/webhooks: duplicate cron, simultaneous manual/cron claims, invocation
  termination and hard-cutoff interruption, provider timeout, failed conditional
  completion, duplicate/replayed Svix events, partial object writes, and inline
  operator retry.
- Deployment: clean-clone Vercel build, cold/warm requests, static cache headers,
  4.5 MB boundary, large direct upload, object redirect, protected preview, cron
  authorization, and production-like configuration validation.
- Migration: manifest equality and golden domain output on two independent
  rehearsals and again during production cutover.

Set route-specific warm-latency and query-round-trip budgets from the Phase 0
baseline. A mechanical port that adds dozens of serial remote queries is not
accepted merely because the HTML is correct.

## Failure-mode review

| Failure | Expected behavior | Required proof |
|---|---|---|
| Turso unavailable/slow | Bounded timeout; typed 503; no empty-success UI; mutations not acknowledged. | Fault-injection HTTP tests. |
| Session-store read fails | Typed 503, not a false logout/login loop; no request proceeds unauthenticated. | Auth store fault-injection test. |
| Cold initialization fails | Error is not cached forever; later request can retry. | Initialization retry test. |
| Schema version mismatch | Readiness fails and app refuses writes. | Old/new schema deploy tests. |
| Foreign keys off | Connection rejected before use. | Per-connection invalid-FK canary. |
| Concurrent cron/manual sync calls | The partial unique index permits one `running` row per connection; a duplicate slot or active run is harmless. | Parallel remote claim test. |
| Invocation dies during sync | The row remains `running` until a later request, after the Vercel hard-limit cutoff, marks it `error`; domain writes remain idempotent. | Terminate-and-interrupt test using the configured cutoff. |
| TMO sync exceeds function budget | Phase 1 blocks the inline design; explicitly design bounded phases or an external runner, with no long transaction. | Timed production-shaped rehearsal. |
| Resend retries/replays | The unique received-email row and deterministic object keys make replay idempotent; only `stored` gets 200, while failure stays visible and returns non-2xx. | Duplicate/replay/partial-write suite. |
| Webhook bypass URL leaks | App auth still gates other routes; rotate bypass; webhook still requires Svix. | Protection/auth security test and runbook. |
| Unmatched path skips NextRS app middleware | Outer Axum/Tower layer still applies auth, request ID, and security headers to the 404. | Authenticated/anonymous 404 tests in both entrypoints. |
| Read-only gate misses a mutation | CI fails its mutation inventory; production returns 503 before any side effect. | Route/sync/webhook gate matrix. |
| Object store misconfigured | Production config fails closed; never writes local disk. | Config matrix test. |
| Legacy local media was missed | Cutover blocks on referenced/missing/orphan manifest or checksum mismatch. | Object migration manifest + sampled/full hash verification. |
| Tailwind bundle is stale/missing | Clean build fails or browser smoke catches missing classes; never rely on a developer's generated file. | Clean-clone CSS build and asset hash/screenshot test. |
| Oversized upload | Direct upload succeeds; function route rejects unsupported body early. | >4.5 MB browser test. |
| Bad row/driver decode | Visible/logged typed error, not an empty list. | Corrupt fixture tests. |
| Import conversion drift | Export aborts before Turso creation; manifest identifies row/column. | Invalid fixture tests. |
| Cutover fails before writes | Return traffic to unchanged PostgreSQL app. | Rehearsed rollback. |
| Cutover fails after writes | Stop writes and fix forward or run tested delta replay. | Owner-approved runbook; optional reverse-replay drill. |
| Turso-only data must be restored | Restore into a fresh database and repoint a protected test deployment before production writes are allowed. | Phase 6 database/object restore drill. |
| NextRS route generation changes | Exact pin prevents surprise; upgrade branch must pass contract suite. | Lockfile and clean-build gate. |

## Parallel work plan

Use separate worktrees with narrow file ownership after Phase 1 proves the shared
foundation:

| Lane | Owns | Depends on |
|---|---|---|
| Framework/deploy | `nextrs-app/{Cargo.toml,build.rs,api/,src/lib.rs,src/main.rs,vercel.json,public/}` | Phase 1 spikes |
| Data | `nextrs-app/src/db/`, `nextrs-app/migrations/`, `nextrs-app/tools/pg-to-turso/` | frozen source schema |
| Routes/UI | `nextrs-app/app/`, `nextrs-app/templates/`, target HTTP contract fixtures | framework shell + repository interfaces |
| Operations | `nextrs-app/src/services/`, cron/webhook routes, target object-storage adapters | `sync_log`/email repositories + deploy shell |
| QA/cutover | target browser suites, migration manifests, observability, runbooks | stable vertical slices |

The framework shell is isolated under `nextrs-app/`; land its database interfaces
next. Thereafter, vertical
slices should own their route, repository, and tests together to avoid a large
integration branch. Keep generated registry/client output out of manual merges;
regenerate it from `nextrs-app/app/` in target CI/deploy. Parallel lanes must not
add target files to the root Cargo package or convert the repository to a Cargo
workspace.

## Owner decision checkpoints

These recommendations are not blockers to beginning Phase 0, but they must be
confirmed before Phase 2 or production provisioning:

1. **Framework interpretation:** proceed with NextRS/Rust, not TypeScript/Next.js.
   **Recommendation: NextRS/Rust.**
2. **Frontend scope:** preserve Askama/HTMX for the cutover, then modernize
   selectively. **Recommendation: staged HTML-first port.**
3. **Perimeter and cost:** retain the current outer gate with Pro + Advanced
   Deployment Protection, or accept a public production endpoint secured by
   hardened app auth. **Recommendation: preserve the outer gate unless the owner
   explicitly accepts the posture change after seeing the current $150/month
   add-on cost; use one project plus the Resend bypass URL if protection is kept.**
4. **Execution budget:** inline Vercel execution versus a separately designed
   bounded or external runner. **Recommendation: use the minimal inline
   `sync_log` design when Phase 1 proves it fits the hard duration; split or
   externalize only if measurement requires it.**
5. **Post-write rollback:** accept a fix-forward boundary after writes or fund a
   reverse mutation journal/replayer. **Recommendation: read-only soak plus
   fix-forward for this personal app.**
6. **Backup RPO:** accept Turso's documented possible PITR gap of up to 15 seconds
   or add an independent ordered mutation journal. **Recommendation: accept and
   document the PITR RPO for the first port, with Resend replay/provider resync
   and frequent encrypted exports; fund zero-loss journaling only if required.**

## Verified platform references

- Turso Rust SDK: [quickstart](https://docs.turso.tech/sdk/rust/quickstart) and
  [API reference](https://docs.turso.tech/sdk/rust/reference).
- Turso import path and SQLite-file requirement:
  [Migrate to Turso](https://docs.turso.tech/cloud/migrate-to-turso).
- Turso recovery behavior:
  [Point-in-Time Recovery](https://docs.turso.tech/features/point-in-time-recovery).
- Turso/SQLite behavior:
  [cloud limitations](https://docs.turso.tech/cloud/limitations),
  [PRAGMAs](https://docs.turso.tech/sql-reference/pragmas), and
  [data types](https://docs.turso.tech/sql-reference/data-types).
- Vercel Rust Functions:
  [Rust runtime](https://vercel.com/docs/functions/runtimes/rust).
- Vercel request/response size and direct-upload guidance:
  [function limits](https://vercel.com/docs/functions/limitations) and
  [bypassing the body-size limit](https://vercel.com/kb/guide/how-to-bypass-vercel-body-size-limit-serverless-functions).
- Vercel Cron behavior and plan limits:
  [Cron Jobs](https://vercel.com/docs/cron-jobs),
  [management/security](https://vercel.com/docs/cron-jobs/manage-cron-jobs), and
  [usage/pricing](https://vercel.com/docs/cron-jobs/usage-and-pricing).
- Vercel access controls:
  [Deployment Protection](https://vercel.com/docs/deployment-protection) and
  [automation bypass](https://vercel.com/docs/deployment-protection/methods-to-bypass-deployment-protection/protection-bypass-automation).
- Resend delivery semantics:
  [webhook retries and replays](https://resend.com/docs/webhooks/retries-and-replays).

## Engineering review conclusion

The port is feasible without a big-bang rewrite. The critical sequencing rule is
to separate **behavior preservation** from **platform replacement** and defer
React. The principal risks are not NextRS page rendering; they are remote-query
round trips, an authoritative PostgreSQL-to-SQLite conversion, Vercel's ephemeral
execution model, the 4.5 MB body limit, inline sync/webhook interruption, and
replacing the current Tailscale perimeter deliberately.

The fastest responsible route is: freeze the active schema work, prove NextRS +
remote Turso + auth/inline sync/uploads on Vercel, port the persistence layer and pages
in vertical slices, rehearse an atomic maintenance-window import twice, then cut
over with the explicitly chosen production-access posture. That keeps the work
bounded while producing a genuine NextRS implementation rather than merely
wrapping the old server.

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | — | — |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | — | — |
| Eng Review | `/plan-eng-review` | Architecture & tests (required) | 1 | ISSUES OPEN | 35 issues found and incorporated, 0 critical gaps; 6 owner decisions remain |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | — | — |
| DX Review | `/plan-devex-review` | Developer experience gaps | 0 | — | — |

- **ADVERSARIAL:** Internal adversarial plan review completed; all concrete
  implementation findings were incorporated.
- **UNRESOLVED:** 6 owner decisions, listed under “Owner decision checkpoints.”
- **VERDICT:** Engineering mechanics are ready; eng review remains open until the
  owner confirms the six scope/security/operations choices.
