#!/usr/bin/env bash
# Compile the macOS 26 "Liquid Glass" app icon into an Assets.car that the app
# bundle ships. On macOS 26+ the system then renders the glass/specular sheen
# from this asset; the classic .icns (built by build-app-icon.sh) stays as the
# fallback for macOS 15 (Sequoia) and earlier.
#
# Author the icon in Apple's Icon Composer and save it in this crate as
#   crates/hitch-desktop/App.icon
# Requires Xcode 26+ (for actool's .icon compiler).
set -euo pipefail

CRATE_DIR="$(cd "$(dirname "$0")/.." && pwd)" # crates/hitch-desktop
ICON="$CRATE_DIR/App.icon"
OUT_DIR="$CRATE_DIR/src-tauri/gen/liquid-glass"
mkdir -p "$OUT_DIR"

if [ ! -e "$ICON" ]; then
  echo "error: $ICON not found." >&2
  echo "Author it in Icon Composer and save it as App.icon in $CRATE_DIR." >&2
  exit 1
fi

ACTOOL="$(xcrun --find actool)"
echo "Compiling $ICON with actool…"
"$ACTOOL" "$ICON" \
  --compile "$OUT_DIR" \
  --app-icon App \
  --include-all-app-icons \
  --output-partial-info-plist "$OUT_DIR/partial-info.plist" \
  --enable-on-demand-resources NO \
  --development-region en \
  --target-device mac \
  --minimum-deployment-target 26.0 \
  --platform macosx \
  --output-format human-readable-text --notices --warnings --errors

# Stage the compiled catalog where the Tauri bundler picks it up as a resource
# (it lands at Contents/Resources/Assets.car, the location macOS expects).
cp "$OUT_DIR/Assets.car" "$CRATE_DIR/src-tauri/Assets.car"

echo
echo "Wrote $CRATE_DIR/src-tauri/Assets.car"
echo "The bundle config ships this and sets CFBundleIconName=App (macOS 26+)."
