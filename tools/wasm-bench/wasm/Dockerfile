# syntax=docker/dockerfile:1.7
#
# WASM bench — compile a wasi:http/proxy component with Rust's wasm32-wasip2
# target, then serve it with `wasmtime serve` inside a minimal runtime image.

FROM rust:1.88-bookworm AS builder
WORKDIR /app

# rust:1.88-bookworm ships rustup; install the wasm32-wasip2 target.
RUN rustup target add wasm32-wasip2

COPY Cargo.toml ./
COPY askama.toml ./
COPY src ./src
COPY templates ./templates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --target wasm32-wasip2 \
    && cp target/wasm32-wasip2/release/wasm_bench_wasm.wasm /app/bench.wasm

# wasmtime runtime image: debian-slim + the wasmtime binary.
FROM debian:bookworm-slim AS runtime
WORKDIR /app

ARG WASMTIME_VERSION=44.0.0
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl xz-utils \
    && curl -fsSL "https://github.com/bytecodealliance/wasmtime/releases/download/v${WASMTIME_VERSION}/wasmtime-v${WASMTIME_VERSION}-x86_64-linux.tar.xz" \
         -o /tmp/wasmtime.tar.xz \
    && tar -xJf /tmp/wasmtime.tar.xz -C /tmp \
    && install -m 0755 /tmp/wasmtime-v${WASMTIME_VERSION}-x86_64-linux/wasmtime /usr/local/bin/wasmtime \
    && rm -rf /tmp/wasmtime* \
    && apt-get purge -y xz-utils \
    && apt-get autoremove -y \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/bench.wasm /app/bench.wasm

ENV LISTEN_ADDR=0.0.0.0:3000
EXPOSE 3000

# `wasmtime serve` starts an HTTP listener on --addr and dispatches incoming
# requests to the component's wasi:http/proxy export. JIT-compiles on first
# request; subsequent requests reuse the compiled code.
CMD ["sh", "-c", "wasmtime serve -S cli=y --addr $LISTEN_ADDR /app/bench.wasm"]
