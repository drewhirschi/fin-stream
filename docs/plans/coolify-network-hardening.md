# Coolify Network + Deployment Outstanding Work

Remaining infrastructure work to finish the deployment. App-level auth is being handled separately — see `session-auth.md`.

## Context

The app is deployed to `gory` via Coolify behind Tailscale. Architecture is set (VPN-only access, no public port forwarding) and documented in `CLAUDE.md`. What's left:

1. Cloudflare Tunnel to publicly expose the Resend webhook endpoint only
2. Verify the local multi-arch Docker build + push pipeline
3. Clean up the stale `iptables-persistent` install
4. Verify the `.env` `DATABASE_URL` that points at `gory:5433`

---

## 1. Cloudflare Tunnel for webhooks

Goal: `https://webhooks.<domain>/webhooks/resend` reaches the Trust Deeds app; every other path returns 404. No home router port forwarding.

### 1.1 — Cloudflare setup

1. Create a Cloudflare account (free tier is fine)
2. **Zero Trust → Networks → Tunnels** → create a tunnel named `fin-stream-webhooks`
3. Copy the tunnel token (shown once)
4. Note the tunnel's `*.cfargotunnel.com` UUID address

### 1.2 — DNS

At the current registrar for the domain you'll use (e.g. `hirschi.dev`), add:

```
Type:  CNAME
Name:  webhooks
Value: <tunnel-uuid>.cfargotunnel.com
```

No NS migration. No Cloudflare proxy. The tunnel is the only ingress path.

### 1.3 — Cloudflared as a Coolify service

In Coolify, add a new service (Docker Compose) on the same network as the app:

```yaml
services:
  cloudflared:
    image: cloudflare/cloudflared:latest
    restart: unless-stopped
    command: tunnel --no-autoupdate run --token ${CF_TUNNEL_TOKEN}
```

Set `CF_TUNNEL_TOKEN` as a secret env var on the service.

### 1.4 — Tunnel ingress rules

In Cloudflare Zero Trust → Tunnels → this tunnel → Public Hostnames, add:

- **Hostname**: `webhooks.<domain>`
- **Path**: `/webhooks/*`
- **Service**: `http://trust-deeds:3000` (the app container's internal DNS name + port)

Add a catch-all fallback that returns 404:
- **Hostname**: `webhooks.<domain>`
- **Path**: `.*`
- **Service**: `http_status:404`

This ensures only `/webhooks/*` reaches the app even if the tunnel hostname is discovered.

### 1.5 — Resend dashboard

- Webhook URL: `https://webhooks.<domain>/webhooks/resend`
- Copy signing secret → `RESEND_WEBHOOK_SECRET` in Coolify env vars
- Trigger a test webhook → check app logs for 200 response and DB insert

### Verification

```bash
# From anywhere (no Tailscale): only /webhooks/* should work
curl -sI https://webhooks.<domain>/                    # 404
curl -sI https://webhooks.<domain>/api/forecast         # 404
curl -sI -X POST https://webhooks.<domain>/webhooks/resend  # 401 (no Svix headers) — OK

# Tailscale access to main app still works
curl -sI http://gory                                    # Coolify login
```

---

## 2. Verify multi-arch Docker build

The `Makefile` has `make build` that builds amd64 + arm64 and pushes to `ghcr.io/drewhirschi/fin-stream`. This has never been run from the local machine.

```bash
# Log in to GHCR first
docker login ghcr.io
# Username: drewhirschi
# Password: <GHCR PAT with write:packages scope>

# First run will create a buildx builder using QEMU for arm64 emulation (slow first time)
make build
```

If arm64 build is too slow via QEMU, two options:
- Rely on the existing GitHub Actions workflow (`.github/workflows/ci-image.yml`) — already set up, runs on push to main
- Add a remote buildx node on an arm64 machine

After a successful `make build`, `make deploy` tells Coolify to pull the new `:latest` and redeploy. `make ship` does both.

---

## 3. Clean up `iptables-persistent`

Installed during the abandoned firewall attempt. No custom rules active now.

Optional uninstall:

```bash
ssh gory
sudo apt remove --purge iptables-persistent netfilter-persistent
```

Or leave it installed — it's harmless without active rules.

---

## 4. Verify `DATABASE_URL` in `.env`

Local `.env` has:
```
DATABASE_URL=postgres://postgres:...@gory:5433/postgres
```

Coolify's managed Postgres container doesn't expose port 5433 to the host by default. Verify:

```bash
ssh gory "sudo ss -tlnp | grep :5433"
```

If port 5433 isn't listening, either:
- Add a port mapping in Coolify for the Postgres service, OR
- Use a different approach (SSH port-forward: `ssh -L 5433:<postgres-container>:5432 gory`), OR
- Run a local Postgres for dev and only hit the Coolify DB in production

---

## Files

- `Makefile` — build/deploy/logs/status/stats/envs/ship targets
- `gory_coolify_access_token.txt` — Coolify API token (gitignored)
- `.env` — local env vars pulled from Coolify via API
- `CLAUDE.md` — deployment architecture documented under "Deployment (gory)"
- `docs/plans/session-auth.md` — app-layer auth + webhook routing plan
