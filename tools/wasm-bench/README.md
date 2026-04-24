# wasm-bench

Apples-to-apples benchmark: the same HTTP+template workload built two ways.

- `native/` — Axum + Askama, compiled for `x86_64-unknown-linux-gnu`, packaged in `debian:bookworm-slim`. Same stack as the production `trust-deeds` app.
- `wasm/` — Same routes and template, compiled for `wasm32-wasip2` as a `wasi:http/proxy` component, served by `wasmtime serve` inside a minimal runtime image.

Both apps expose:
- `GET /health` → `ok`
- `GET /bench/render` → a loan-overview HTML page with 15 loans + 8 payments (data hardcoded in-process, same shape as `/bench/render` on the main app).

The render path in both apps uses Askama, so the template compilation is identical. The only moving variable is the HTTP serving layer: native Axum/Tokio vs `wasi:http` dispatched by wasmtime.

## Run the experiments

```sh
cd tools/wasm-bench

# Docker-wrapped measurements
make build                                       # build both images (first run installs wasm32-wasip2)
make size                                        # print image sizes
./scripts/measure-compile.sh data/compile.csv    # cold cargo build for each target
make startup                                     # cold-start timings (10 runs each)
make rps                                         # oha ramp, 1k..40k, 10s per tier

# Bare-host measurements (no Docker for either variant)
# Needs ~/.local/bin/wasmtime (or `wasmtime` on PATH).
make bare                                        # extracts artifacts from images, runs bare bench

make report                                      # concatenate results into data/summary.md
```

Results land in `data/`. The human-readable writeup goes to
`../../../rust-web-scaffold/docs/wasm-overhead.md`.

## Why these specific choices

- `wasmtime serve` (not Spin, not wasmCloud) to isolate the runtime cost. Spin adds its own routing layer and configuration surface; we want to know what bare WASI+HTTP costs.
- wasip2 (not wasip1) because wasip1 has no sockets — you can't serve HTTP from it without host-side shims. wasip2's `wasi:http` world is the official Preview 2 story.
- Same Askama template in both so template rendering isn't what differs. The only real difference is request dispatch.
- Direct-to-container benchmarking (Docker network), same as the Go-vs-Rust bench, so LAN is not the bottleneck.
