#!/usr/bin/env bash
# Run tailscaled (Tailscale's daemon) as a systemd *user* service in userspace
# networking mode: no root, no TUN. Log in once with:
#   tailscale --socket ~/.local/share/pcrc-tailscale/tailscaled.sock up --hostname <name>
# Then set tunnel.tailscale_socket to that socket path in the server config.
set -euo pipefail
SOCK="$HOME/.local/share/pcrc-tailscale/tailscaled.sock"
mkdir -p "$HOME/.local/share/pcrc-tailscale" "$HOME/.config/systemd/user"

cat > "$HOME/.config/systemd/user/pcrc-tailscaled.service" <<UNIT
[Unit]
Description=PC Remote Control - Tailscale userspace daemon
After=network-online.target

[Service]
ExecStart=%h/.local/bin/tailscaled --tun=userspace-networking --socket=%h/.local/share/pcrc-tailscale/tailscaled.sock --state=%h/.local/share/pcrc-tailscale/tailscaled.state --port=0 --socks5-server=127.0.0.1:1055
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
UNIT

systemctl --user daemon-reload
systemctl --user enable --now pcrc-tailscaled.service
sleep 2
systemctl --user --no-pager status pcrc-tailscaled.service | head -8 || true
printf '\nsocket: %s\n' "$SOCK"
