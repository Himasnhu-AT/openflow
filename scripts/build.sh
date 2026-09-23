#!/bin/zsh
# Builds JezFlow.app (Tauri) with the Swift helper sidecar and optionally installs it to /Applications.
#   ./scripts/build.sh            build out/JezFlow.app
#   ./scripts/build.sh --install  build, then replace /Applications/JezFlow.app
set -euo pipefail
setopt null_glob
cd "$(dirname "$0")/.."

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')

# 1. Swift helper (FluidAudio: speech models, system audio tap, Contacts/Calendar names).
( cd sidecar/jezflow-helper && swift build -c release 2>&1 | tail -3 )
HELPER_BUILD=sidecar/jezflow-helper/.build/release
mkdir -p src-tauri/binaries
cp "$HELPER_BUILD/jezflow-helper" "src-tauri/binaries/jezflow-helper-$TRIPLE"

# 2. Tauri app (frontend + Rust). Produces src-tauri/target or target/release/bundle/macos/JezFlow.app.
pnpm install --frozen-lockfile >/dev/null
pnpm tauri build --bundles app

APP_SRC=target/release/bundle/macos/JezFlow.app
APP=out/JezFlow.app
rm -rf "$APP"; mkdir -p out
cp -R "$APP_SRC" "$APP"

# SwiftPM resource bundles (FluidAudio ships one) are found through Bundle.main.resourceURL.
for b in "$HELPER_BUILD"/*.bundle; do cp -R "$b" "$APP/Contents/Resources/"; done

# Identifier-based designated requirement so macOS keeps the Accessibility grant across rebuilds.
codesign --force --deep --sign - -r '=designated => identifier "com.jez.flow"' "$APP"
echo "Built $APP"

if [[ "${1:-}" == "--install" ]]; then
  osascript -e 'quit app "JezFlow"' 2>/dev/null || true   # a clean quit also stops llama-server
  sleep 1; pkill -x JezFlow || true
  rm -rf /Applications/JezFlow.app
  cp -R "$APP" /Applications/JezFlow.app
  echo "Installed /Applications/JezFlow.app"
fi
