# Traefik Routing Cleanup

## Problem

Traffic to the app on `gory:8008` goes through a standalone nginx container (`trust-deeds-vpn`) with a single worker thread, bottlenecking throughput at ~17k RPS. The Rust app itself handles 100k+ RPS when hit directly.

## Current state

```
gory:80  (Traefik)
  ├── Host("gory")                                    → Coolify dashboard (:8080)
  ├── Host("finstream-webhooks.hirschi.dev")           → App (:3000)  ✅ webhooks
  └── Host("gory") && PathPrefix("/")                  → App (:8008)  ⚠️ conflicts with dashboard

gory:8008  (nginx "trust-deeds-vpn")
  └── everything → host.docker.internal:3801 → App     🐌 bottleneck

gory:3801  → App container (:3000 internal)             ✅ direct, fast
```

Problems:
1. nginx single-worker caps throughput at ~17k RPS
2. App's Traefik label has `Host("gory")` which conflicts with Coolify dashboard
3. App's Traefik label routes to port 8008 (the nginx port), not the app's real port

## Desired state

```
gory:80  (Traefik)
  ├── Host("gory") or Host("gory.local")              → Coolify dashboard
  └── Host("finstream-webhooks.hirschi.dev")           → App (:3000)  webhooks

gory:8008  (Traefik, new entrypoint)
  └── all traffic                                      → App (:3000)  fast, direct
```

- Coolify dashboard stays on `gory:80`
- App gets its own Traefik entrypoint on `:8008` — no hostname juggling, no /etc/hosts on every device
- Webhooks unchanged (cloudflared → Traefik :80 → app)
- nginx container removed

## Steps

### 1. Add :8008 entrypoint to Traefik

Edit Traefik's docker-compose on gory at `/data/coolify/source/docker-compose.yml` (or wherever Coolify stores the proxy compose file). Add to the ports and command:

```yaml
# ports section — add:
- '8008:8008'

# command section — add:
- '--entrypoints.app.address=:8008'
```

### 2. Update app's Traefik labels in Coolify

In Coolify's UI for the Trust Deeds app, update the domain/routing config. The labels should be:

```
# Webhook route (keep as-is)
traefik.http.routers.http-0-<id>.entryPoints=http
traefik.http.routers.http-0-<id>.rule=Host(`finstream-webhooks.hirschi.dev`) && PathPrefix(`/`)
traefik.http.services.http-0-<id>.loadbalancer.server.port=3000

# App route (fix: use new entrypoint, remove Host("gory") conflict)
traefik.http.routers.http-1-<id>.entryPoints=app
traefik.http.routers.http-1-<id>.rule=PathPrefix(`/`)
traefik.http.services.http-1-<id>.loadbalancer.server.port=3000
```

Key changes:
- Entrypoint changed from `http` to `app` (the new :8008 entrypoint)
- Rule simplified to just `PathPrefix("/")` — no `Host("gory")` needed since it's the only thing on :8008
- Service port changed from `8008` to `3000` (app's actual port)

### 3. Stop and remove the nginx container

```bash
docker stop trust-deeds-vpn && docker rm trust-deeds-vpn
```

### 4. Restart Traefik to pick up the new entrypoint

```bash
cd /data/coolify/source && docker compose up -d
```

### 5. Redeploy the app in Coolify

So it picks up the new labels.

### 6. Verify

```bash
# App works on :8008 (now through Traefik, not nginx)
curl http://gory:8008/health

# Coolify dashboard still works on :80
curl -s -o /dev/null -w "%{http_code}" http://gory:80

# Webhooks still work
curl -sI -X POST https://finstream-webhooks.hirschi.dev/webhooks/resend
```

### 7. Re-run load test

```bash
./scripts/load-test.sh /health http://gory:8008
```

Should now see performance close to the direct :3801 numbers.

## Risk

- Traefik restart briefly drops all proxied traffic (~1-2 seconds)
- If labels are wrong, the app becomes unreachable on :8008 — but :3801 direct still works as fallback
- Webhooks go through a different entrypoint (:80) so they're unaffected by :8008 changes
