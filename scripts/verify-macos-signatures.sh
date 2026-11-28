#!/usr/bin/env bash
# Verify the Developer ID signature and notarization of a macOS release
# (control T-15). Run after `yarn tauri build` on macOS.
#
# A macOS build is not releasable until every .app is signed with a
# Developer ID Application certificate, hardened, and accepted by Gatekeeper,
# and every .dmg is signed, notarized and stapled.

set -euo pipefail

root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
bundle="$root/src-tauri/target/release/bundle"

apps=()
while IFS= read -r app; do
  apps+=("$app")
done < <(find "$bundle" -maxdepth 3 -type d -name '*.app' 2>/dev/null | sort)

if [ "${#apps[@]}" -eq 0 ]; then
  echo "No .app bundle found under $bundle; build first with 'yarn tauri build'." >&2
  exit 1
fi

for app in "${apps[@]}"; do
  echo "verifying $app"
  codesign --verify --deep --strict --verbose=2 "$app"

  identity="$(codesign -dv --verbose=4 "$app" 2>&1 | sed -n 's/^Authority=//p' | head -n 1)"
  case "$identity" in
    "Developer ID Application:"*) ;;
    *)
      echo "unexpected signing identity for $app: ${identity:-none} (T-15)" >&2
      exit 1
      ;;
  esac

  # Gatekeeper accepts a Developer ID app only after notarization.
  spctl --assess --type execute --verbose=4 "$app"
done

while IFS= read -r dmg; do
  echo "verifying $dmg"
  codesign --verify --verbose=2 "$dmg"
  spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg"
  xcrun stapler validate "$dmg"
done < <(find "$bundle" -maxdepth 3 -type f -name '*.dmg' 2>/dev/null | sort)

echo "All macOS bundles are signed, notarized and stapled."
