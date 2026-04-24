#!/usr/bin/env bash
# Measure cold-start time: ms from `docker run` to first /health 200 OK.
# Polls the container via its docker-bridge IP (skips iptables DNAT, also
# means this works on hosts where the DOCKER chain isn't fully set up).
#
# Usage: measure-startup.sh <variant> <image> <csv-out>
set -euo pipefail

VARIANT="$1"
IMAGE="$2"
OUT="$3"
SAMPLES="${SAMPLES:-10}"
CTN="wasm-bench-${VARIANT}-startup"

echo "ms,attempt,timestamp" > "$OUT"

for i in $(seq 1 "$SAMPLES"); do
    docker rm -f "$CTN" >/dev/null 2>&1 || true

    START_NS=$(date +%s%N)
    docker run -d --rm --name "$CTN" "$IMAGE" >/dev/null

    # Grab the bridge IP. Container starts immediately; IP assignment is
    # typically available within the first Docker inspect.
    IP=""
    for _ in $(seq 1 100); do
        IP=$(docker inspect "$CTN" --format '{{.NetworkSettings.Networks.bridge.IPAddress}}' 2>/dev/null || true)
        [ -n "$IP" ] && break
        sleep 0.002
    done
    if [ -z "$IP" ]; then
        echo "  [$i/$SAMPLES] failed to get IP for $VARIANT" >&2
        docker rm -f "$CTN" >/dev/null 2>&1 || true
        exit 1
    fi

    # Poll /health with a tight loop, 2ms between tries.
    DEADLINE=$(( START_NS + 30 * 1000000000 ))
    while true; do
        NOW_NS=$(date +%s%N)
        if [ "$NOW_NS" -gt "$DEADLINE" ]; then
            echo "  [$i/$SAMPLES] TIMEOUT after 30s for $VARIANT" >&2
            docker logs "$CTN" 2>&1 | tail -40 >&2 || true
            docker rm -f "$CTN" >/dev/null 2>&1 || true
            exit 1
        fi
        if curl -fsS --max-time 1 "http://$IP:3000/health" >/dev/null 2>&1; then
            break
        fi
        sleep 0.002
    done
    END_NS=$(date +%s%N)

    MS=$(( (END_NS - START_NS) / 1000000 ))
    TS=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    echo "$MS,$i,$TS" >> "$OUT"
    echo "  [$i/$SAMPLES] $VARIANT ready in ${MS}ms"

    docker rm -f "$CTN" >/dev/null 2>&1 || true
    sleep 0.2
done
