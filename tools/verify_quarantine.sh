#!/usr/bin/env bash
# Quarantine acceptance test: rename the intg schema on a dev DB and verify
# that app-layer pages still render. Integration pages are expected to 500
# (or show empty states) while the schema is renamed.
#
# Requires `psql` and a DATABASE_URL pointing at a DEV database you can
# afford to break briefly. The script always restores the schema, even on
# failure.
set -euo pipefail

if [ -z "${DATABASE_URL:-}" ]; then
  echo "DATABASE_URL must be set (point at a dev database)." >&2
  exit 2
fi

restore() {
  psql "$DATABASE_URL" -q -c "ALTER SCHEMA intg_quarantined RENAME TO intg" 2>/dev/null \
    || echo "warning: could not restore intg schema — check manually" >&2
}
trap restore EXIT

echo "renaming intg → intg_quarantined …"
psql "$DATABASE_URL" -q -c "ALTER SCHEMA intg RENAME TO intg_quarantined"

echo
echo "schema renamed. Start the app with this DATABASE_URL and confirm:"
echo "  ✓ /, /forecast, /streams, /canvas, /inbox, /login still render"
echo "  ✗ /integrations/* may 500 or show empty state (expected)"
echo
echo "press ENTER when you've finished verifying; schema will be restored."
read -r _
