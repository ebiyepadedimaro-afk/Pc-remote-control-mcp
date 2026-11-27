# Security model

PC Remote Control is a remote-control server by design: an authorized MCP client can move the mouse, type, run
commands and manage the machine. That is the product. Everything below describes how access is controlled and what
the boundaries are.

## Access control

- **Loopback by default.** The server binds `127.0.0.1` unless you change `bind`. Remote access only exists through
  a tunnel you start (ngrok, cloudflared, Tailscale).
- **Always-approve vs token.** With `token_enabled: false` every client that can reach the server is approved — the
  product's default mode. With `token_enabled: true`, requests must send `Authorization: Bearer <token>` (or
  `X-API-Key`); tokens are compared in constant time. Regenerate from the app.
- **Tunnel gate.** No remote tunnel may start while the token is disabled (cloudflared/ngrok/Tailscale alike).
- **Admin API is loopback-only** and is what the desktop app uses.

## Tool boundaries

- **Per-tool and per-group switches.** Every tool (and whole groups) can be disabled in the app; disabled tools are
  hidden from `tools/list` and refused.
- **File tools** are confined to `files.root` when set, refuse writes/deletes when disabled, cap read/write sizes,
  and never touch the folders listed in `files.protected` (`~/.ssh`, `~/.aws`, `~/.gnupg`, keychains, …) — even if
  the root covers the whole filesystem. Downloads block private addresses unless allowed and cap size/time.
- **Shell** runs as the user with a configurable timeout, output caps and a blocklist. This is a remote-control
  feature by design; disable it (`shell.enabled: false`) if you do not want it.
- **Tunnels** require the bearer token (cloudflared/ngrok) and are disabled entirely via `system.tunnel_enabled`.
- **Power/administration** (shutdown, elevated commands, services, terminal) each have an independent switch.

## Tunnel boundaries

- cloudflared quick tunnels and ngrok expose the server **publicly** — the bearer token is the only gate, so keep it
  long/random and rotate it if leaked.
- Tailscale Serve is tailnet-only. With tailnet HTTPS certificates disabled the server falls back to HTTP on port 80
  inside the tailnet; enable HTTPS certificates in the admin console for TLS.
- The Cloudflare "Your Domain" mode uses a dashboard-managed tunnel token stored in the config file (mode `0600`).

## Incident handling

- Regenerate the bearer token (`/admin/token` or the app) — connected clients stop working immediately.
- Stop the active tunnel from the app (Remote access → Stop) or quit the app; quitting always stops serving.
- Logs (last N lines) are available in the app's Logs page and via `/admin/logs`.
