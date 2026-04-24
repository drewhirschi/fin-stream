#!/usr/bin/env bash
# Measure clean-build compile time for both targets.
# Runs `cargo build --release` in a rust:1.88-bookworm container, once
# for each target, starting from a cold cargo registry + target dir.
# The wasm32-wasip2 target is pre-installed before timing starts so we're
# measuring compile work, not rustup download time.
#
# Writes two rows to data/compile.csv: one for native, one for wasm.
set -euo pipefail

OUT="${1:-data/compile.csv}"
mkdir -p "$(dirname "$OUT")"

echo "variant,seconds,wall_clock,target,rustc_version" > "$OUT"

BENCH_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

docker run --rm \
    -v "$BENCH_ROOT":/work \
    -w /work \
    rust:1.88-bookworm \
    bash -c '
set -euo pipefail
echo "--- installing wasm32-wasip2 target (not counted) ---" >&2
rustup target add wasm32-wasip2 >&2

RUSTC=$(rustc --version)

for variant in native wasm; do
    echo "--- $variant clean build ---" >&2
    # Fresh target dir and cargo home per run so neither variant benefits
    # from the others cache. Seed both with the same registry state.
    rm -rf "/tmp/${variant}-target" "/tmp/${variant}-cargo"
    cp -r /work/$variant /tmp/src-$variant
    mkdir -p "/tmp/${variant}-target" "/tmp/${variant}-cargo"

    if [ "$variant" = "native" ]; then
        CMD="cargo build --release --bin wasm-bench-native"
    else
        CMD="cargo build --release --target wasm32-wasip2"
    fi

    START=$(date +%s.%N)
    CARGO_HOME=/tmp/${variant}-cargo CARGO_TARGET_DIR=/tmp/${variant}-target \
        bash -c "cd /tmp/src-$variant && $CMD" >&2
    END=$(date +%s.%N)

    SEC=$(awk -v s="$START" -v e="$END" "BEGIN{printf \"%.2f\", e-s}")
    WALL=$(date -u -d "@${SEC%.*}" +%H:%M:%S 2>/dev/null || echo "-")
    TARGET=$([ "$variant" = "native" ] && echo "x86_64-unknown-linux-gnu" || echo "wasm32-wasip2")
    echo "$variant,$SEC,$WALL,$TARGET,$RUSTC" >> /work/'"$OUT"'
    echo "  $variant: ${SEC}s" >&2
done
'

echo "---"
cat "$OUT" | column -t -s,
