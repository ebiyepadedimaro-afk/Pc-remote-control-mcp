#!/usr/bin/env bash
# Install the desktop app for the current user (no root):
#   ~/.local/share/pc-remote-control + an application-menu entry with the icon.
# Launch "PC Remote Control" from the app grid afterward. (`nsh run` shows the
# nsh dev host's gear icon; the packaged app shows the app icon.)
set -euo pipefail
cd "$(dirname "$0")/.."

OUT=$(mktemp -d)
trap 'rm -rf "$OUT"' EXIT
nsh package ./ui --target linux --out "$OUT"
APP=$(find "$OUT" -maxdepth 1 -type d -name "PC Remote Control" | head -1)
if [ -z "$APP" ]; then echo "packaging produced no app directory" >&2; exit 1; fi

DEST="$HOME/.local/share/pc-remote-control"
rm -rf "$DEST"
mkdir -p "$DEST"
cp -a "$APP"/. "$DEST/"

mkdir -p "$HOME/.local/share/applications" "$HOME/.local/share/icons/hicolor/1024x1024/apps"
cp "$DEST/com.pcrc.control.png" "$HOME/.local/share/icons/hicolor/1024x1024/apps/com.pcrc.control.png"

cat > "$HOME/.local/share/applications/com.pcrc.control.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=PC Remote Control
Comment=MCP remote-control desktop app
Exec=$DEST/PCRemoteControl
Icon=$DEST/com.pcrc.control.png
Terminal=false
Categories=Utility;Development;
StartupWMClass=com.pcrc.control
StartupNotify=true
DESKTOP

if command -v update-desktop-database >/dev/null; then
  update-desktop-database "$HOME/.local/share/applications" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null; then
  gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" >/dev/null 2>&1 || true
fi

printf 'installed: %s\n' "$DEST"
printf 'launch "PC Remote Control" from the application grid.\n'
