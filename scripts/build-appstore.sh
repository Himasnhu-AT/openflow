#!/bin/zsh
# Mac App Store build of OpenFlow: sandboxed app, signed for distribution, packaged as a .pkg for App Store Connect.
#
#   ./scripts/build-appstore.sh --local     sandboxed build signed ad hoc, to run and test on this Mac
#   ./scripts/build-appstore.sh             signed .pkg for App Store Connect (needs the variables below)
#   ./scripts/build-appstore.sh --upload    ... then validate and upload it
#
# Release variables (see docs/APP_STORE.md):
#   APPLE_TEAM_ID            10-character Team ID
#   APP_SIGN_IDENTITY        "Apple Distribution: Your Name (TEAMID)"
#   INSTALLER_SIGN_IDENTITY  "3rd Party Mac Developer Installer: Your Name (TEAMID)"
#   PROVISIONING_PROFILE     path to the Mac App Store .provisionprofile for com.himanshuat.openflow
#   BUILD_NUMBER             CFBundleVersion; must grow with every upload (default: git commit count)
#   ASC_KEY_ID, ASC_ISSUER_ID  App Store Connect API key (for --upload; the .p8 goes in ~/.appstoreconnect/private_keys)
set -euo pipefail
setopt null_glob
cd "$(dirname "$0")/.."

MODE=release
case "${1:-}" in
  --local) MODE=local ;;
  --upload) MODE=upload ;;
  "") ;;
  *) echo "usage: $0 [--local|--upload]" >&2; exit 2 ;;
esac

BUNDLE_ID=com.himanshuat.openflow
BUILD_NUMBER="${BUILD_NUMBER:-$(git rev-list --count HEAD)}"
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
OUT=out/appstore
APP="$OUT/OpenFlow.app"
ENT_DIR=src-tauri/appstore

if [[ $MODE != local ]]; then
  for v in APPLE_TEAM_ID APP_SIGN_IDENTITY INSTALLER_SIGN_IDENTITY PROVISIONING_PROFILE; do
    [[ -n "${(P)v:-}" ]] || { echo "$v is not set (see docs/APP_STORE.md)" >&2; exit 2; }
  done
  [[ -f "$PROVISIONING_PROFILE" ]] || { echo "No provisioning profile at $PROVISIONING_PROFILE" >&2; exit 2; }
  security find-identity -v | grep -q "$APP_SIGN_IDENTITY" || { echo "Signing identity not in the keychain: $APP_SIGN_IDENTITY" >&2; exit 2; }
fi

# 1. Helpers: the Swift speech/audio helper and the meeting speech server.
( cd sidecar/openflow-helper && swift build -c release 2>&1 | tail -1 )
sidecar/transcribe-server/build.sh
mkdir -p src-tauri/binaries
cp sidecar/openflow-helper/.build/release/openflow-helper "src-tauri/binaries/openflow-helper-$TRIPLE"
cp sidecar/transcribe-server/transcribe-server "src-tauri/binaries/transcribe-server-$TRIPLE"

# 2. The app, with this upload's build number.
pnpm install --frozen-lockfile >/dev/null
pnpm tauri build --bundles app --config "{\"bundle\":{\"macOS\":{\"bundleVersion\":\"$BUILD_NUMBER\"}}}"
rm -rf "$OUT"; mkdir -p "$OUT"
cp -R target/release/bundle/macos/OpenFlow.app "$APP"
for b in sidecar/openflow-helper/.build/release/*.bundle; do cp -R "$b" "$APP/Contents/Resources/"; done
[[ -f "$APP/Contents/Resources/PrivacyInfo.xcprivacy" ]] || { echo "PrivacyInfo.xcprivacy missing from the bundle" >&2; exit 1; }

# 3. Entitlements: the sandbox set, plus the team/app identifiers the provisioning profile grants.
APP_ENT="$OUT/OpenFlow.entitlements"
cp "$ENT_DIR/OpenFlow.entitlements" "$APP_ENT"
if [[ $MODE != local ]]; then
  /usr/libexec/PlistBuddy -c "Add :com.apple.application-identifier string $APPLE_TEAM_ID.$BUNDLE_ID" "$APP_ENT"
  /usr/libexec/PlistBuddy -c "Add :com.apple.developer.team-identifier string $APPLE_TEAM_ID" "$APP_ENT"
  cp "$PROVISIONING_PROFILE" "$APP/Contents/embedded.provisionprofile"
  IDENTITY="$APP_SIGN_IDENTITY"
else
  IDENTITY=-
fi

# 4. Sign inside out: helpers inherit the app's sandbox, then the app itself.
for BIN in openflow-helper transcribe-server; do
  codesign --force --sign "$IDENTITY" --identifier "$BUNDLE_ID.$BIN" \
    --entitlements "$ENT_DIR/Helper.entitlements" "$APP/Contents/MacOS/$BIN"
done
codesign --force --sign "$IDENTITY" --entitlements "$APP_ENT" \
  -r "=designated => identifier \"$BUNDLE_ID\"" "$APP"

# 5. Checks App Review would otherwise fail on.
codesign --verify --deep --strict "$APP"
codesign -d --entitlements - --xml "$APP" 2>/dev/null | grep -q "com.apple.security.app-sandbox" || { echo "app is not sandboxed" >&2; exit 1; }
for BIN in openflow-helper transcribe-server; do
  codesign -d --entitlements - --xml "$APP/Contents/MacOS/$BIN" 2>/dev/null | grep -q "com.apple.security.inherit" ||
    { echo "$BIN does not inherit the sandbox" >&2; exit 1; }
done
if grep -q "macOSPrivateApi\": true" src-tauri/tauri.conf.json; then echo "private macOS API is enabled" >&2; exit 1; fi
# (No --selftest on the signed helpers here: a binary that inherits the sandbox traps when started on its own.
# transcribe-server/build.sh already ran it before signing.)
echo "Signed $APP (build $BUILD_NUMBER, $MODE)"

[[ $MODE == local ]] && exit 0

# 6. Installer package for App Store Connect.
PKG="$OUT/OpenFlow-$BUILD_NUMBER.pkg"
productbuild --component "$APP" /Applications --sign "$INSTALLER_SIGN_IDENTITY" "$PKG"
echo "Built $PKG"

if [[ $MODE == upload ]]; then
  [[ -n "${ASC_KEY_ID:-}" && -n "${ASC_ISSUER_ID:-}" ]] || { echo "ASC_KEY_ID and ASC_ISSUER_ID are needed to upload" >&2; exit 2; }
  xcrun altool --validate-app -f "$PKG" -t macos --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  xcrun altool --upload-app -f "$PKG" -t macos --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER_ID"
  echo "Uploaded build $BUILD_NUMBER. It appears in App Store Connect › TestFlight after processing."
fi
