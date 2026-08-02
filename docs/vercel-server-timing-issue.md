# Vercel silently strips the standard `Server-Timing` response header

## Expected behavior

`Server-Timing` is a standard HTTP response header (https://www.w3.org/TR/server-timing/) that browsers parse natively and surface in the DevTools Network/Timing panels and the Performance API. Vercel's response-header and `vercel.json` documentation does not identify it as reserved, unsupported, or filtered:

- https://vercel.com/docs/headers/response-headers
- https://vercel.com/docs/project-configuration/vercel-json#headers

A response that sets it should reach the client with the header intact, e.g.:

```http
Server-Timing: db;dur=24.8, handler;dur=31.2, total;dur=31.5
```

## Actual behavior

Vercel removes `Server-Timing` from production responses on two independent paths, while forwarding adjacent custom headers carrying the identical value. Reproducible with `curl` (so not browser-side), first observed 2026-07-21, still reproducible 2026-08-02.

**1. Static asset, headers set in `vercel.json`** — one rule sets both a custom control header and `Server-Timing` on the same response:

```json
{
  "source": "/static/server-timing-probe-20260721.txt",
  "headers": [
    { "key": "X-Server-Timing-Probe", "value": "control-present" },
    { "key": "Server-Timing", "value": "edge-probe;dur=1.0" }
  ]
}
```

```sh
curl -sS -D - -o /dev/null https://finstream.hirschi.dev/static/server-timing-probe-20260721.txt
```

```http
HTTP/2 200
age: 0
cache-control: private, no-store
x-server-timing-probe: control-present
x-vercel-cache: MISS
x-vercel-id: sfo1::rsgx2-1785647824105-0766e55a7872
```

The control header from the same rule arrives; `Server-Timing` does not. Captured on a cache miss with `age: 0`.

**2. Function response** — the application (Rust runtime, `pdx1`) emits `Server-Timing` and, immediately before handing the response to the Vercel runtime, mirrors the exact same value into `x-debug-server-timing`:

```sh
curl -sS -D - -o /dev/null https://finstream.hirschi.dev/login
```

```http
HTTP/2 200
x-debug-server-timing: mw;dur=0.0, handler;dur=0.0, total;dur=0.0, route;desc="/login"
x-vercel-id: sfo1::pdx1::gbd2p-1785647824366-658327fd7550
```

The mirror header (added to the same response object, after the original) arrives; `Server-Timing` does not. The `sfo1::pdx1` id confirms edge traversal plus function execution.

Together these rule out the browser, the application framework, the function runtime, response streaming, caching, and general custom-header handling — the control headers travel the identical path and survive. The remaining common boundary is Vercel's platform response processing.

Is this intentional? If so, please document the restriction and a supported way to expose standard server-timing metrics; if not, please stop filtering the header.
