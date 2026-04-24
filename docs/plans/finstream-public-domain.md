# Expose finstream at finstream.hirschi.dev

Put the app behind a public HTTPS hostname via the existing Cloudflare Tunnel, gate it with Cloudflare Access (identity at the edge), and remove the `8008:3000` host port mapping so Coolify can do rolling deploys.

## Motivation

Two things we want, one we want to avoid:

- **Want:** mobile access without fiddling with Tailscale / mDNS / `/etc/hosts`. iOS has no hosts file, so `finstream.gory` tricks don't work.
- **Want:** zero-downtime rolling deploys in Coolify. Today the app has `ports_mappings: "8008:3000"`, which forces Coolify to kill the old container before starting the new one ("Application has ports mapped to the host system, rolling update is not supported").
- **Avoid:** meaningfully increasing blast radius on the home LAN. `gory` sits on the same network as personal devices; no VLAN available.

The existing Cloudflare Tunnel already exposes `finstream-webhooks.hirschi.dev` publicly for Resend webhooks — we're extending that same pattern to the whole app, with an identity gate in front.

## Target architecture

```
Internet                        Cloudflare edge                       Home (gory, behind NAT)
────────                        ───────────────                       ──────────────────────

browser ──── HTTPS ────► finstream.hirschi.dev                        cloudflared (outbound tunnel)
                          │                                            │
                          ├── Cloudflare Access check ──┐              │
                          │   (allow email=drew@…       │              │
                          │    except /webhooks/*,      │              │
                          │    /healthz)                │              │
                          │                              │              │
                          ▼                              ▼              ▼
                       reject                         allow ──── tunnel ──► coolify-proxy (Traefik :80)
                                                                               │
                                                                               ▼
                                                                         app container :3000
```

Public hostnames after this lands:

- `finstream.hirschi.dev` — full app, gated by Cloudflare Access (identity = Drew) + app session auth
- `finstream-webhooks.hirschi.dev` — stays as-is for Resend. Could consolidate onto the main hostname's `/webhooks/*` bypass later, but no reason to touch it now.

Local hostnames that go away or change:

- `http://gory:8008` — **removed.** Direct host port mapping is what was blocking rolling deploys. Access the app via `finstream.hirschi.dev` from everywhere (including on-LAN and Tailscale — Cloudflare Access skips the identity check for sessions once you've logged in).

## Security model (without VLAN)

Defense stack, outer → inner:

1. **Cloudflare Tunnel.** No inbound ports on the home router. Only outbound TLS from `cloudflared` to CF edge. Same posture we have today for webhooks.
2. **Cloudflare Access.** Identity gate at the edge. Policy: `email == drew@enzo.health` → allow; everything else → block. Random internet traffic never even reaches the tunnel. Free tier covers this.
3. **Bypass carve-outs** for machine-to-machine endpoints that can't present a user identity:
   - `/webhooks/*` — Resend delivers these; verified by Svix signature in `src/routes/webhooks.rs:47-54`.
   - `/healthz`, `/health`, `/ready` — liveness/readiness probes; no secrets.
4. **App session auth.** Existing `tower-sessions` + Argon2id layer (`src/main.rs:57-67`). Second factor behind CF Access.
5. **Container hardening.** Already running non-root as uid 10001 (`Dockerfile:39-42`). Leave as-is.

### Threat model (explicit)

| Attacker capability | Defense | Outcome |
|---|---|---|
| Internet scanner hits `finstream.hirschi.dev` | CF Access login wall | Never sees the app |
| Phishes/steals drew's CF identity | App session auth (password) | Needs to also crack/steal session |
| Finds RCE in Rust app | Container uid 10001, dropped caps | Contained to container (no root) |
| Escapes container to gory | *No VLAN* | Lands on home LAN — accepted risk |

The unmitigated risk is container-escape-to-LAN. VLAN is the only defense and is explicitly out of scope (hardware cost). Probability is low enough for a personal app.

## Investigation findings

### Cloudflare tunnel

- Container `cloudflared-t7djtak7o7rdwrmysu3slj9i` runs `cloudflare/cloudflared:latest`, command `tunnel --no-autoupdate run --token …`. Tunnel ID `dc408b55-4b2c-4460-aa5f-a389da1865c8`.
- Routing config lives in the Cloudflare dashboard, not a local `config.yml`. Means new hostnames are added in the CF dashboard (Zero Trust → Networks → Tunnels → [tunnel] → Public Hostname), not by editing a file on gory.

### Coolify app

- UUID `qx36dh9sz8wqauggabhki4h3`, name `app-main`.
- `fqdn: "http://finstream-webhooks.hirschi.dev"` — Coolify auto-generates Traefik labels routing `Host(finstream-webhooks.hirschi.dev)` on port 80 to container port 3000.
- `ports_mappings: "8008:3000"` — the host port binding. This is what blocks rolling deploys.
- `ports_exposes: "3000"` — container port, stays the same.

### App public routes

From `src/main.rs:48-55`, three categories stay reachable without a session:

- `routes::health::router()` — `/health`, `/healthz`, `/ready`, `/bench/render`
- `routes::webhooks::router()` — `/webhooks/resend` (Svix-verified)
- `routes::auth::router()` — login/logout pages

Cloudflare Access bypass list must cover `/webhooks/*` (mandatory — external caller). `/healthz` is nice to bypass for uptime monitors but not required. Login/auth routes stay behind CF Access — CF Access *is* the primary login; the app's password page then runs behind it.

### Session cookie posture

`src/auth.rs` / `src/main.rs:41-46` — `Secure` flag on the cookie is controlled by `SESSION_COOKIE_SECURE` env var. Currently likely unset or `false` because Tailscale hostnames don't terminate TLS. Under `finstream.hirschi.dev` the browser sees HTTPS (CF terminates TLS), so **we must set `SESSION_COOKIE_SECURE=true`** or the Secure-cookie mismatch will cause auth weirdness.

## Implementation

One PR, in this order. Each step is independently reversible up until step 5.

### 1. Add the CNAME and Cloudflare Access application

In Cloudflare dashboard:
- Zero Trust → Networks → Tunnels → select the existing tunnel → Public Hostnames → **Add**:
  - Subdomain: `finstream`
  - Domain: `hirschi.dev`
  - Service: `http://coolify-proxy:80` (the Coolify Traefik service on the internal Docker network — same target the webhook hostname uses)
- Zero Trust → Access → Applications → **Add self-hosted application**:
  - Name: `finstream`
  - Subdomain: `finstream`, Domain: `hirschi.dev`, Path: blank (protect everything)
  - Identity provider: one-time PIN to `drew@enzo.health` (simplest; upgrade to Google/GitHub later if desired)
  - Policy: allow include `email == drew@enzo.health`
- Zero Trust → Access → Applications → **Add bypass application** for the same hostname:
  - Path: `/webhooks/*`
  - Policy: bypass (allow everyone, no identity required)
- Optionally repeat the bypass for `/healthz` if you want external uptime monitoring.

At this point `finstream.hirschi.dev` resolves and shows a CF login prompt. The app isn't receiving traffic yet because Traefik has no `Host(finstream.hirschi.dev)` rule — next step.

### 2. Add the hostname to Coolify

Edit `app-main` in the Coolify UI (or via API):
- Add `http://finstream.hirschi.dev` to the Domains field. Coolify supports comma-separated FQDNs and will generate additional Traefik labels.
- Leave `finstream-webhooks.hirschi.dev` in place for now.
- Save. Coolify will redeploy and emit new Traefik labels.

Verify: `curl -H 'Cf-Access-Jwt-Assertion: …' https://finstream.hirschi.dev/healthz` should return `ok` (or check from a browser after CF Access login).

### 3. Set `SESSION_COOKIE_SECURE=true` in Coolify env vars

In the Coolify app's Environment Variables tab, add or update:
- `SESSION_COOKIE_SECURE=true`

Deploy. Log in via `finstream.hirschi.dev` — cookie inspector should show `Secure` flag.

### 4. Remove the host port mapping (unlocks rolling deploys)

In the Coolify UI: edit `app-main`, clear `Ports Mappings` (currently `8008:3000`). Save.

Side effect: `http://gory:8008` and `http://gory.local:8008` stop working. This is intentional — all access now flows through `finstream.hirschi.dev`.

Deploy. Confirm Coolify no longer prints the "ports mapped to the host system, rolling update is not supported" message; next deploy should do a rolling swap with the `/ready` health check gating cutover.

### 5. Consolidate webhooks (optional, later)

`finstream-webhooks.hirschi.dev` is redundant once `finstream.hirschi.dev/webhooks/resend` works. Migration path:
- Update Resend's webhook URL in the Resend dashboard to `https://finstream.hirschi.dev/webhooks/resend`.
- Confirm one webhook is received successfully.
- Remove `finstream-webhooks.hirschi.dev` from the tunnel's public hostnames and from Coolify's FQDN field.

Skip this step for now unless you want the cleanup; it's purely cosmetic.

## Validation

After step 4, run through this checklist:

- [ ] `https://finstream.hirschi.dev` from a clean browser → CF Access login page
- [ ] After CF Access login → app login page
- [ ] After app login → dashboard loads
- [ ] Cookie on the app session is marked `Secure` (inspect in devtools)
- [ ] From a phone (cellular, no VPN) → same flow works
- [ ] `curl https://finstream.hirschi.dev/healthz` → `ok` (no auth)
- [ ] `curl -X POST https://finstream.hirschi.dev/webhooks/resend` → 401 from app (because no Svix signature), **not** from Cloudflare Access
- [ ] Trigger a Coolify redeploy → deployment logs no longer include "ports mapped to the host system"; new container runs alongside old while `/ready` is probed

## Rollback

Each step reverses independently:

- Step 1: delete the CF Access application and the tunnel public hostname. Free.
- Step 2: remove the domain from Coolify's FQDN list. Traefik routes drop on next save.
- Step 3: delete the env var. Old cookies remain valid until expiry.
- Step 4: re-add `ports_mappings: "8008:3000"` in Coolify. LAN/Tailscale direct access returns.

## Out of scope

- VLAN / network segmentation for the home LAN (requires hardware).
- WAF rules, bot blocking, geo restrictions at Cloudflare (marginal value once CF Access is in place).
- Replacing Svix signature with mutual TLS for webhooks.
- Multi-user access (current CF Access policy is single-user).
