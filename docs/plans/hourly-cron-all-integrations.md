# Hourly cron over all integrations (NextRS 0.6)

Status: implemented 2026-08-22 — pending operator deploy (wrangler + vercel) and DB health review

## Goal

Replace the single daily Vercel cron (which can only ever satisfy the `daily`
cadence) with an hourly trigger that reads each integration's stored
`sync_cadence`, decides what is due, runs it inline, and no-ops otherwise.
Adopt the NextRS 0.6 cron conventions rather than bespoke plumbing.

## Steps

1. **Upgrade nextrs 0.5 → 0.6** in both dependency tables; `cargo update`;
   fix any breakage; keep the `[patch.crates-io]` vendored `vercel_runtime`
   (0.6 still depends on `vercel_runtime = "2"`). Install `cargo-nextrs`.
   Keep our hashed `CronAuthenticator` — it already implements the framework
   contract (`Authorization: Bearer $CRON_SECRET`, fail-closed 401), which the
   generated Worker's deploy preflight checks for. `ApiError` adoption is
   deferred; it is a broad handler refactor with no behavior change.
2. **Generalize the scheduler.** `run_cron` currently prepares and runs only
   TMO. Iterate every configured integration connection (`tmo`, `monarch`),
   parse its cadence, compute the most recent deterministic slot, and attempt
   the unique-index-backed scheduled claim. Redelivery of an already-claimed
   slot stays a no-op, so an hourly trigger is safe for every cadence.
   Monarch gains a scheduled entry point (`claim_scheduled` is already
   slug-generic); its `as_of_date` is the slot's UTC calendar date.
3. **Declare schedules in `nextrs.toml`** (`app.name = finstream`,
   `app.url = https://finstream.hirschi.dev`, cron `0 * * * *` →
   `/internal/cron`, provider defaults to Cloudflare). Run
   `cargo nextrs cron generate`; commit `.nextrs/cloudflare/`. Keep the
   existing daily 07:00 UTC native Vercel cron as a backstop.
4. **Deploy** (operator): `cargo nextrs cron deploy` (wrangler login +
   `CRON_SECRET` in env), then the usual
   `vercel build --prod && vercel deploy --prebuilt --prod`.
5. **Background-job health review** — read `sync_log` outcome history from
   Turso (needs `turso auth login`) and report failure classes since July.
