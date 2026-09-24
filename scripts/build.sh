#!/bin/zsh
# Builds OpenFlow.app (Tauri) with the Swift helper sidecar and optionally installs it to /Applications.
#   ./scripts/build.sh            build out/OpenFlow.app
#   ./scripts/build.sh --install  build, then replace /Applications/OpenFlow.app
set -euo pipefail
setopt null_glob
cd "$(dirname "$0")/.."

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')

# 1. Swift helper (FluidAudio: speech models, system audio tap, Contacts/Calendar names).
( cd sidecar/openflow-helper && swift build -c release 2>&1 | tail -3 )
HELPER_BUILD=sidecar/openflow-helper/.build/release
mkdir -p src-tauri/binaries
cp "$HELPER_BUILD/openflow-helper" "src-tauri/binaries/openflow-helper-$TRIPLE"

# 1b. Meeting speech server (transcribe.cpp on Metal). The first build fetches the pinned source and compiles ggml.
sidecar/transcribe-server/build.sh   # incremental after the first build
cp sidecar/transcribe-server/transcribe-server "src-tauri/binaries/transcribe-server-$TRIPLE"

# 2. Tauri app (frontend + Rust). Produces src-tauri/target or target/release/bundle/macos/OpenFlow.app.
pnpm install --frozen-lockfile >/dev/null
pnpm tauri build --bundles app

APP_SRC=target/release/bundle/macos/OpenFlow.app
APP=out/OpenFlow.app
rm -rf "$APP"; mkdir -p out
cp -R "$APP_SRC" "$APP"

# SwiftPM resource bundles (FluidAudio ships one) are found through Bundle.main.resourceURL.
for b in "$HELPER_BUILD"/*.bundle; do cp -R "$b" "$APP/Contents/Resources/"; done

# Identifier-based designated requirement so macOS keeps the Accessibility grant across rebuilds.
codesign --force --deep --sign - -r '=designated => identifier "com.himanshuat.openflow"' "$APP"
echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
  osascript -e 'quit app "OpenFlow"' 2>/dev/null || true   # a clean quit also stops llama-server
  sleep 1; pkill -x OpenFlow || true
  rm -rf /Applications/OpenFlow.app
  cp -R "$APP" /Applications/OpenFlow.app
  echo "Installed /Applications/OpenFlow.app"
fi
