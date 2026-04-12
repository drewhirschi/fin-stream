# syntax=docker/dockerfile:1.7

FROM rust:1.88-bookworm AS chef
WORKDIR /app

RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY askama.toml ./
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json ./recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY askama.toml ./
COPY src ./src
COPY templates ./templates
COPY static ./static

RUN cargo build --release --locked --bin trust-deeds

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/trust-deeds /usr/local/bin/trust-deeds
COPY --from=builder /app/templates ./templates
COPY --from=builder /app/static ./static

RUN useradd --system --uid 10001 --create-home appuser \
    && chown -R appuser:appuser /app

USER appuser

ENV HOST=0.0.0.0
ENV PORT=3000
ENV DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/trust_deeds

EXPOSE 3000

CMD ["trust-deeds"]
