# syntax=docker/dockerfile:1.7

# ── CSS builder ──────────────────────────────────────────────────────────────
# Runs Tailwind v4 against our Askama templates, emits a minified+purged
# static/app.css. Replaces the browser-JIT Tailwind runtime entirely.
FROM node:22-alpine AS css-builder
WORKDIR /css
COPY package.json package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY static/app.input.css ./static/
COPY templates ./templates
COPY src ./src
RUN npx @tailwindcss/cli -i ./static/app.input.css -o ./static/app.css --minify

# ── Rust builder ─────────────────────────────────────────────────────────────
FROM rust:1.88-bookworm AS chef
WORKDIR /app

RUN cargo install cargo-chef --locked

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY askama.toml ./
# cargo metadata needs target files present to resolve package targets.
COPY src/lib.rs src/main.rs ./src/
COPY src/bin ./src/bin
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
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/trust-deeds /usr/local/bin/trust-deeds
COPY --from=builder /app/templates ./templates
COPY --from=builder /app/static ./static
# Purged, minified Tailwind bundle — overrides any stale app.css copied above.
COPY --from=css-builder /css/static/app.css ./static/app.css

RUN useradd --system --uid 10001 --create-home appuser \
    && chown -R appuser:appuser /app

USER appuser

ENV HOST=0.0.0.0
ENV PORT=3000
ENV DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/trust_deeds

EXPOSE 3000

CMD ["trust-deeds"]
