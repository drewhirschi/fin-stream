# syntax=docker/dockerfile:1.7
#
# Native x86_64 bench — mirrors the production trust-deeds Dockerfile:
# rust:1.88-bookworm builder, debian:bookworm-slim runtime.
FROM rust:1.88-bookworm AS builder
WORKDIR /app

COPY Cargo.toml ./
COPY askama.toml ./
COPY src ./src
COPY templates ./templates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin wasm-bench-native \
    && cp target/release/wasm-bench-native /usr/local/bin/wasm-bench-native

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/local/bin/wasm-bench-native /usr/local/bin/wasm-bench-native

ENV LISTEN_ADDR=0.0.0.0:3000
EXPOSE 3000

CMD ["wasm-bench-native"]
