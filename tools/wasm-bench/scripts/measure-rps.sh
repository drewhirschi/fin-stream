#!/usr/bin/env bash
# Ramped-rate RPS benchmark using oha. Targets the container via its
# docker-bridge IP so we avoid iptables DNAT overhead and match the
# "inside the docker network" methodology used in the Go-vs-Rust bench.
#
# Usage: measure-rps.sh <variant> <image> <csv-out>
set -euo pipefail

VARIANT="$1"
IMAGE="$2"
OUT="$3"
PATH_UNDER_TEST="${BENCH_PATH:-/bench/render}"
DURATION="${DURATION:-10s}"
CONNECTIONS="${CONNECTIONS:-200}"
TIERS="${TIERS:-1000 2000 5000 10000 20000 40000}"
CTN="wasm-bench-${VARIANT}-rps"

echo "tier,actual_rps,p50_ms,p99_ms,success_pct" > "$OUT"

docker rm -f "$CTN" >/dev/null 2>&1 || true
docker run -d --rm --name "$CTN" "$IMAGE" >/dev/null

IP=""
for _ in $(seq 1 300); do
    IP=$(docker inspect "$CTN" --format '{{.NetworkSettings.Networks.bridge.IPAddress}}' 2>/dev/null || true)
    if [ -n "$IP" ] && curl -fsS --max-time 1 "http://$IP:3000/health" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if [ -z "$IP" ]; then
    echo "could not get IP for $CTN" >&2
    docker logs "$CTN" 2>&1 | tail -40 >&2 || true
    docker rm -f "$CTN" >/dev/null 2>&1 || true
    exit 1
fi

URL="http://$IP:3000${PATH_UNDER_TEST}"
echo "target: $URL"

# Warmup: prime caches, let wasmtime JIT warm up.
oha -z 3s -q 1000 -c 50 --no-tui "$URL" >/dev/null 2>&1 || true

for TIER in $TIERS; do
    RAW=$(oha -z "$DURATION" -q "$TIER" -c "$CONNECTIONS" --no-tui "$URL" 2>&1 || true)

    ACTUAL=$(echo "$RAW" | awk '/Requests\/sec:/ {print $2}' | tr -d ',')
    P50_SEC=$(echo "$RAW" | awk '/  50\.00% in/ {print $3}')
    P99_SEC=$(echo "$RAW" | awk '/  99\.00% in/ {print $3}')
    SUCCESS=$(echo "$RAW" | awk '/Success rate:/ {print $3}' | tr -d '%')

    P50_MS=$(awk -v s="$P50_SEC" 'BEGIN {if (s=="") print "-"; else printf "%.1f", s*1000}')
    P99_MS=$(awk -v s="$P99_SEC" 'BEGIN {if (s=="") print "-"; else printf "%.1f", s*1000}')

    echo "$TIER,${ACTUAL:-0},${P50_MS},${P99_MS},${SUCCESS:-0}" >> "$OUT"
    echo "  $VARIANT tier=$TIER → ${ACTUAL:-0} RPS, p99=${P99_MS}ms, success=${SUCCESS}%"
done

docker rm -f "$CTN" >/dev/null 2>&1 || true
