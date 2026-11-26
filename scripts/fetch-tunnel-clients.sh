#!/usr/bin/env sh
# Fetch the official tunnel clients for this machine into ./vendor, so a
# distribution folder can ship them next to pcrc-server (the server looks
# beside its own executable first, then PATH).
set -u
cd "$(dirname "$0")/.."
mkdir -p vendor

OS=$(uname -s)
ARCH=$(uname -m)
case "$OS" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) echo "unsupported OS: $OS (fetch the clients manually for Windows)" >&2; exit 1 ;;
esac
case "$ARCH" in
  x86_64|amd64) arch=amd64 ;;
  aarch64|arm64) arch=arm64 ;;
  *) echo "unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

# cloudflared: free quick tunnels, no account
if [ ! -x vendor/cloudflared ]; then
  echo "fetching cloudflared..."
  if curl -fL --retry 2 --max-time 180 -o vendor/cloudflared \
      "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-${os}-${arch}" \
      && chmod +x vendor/cloudflared && ./vendor/cloudflared --version >/dev/null 2>&1; then
    echo "cloudflared fetched"
  else
    rm -f vendor/cloudflared
    echo "warning: cloudflared could not be fetched (network); skipping" >&2
  fi
fi

# ngrok agent
if [ ! -x vendor/ngrok ]; then
  echo "fetching ngrok..."
  tmp=$(mktemp -d)
  if curl -fL --retry 2 --max-time 180 -o "$tmp/ngrok.tgz" \
      "https://bin.equinox.io/c/bNyj1mQVY4c/ngrok-v3-stable-${os}-${arch}.tgz" \
      && tar -xzf "$tmp/ngrok.tgz" -C "$tmp"; then
    install -m 755 "$tmp/ngrok" vendor/ngrok
  else
    echo "warning: ngrok could not be fetched; skipping" >&2
  fi
  rm -rf "$tmp"
fi

# tailscale + tailscaled (runs in userspace mode without root)
if [ ! -x vendor/tailscale ] || [ ! -x vendor/tailscaled ]; then
  echo "fetching tailscale..."
  tmp=$(mktemp -d)
  index=$(curl -fsSL --max-time 60 https://pkgs.tailscale.com/stable/ || true)
  file=$(printf '%s' "$index" | grep -oE "tailscale_[0-9.]+_${arch}\.tgz" | head -1 || true)
  if [ -z "$file" ]; then
    echo "warning: could not resolve the Tailscale stable tarball; skipping" >&2
  elif curl -fL --retry 2 --max-time 240 -o "$tmp/ts.tgz" "https://pkgs.tailscale.com/stable/${file}" \
      && tar -xzf "$tmp/ts.tgz" -C "$tmp"; then
    install -m 755 "$tmp"/tailscale_*/tailscale vendor/tailscale
    install -m 755 "$tmp"/tailscale_*/tailscaled vendor/tailscaled
  else
    echo "warning: tailscale could not be fetched; skipping" >&2
  fi
  rm -rf "$tmp"
fi

echo
./vendor/cloudflared --version 2>/dev/null || true
./vendor/ngrok --version 2>/dev/null || true
./vendor/tailscale version 2>/dev/null | head -2 || true
printf '\ntunnel clients are in %s/vendor\n' "$PWD"
