#!/usr/bin/env bash
# Bare-host benchmarks — no Docker for either variant.
#
# Measures:
#   - startup: ms from exec to first successful /health 200
#   - rps:     peak sustained /bench/render throughput under oha -c 500
#   - size:    just the artifacts (binary, .wasm, wasmtime runtime)
#
# Artifacts expected in tools/wasm-bench/bare/:
#   wasm-bench-native   - native x86_64 Rust binary
#   bench.wasm          - wasi:http/proxy component
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BARE="$ROOT/bare"
DATA="$ROOT/data"
mkdir -p "$DATA"

NATIVE_BIN="$BARE/wasm-bench-native"
WASM_FILE="$BARE/bench.wasm"
WASMTIME="${WASMTIME:-$HOME/.local/bin/wasmtime}"

if [ ! -x "$NATIVE_BIN" ] || [ ! -f "$WASM_FILE" ] || [ ! -x "$WASMTIME" ]; then
    echo "missing prereqs — run Makefile build + extract first" >&2
    exit 1
fi

SAMPLES="${SAMPLES:-10}"
DURATION="${DURATION:-15s}"
CONNS="${CONNS:-500}"

NATIVE_PORT=3301
WASM_PORT=3302

cleanup() {
    pkill -f "$NATIVE_BIN" 2>/dev/null || true
    pkill -f "wasmtime serve" 2>/dev/null || true
}
trap cleanup EXIT

# ---------- Startup ----------
STARTUP_CSV="$DATA/bare-startup.csv"
echo "variant,attempt,ms" > "$STARTUP_CSV"

startup_sample() {
    local variant="$1" port="$2"; shift 2
    local start_ns end_ns ms
    start_ns=$(date +%s%N)
    "$@" >/tmp/bare-$variant.log 2>&1 &
    local pid=$!
    local deadline=$(( start_ns + 15 * 1000000000 ))
    while true; do
        if curl -fsS --max-time 0.2 "http://127.0.0.1:$port/health" >/dev/null 2>&1; then
            break
        fi
        if [ "$(date +%s%N)" -gt "$deadline" ]; then
            echo "TIMEOUT $variant" >&2
            kill -9 "$pid" 2>/dev/null || true
            cat /tmp/bare-$variant.log >&2
            return 1
        fi
    done
    end_ns=$(date +%s%N)
    ms=$(( (end_ns - start_ns) / 1000000 ))
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
    sleep 0.05
    echo "$ms"
}

echo "=== startup: native bare ==="
for i in $(seq 1 "$SAMPLES"); do
    MS=$(LISTEN_ADDR=127.0.0.1:$NATIVE_PORT startup_sample native "$NATIVE_PORT" \
         env LISTEN_ADDR=127.0.0.1:$NATIVE_PORT "$NATIVE_BIN")
    echo "  [$i/$SAMPLES] native ${MS}ms"
    echo "native,$i,$MS" >> "$STARTUP_CSV"
done

echo "=== startup: wasmtime serve bare ==="
for i in $(seq 1 "$SAMPLES"); do
    MS=$(startup_sample wasm "$WASM_PORT" "$WASMTIME" serve -S cli=y --addr 127.0.0.1:$WASM_PORT "$WASM_FILE")
    echo "  [$i/$SAMPLES] wasm   ${MS}ms"
    echo "wasm,$i,$MS" >> "$STARTUP_CSV"
done

echo "---"
echo "startup summary ms (min median max):"
for v in native wasm; do
    awk -F, -v v=$v 'NR>1 && $1==v {print $3}' "$STARTUP_CSV" | sort -n \
      | awk -v v=$v '{a[NR]=$1} END{med=(NR%2)?a[(NR+1)/2]:(a[NR/2]+a[NR/2+1])/2; printf "  %-8s %4d  %4d  %4d\n", v, a[1], med, a[NR]}'
done

# ---------- RPS ----------
RPS_CSV="$DATA/bare-rps.csv"
echo "variant,peak_rps,p50_ms,p99_ms,cpu_percent,mem_mib" > "$RPS_CSV"

run_rps() {
    local variant="$1" port="$2"; shift 2

    echo "=== rps: $variant bare ==="
    "$@" >/tmp/bare-$variant.log 2>&1 &
    local wrapper_pid=$!

    # Wait until /health responds; by then the child server is bound.
    for _ in $(seq 1 300); do
        curl -fsS --max-time 0.2 "http://127.0.0.1:$port/health" >/dev/null 2>&1 && break
        sleep 0.05
    done

    # Find the REAL listening PID from the TCP port (robust to wrappers).
    local server_pid
    server_pid=$(ss -lntpH "sport = :$port" 2>/dev/null | grep -oE 'pid=[0-9]+' | head -1 | cut -d= -f2)
    if [ -z "$server_pid" ] || [ ! -e "/proc/$server_pid" ]; then
        echo "could not identify server pid on :$port" >&2
        server_pid="$wrapper_pid"
    fi
    echo "  server pid: $server_pid"

    # Warmup burst
    oha -z 3s -c 50 --no-tui "http://127.0.0.1:$port/bench/render" >/dev/null 2>&1 || true

    # Peak run backgrounded; mid-run sample CPU/mem via /proc
    oha -z "$DURATION" -c "$CONNS" --no-tui "http://127.0.0.1:$port/bench/render" >/tmp/oha-$variant.txt 2>&1 &
    local oha_pid=$!
    sleep 3

    local cpu_samples=() mem_samples=()
    local clk; clk=$(getconf CLK_TCK)
    for _ in 1 2 3 4 5; do
        if [ ! -e "/proc/$server_pid/stat" ]; then break; fi
        read -r u1 s1 < <(awk '{print $14, $15}' /proc/$server_pid/stat)
        sleep 0.5
        if [ ! -e "/proc/$server_pid/stat" ]; then break; fi
        read -r u2 s2 < <(awk '{print $14, $15}' /proc/$server_pid/stat)
        local d=$(( (u2 - u1) + (s2 - s1) ))
        local pct
        pct=$(awk -v d="$d" -v clk="$clk" 'BEGIN{printf "%.0f", (d/clk)/0.5 * 100}')
        cpu_samples+=("$pct")
        local mkb; mkb=$(awk '/VmRSS:/ {print $2}' /proc/$server_pid/status 2>/dev/null || echo 0)
        mem_samples+=("$mkb")
        sleep 0.8
    done

    wait "$oha_pid"
    kill "$wrapper_pid" 2>/dev/null || true
    pkill -P "$wrapper_pid" 2>/dev/null || true
    wait "$wrapper_pid" 2>/dev/null || true
    sleep 0.2

    local cpu_avg mem_avg
    if [ "${#cpu_samples[@]}" -gt 0 ]; then
        cpu_avg=$(printf '%s\n' "${cpu_samples[@]}" | awk '{s+=$1} END{printf "%.0f", s/NR}')
        mem_avg=$(printf '%s\n' "${mem_samples[@]}" | awk '{s+=$1} END{printf "%.0f", (s/NR)/1024}')
    else
        cpu_avg="-"
        mem_avg="-"
    fi

    local rps p50 p99
    rps=$(awk '/Requests\/sec:/ {print $2}' /tmp/oha-$variant.txt)
    p50=$(awk '/  50\.00% in/ {printf "%.1f", $3*1000}' /tmp/oha-$variant.txt)
    p99=$(awk '/  99\.00% in/ {printf "%.1f", $3*1000}' /tmp/oha-$variant.txt)

    echo "  $variant peak=$rps RPS  p50=${p50}ms  p99=${p99}ms  cpu=${cpu_avg}%  mem=${mem_avg}MiB"
    echo "$variant,$rps,$p50,$p99,$cpu_avg,$mem_avg" >> "$RPS_CSV"
}

run_rps native "$NATIVE_PORT" env LISTEN_ADDR=127.0.0.1:$NATIVE_PORT "$NATIVE_BIN"
run_rps wasm   "$WASM_PORT"  "$WASMTIME" serve -S cli=y --addr 127.0.0.1:$WASM_PORT "$WASM_FILE"

echo "---"
cat "$RPS_CSV" | column -t -s,

# ---------- Size ----------
SIZE_CSV="$DATA/bare-size.csv"
echo "variant,what,bytes,human" > "$SIZE_CSV"
for pair in "native:$NATIVE_BIN" "wasm:$WASM_FILE" "wasm-runtime:$WASMTIME"; do
    label="${pair%%:*}"; path="${pair#*:}"
    bytes=$(stat -c%s "$path")
    human=$(numfmt --to=iec-i --suffix=B "$bytes")
    echo "$label,$path,$bytes,$human" >> "$SIZE_CSV"
done
echo "---"
cat "$SIZE_CSV" | column -t -s,
