#!/usr/bin/env sh
# Build the release server and install it on PATH for the desktop UI.
set -eu
cd "$(dirname "$0")/.."

cargo build --release -p pcrc-server
install -d "$HOME/.local/bin"
install -m 755 target/release/pcrc-server "$HOME/.local/bin/pcrc-server"

# Bundle the tunnel clients when they were fetched into ./vendor
if [ -d vendor ]; then
  for b in cloudflared ngrok tailscale tailscaled; do
    if [ -f "vendor/$b" ]; then
      install -m 755 "vendor/$b" "$HOME/.local/bin/$b"
    fi
  done
fi

printf 'installed: %s\n' "$HOME/.local/bin/pcrc-server"
printf 'ensure %s is on PATH, then run the desktop app:\n' "$HOME/.local/bin"
printf '  nsh run ./ui --target linux   # or macos / windows\n'
printf '\nor run the server headless:\n'
printf '  pcrc-server\n'
printf '\nfor the bundled tunnel clients: ./scripts/fetch-tunnel-clients.sh (then re-run install.sh)\n'
