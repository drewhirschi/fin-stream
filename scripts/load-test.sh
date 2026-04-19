#!/usr/bin/env bash
#
# Ramping load test for the Trust Deeds app.
#
# Usage:
#   ./scripts/load-test.sh [endpoint] [base_url] [cookie]
#
# Examples:
#   ./scripts/load-test.sh /health                          # unauthenticated, direct
#   ./scripts/load-test.sh /health http://gory:8008         # through Traefik
#   ./scripts/load-test.sh /integrations/tmo "" "id=abc123" # authenticated route
#
# For authenticated routes, grab your session cookie from the browser:
#   1. Open the app in your browser, log in
#   2. DevTools → Application → Cookies → copy the "id" cookie value
#   3. Pass it as: ./scripts/load-test.sh /integrations/tmo "" "id=<value>"
#
# Prerequisites: oha (pacman -S oha), jq

set -euo pipefail

ENDPOINT="${1:-/health}"
BASE_URL="${2:-http://gory:3801}"
COOKIE="${3:-}"
URL="${BASE_URL}${ENDPOINT}"
DURATION="10s"

# Rate tiers — pushing to find the real ceiling
TIERS=(50000 100000 150000 200000 250000 300000 350000 400000)

RESULTS_DIR="data/load-tests/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "============================================"
echo " Load Test: ${URL}"
echo " Duration per tier: ${DURATION}"
echo " Auth: $([ -n "$COOKIE" ] && echo "yes (cookie)" || echo "no")"
echo " Results: ${RESULTS_DIR}/"
echo "============================================"
echo ""

# Build extra oha args for auth
OHA_EXTRA=()
CURL_EXTRA=()
if [[ -n "$COOKIE" ]]; then
    OHA_EXTRA+=(-H "Cookie: ${COOKIE}")
    CURL_EXTRA+=(-H "Cookie: ${COOKIE}")
fi

# Sanity check: can we reach the endpoint at all?
echo "--- Preflight: single request ---"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "${CURL_EXTRA[@]+"${CURL_EXTRA[@]}"}" "$URL" 2>/dev/null || echo "FAIL")
if [[ "$STATUS" != "200" ]]; then
    echo "FAIL: ${URL} returned ${STATUS} (expected 200)"
    if [[ "$STATUS" == "303" || "$STATUS" == "302" || "$STATUS" == "401" ]]; then
        echo "This endpoint requires authentication. Pass a session cookie as the 3rd arg."
        echo "Example: $0 $ENDPOINT \"$BASE_URL\" \"id=YOUR_SESSION_COOKIE\""
    fi
    exit 1
fi
echo "OK: ${URL} returned 200"
echo ""

# Pick connection count based on rate tier.
# At very high rates, we need more connections so oha can actually
# generate enough concurrent requests. Also need enough to saturate
# all server cores.
connections_for_rate() {
    local rate=$1
    if   (( rate <= 50000 ));  then echo 1000
    elif (( rate <= 150000 )); then echo 3000
    elif (( rate <= 250000 )); then echo 5000
    else                            echo 8000
    fi
}

# Convert latency from seconds to ms for display
fmt_ms() { [[ "$1" == "N/A" ]] && echo "N/A" || awk "BEGIN {printf \"%.2fms\", $1 * 1000}"; }
fmt_rps() { [[ "$1" == "N/A" ]] && echo "N/A" || awk "BEGIN {printf \"%d\", $1}"; }

echo "tier_rps,actual_rps,latency_p50,latency_p95,latency_p99,latency_max,success_rate,status" \
    > "${RESULTS_DIR}/summary.csv"

for RATE in "${TIERS[@]}"; do
    CONNS=$(connections_for_rate "$RATE")
    TIER_FILE="${RESULTS_DIR}/tier-${RATE}rps.json"

    echo "--- Tier: ${RATE} RPS  (${CONNS} connections, ${DURATION}) ---"

    # Run oha with rate limiting, JSON output, no TUI
    oha -q "$RATE" \
        -c "$CONNS" \
        -z "$DURATION" \
        --no-tui \
        --output-format json \
        "${OHA_EXTRA[@]+"${OHA_EXTRA[@]}"}" \
        "$URL" > "$TIER_FILE" 2>&1 || true

    # Parse results from JSON
    ACTUAL_RPS=$(jq -r '.summary.requestsPerSec // "N/A"' "$TIER_FILE" 2>/dev/null || echo "N/A")
    P50=$(jq -r '.latencyPercentiles.p50 // "N/A"' "$TIER_FILE" 2>/dev/null || echo "N/A")
    P95=$(jq -r '.latencyPercentiles.p95 // "N/A"' "$TIER_FILE" 2>/dev/null || echo "N/A")
    P99=$(jq -r '.latencyPercentiles.p99 // "N/A"' "$TIER_FILE" 2>/dev/null || echo "N/A")
    LATENCY_MAX=$(jq -r '.latencyPercentiles["p99.99"] // "N/A"' "$TIER_FILE" 2>/dev/null || echo "N/A")
    SUCCESS_RATIO=$(jq -r '.summary.successRate // "N/A"' "$TIER_FILE" 2>/dev/null || echo "N/A")

    if [[ "$SUCCESS_RATIO" != "N/A" ]]; then
        SUCCESS_RATE=$(awk "BEGIN {printf \"%.2f\", $SUCCESS_RATIO * 100}")
    else
        SUCCESS_RATE="N/A"
    fi

    # Determine tier status
    if [[ "$SUCCESS_RATE" == "N/A" ]]; then
        TIER_STATUS="ERROR"
    elif (( $(awk "BEGIN {print ($SUCCESS_RATE < 99.0)}") )); then
        TIER_STATUS="DEGRADED"
    else
        TIER_STATUS="OK"
    fi

    # Print tier summary
    printf "  Actual RPS:   %s\n" "$(fmt_rps "$ACTUAL_RPS")"
    printf "  Latency p50:  %s\n" "$(fmt_ms "$P50")"
    printf "  Latency p95:  %s\n" "$(fmt_ms "$P95")"
    printf "  Latency p99:  %s\n" "$(fmt_ms "$P99")"
    printf "  Latency max:  %s\n" "$(fmt_ms "$LATENCY_MAX")"
    printf "  Success rate: %s%%\n" "$SUCCESS_RATE"
    printf "  Status:       %s\n" "$TIER_STATUS"
    echo ""

    # Append to CSV
    echo "${RATE},${ACTUAL_RPS},${P50},${P95},${P99},${LATENCY_MAX},${SUCCESS_RATE},${TIER_STATUS}" \
        >> "${RESULTS_DIR}/summary.csv"

    # Stop early if things are falling apart
    if [[ "$TIER_STATUS" == "DEGRADED" ]]; then
        echo ">>> Degraded at ${RATE} RPS — continuing to find the breaking point..."
    fi

    # Brief pause between tiers to let the server settle
    sleep 2
done

echo "============================================"
echo " Done! Results saved to ${RESULTS_DIR}/"
echo ""
echo " summary.csv  — one-line-per-tier overview"
echo " tier-*rps.json — full oha output per tier"
echo "============================================"
echo ""
echo "Quick view:"
column -t -s',' "${RESULTS_DIR}/summary.csv"
