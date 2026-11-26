#!/usr/bin/env bash
# Build a .deb from an nsh-packaged desktop app directory + the server binary.
# Usage: scripts/build-deb.sh <version> <app-dir> <pcrc-server> [output.deb]
set -euo pipefail
VERSION="${1:?version, e.g. 0.1.4}"
APPDIR="${2:?nsh package directory (contains PCRemoteControl and nsh/)}"
SERVER="${3:?path to pcrc-server}"
OUT="${4:-dist/pc-remote-control_${VERSION}_amd64.deb}"

ROOT=$(mktemp -d)
trap 'rm -rf "$ROOT"' EXIT

mkdir -p "$ROOT/DEBIAN" "$ROOT/opt/pc-remote-control" "$ROOT/usr/bin" \
  "$ROOT/usr/share/applications" "$ROOT/usr/share/icons/hicolor/1024x1024/apps"
cp -a "$APPDIR"/. "$ROOT/opt/pc-remote-control/"
install -m 755 "$SERVER" "$ROOT/usr/bin/pcrc-server"
cat > "$ROOT/usr/bin/pc-remote-control" <<'LAUNCH'
#!/bin/sh
exec /opt/pc-remote-control/PCRemoteControl "$@"
LAUNCH
chmod 755 "$ROOT/usr/bin/pc-remote-control"

cat > "$ROOT/usr/share/applications/com.pcrc.control.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=PC Remote Control
Comment=Remote-control MCP server desktop app
Exec=/usr/bin/pc-remote-control
Icon=com.pcrc.control
Terminal=false
Categories=Utility;Development;
StartupWMClass=com.pcrc.control
DESKTOP

if [ -f "$APPDIR/com.pcrc.control.png" ]; then
  cp "$APPDIR/com.pcrc.control.png" "$ROOT/usr/share/icons/hicolor/1024x1024/apps/com.pcrc.control.png"
elif [ -f ui/assets/icon.png ]; then
  cp ui/assets/icon.png "$ROOT/usr/share/icons/hicolor/1024x1024/apps/com.pcrc.control.png"
fi

SIZE=$(du -sk "$ROOT" | cut -f1)
cat > "$ROOT/DEBIAN/control" <<CONTROL
Package: pc-remote-control
Version: ${VERSION}
Architecture: amd64
Maintainer: ebiyepadedimaro-afk <ebiyepadedimaro@gmail.com>
Installed-Size: ${SIZE}
Depends: libgtk-4-1, libxkbcommon0, libwayland-client0
Section: utils
Priority: optional
Homepage: https://github.com/ebiyepadedimaro-afk/Pc-remote-control-mcp
Description: PC Remote Control MCP server and desktop app
 A 54-tool MCP remote-control server for this PC (screen, input, files,
 shell, windows, processes, browser automation, documents, coding-agent
 CLIs, power/administration, remote-access tunnels) with an nsh desktop
 app. The app starts and supervises the server and shows its logs,
 settings and public tunnel URL.
CONTROL

mkdir -p "$(dirname "$OUT")"
dpkg-deb --build --root-owner-group "$ROOT" "$OUT"
echo "built $OUT"
