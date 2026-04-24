#!/usr/bin/env bash
# Quarantine check: the app layer may not reference the intg schema or
# TMO-shaped columns/JSON keys. Integration code is allowlisted by path.
#
# Run from the repo root. Exits non-zero if any violation is found.
set -euo pipefail

cd "$(dirname "$0")/.."

# Patterns that count as a TMO / intg leak when seen outside the allowlist.
# - \bintg[._]   → intg.schema refs (e.g. `intg.tmo_import_loan`).
#                  Rust module paths like `db::integrations::...` do not match
#                  because the pattern requires `intg` followed by `.` or `_`.
# - check_number, is_pending_print_check → TMO payment-state keys that used
#   to live in stream_event.metadata JSON and must never resurface in
#   app-layer code.
#
# `loan_account` is NOT in the list — it is both a TMO API identifier and a
# column on `intg.received_email`, so it travels through app-layer code as an
# opaque string. The `\bintg[._]` pattern catches the schema-level leak that
# actually matters.
patterns=(
  '\bintg[._]'
  'check_number'
  'is_pending_print_check'
)

# Files/globs that ARE allowed to mention those patterns. Keep this list
# narrow — every entry is a known TMO / integration boundary.
allow=(
  '!src/tmo/**'
  '!src/monarch/**'
  '!src/resend.rs'
  '!src/routes/webhooks.rs'
  '!src/routes/integrations.rs'
  '!src/routes/health.rs'         # bench_render uses integration templates
  '!src/db/integrations.rs'
  '!src/db/emails.rs'             # Resend inbox lives in intg.received_email
  '!src/db/workspaces.rs'         # TMO loan workspace lives in intg.loan_workspace
  '!src/db/mod.rs'                # migrations create intg schema
  '!src/property_media.rs'        # TMO property photo enrichment
  '!src/scheduler.rs'             # doc comment references intg.integration_connection
  '!src/models/tmo.rs'
  '!src/bin/tmo_*.rs'
  '!src/bin/backfill_property_media.rs'  # TMO-only maintenance tool
)

rg_args=()
for glob in "${allow[@]}"; do
  rg_args+=(--glob "$glob")
done

violations=""
for pattern in "${patterns[@]}"; do
  found=$(rg --no-messages -n "$pattern" src/ "${rg_args[@]}" || true)
  if [ -n "$found" ]; then
    violations+="pattern '${pattern}':\n${found}\n\n"
  fi
done

if [ -n "$violations" ]; then
  echo "intg / TMO-shaped reference leak detected outside the integration layer:"
  echo
  printf '%b' "$violations"
  exit 1
fi

echo "ok — app layer is quarantined from intg / TMO-shaped references."
