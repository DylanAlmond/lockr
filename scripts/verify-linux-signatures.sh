#!/usr/bin/env bash
# Verify the Linux release assets (control T-15).
#
# Checks the detached signature over SHA256SUMS and the embedded AppImage
# signature. Run after scripts/sign-linux-assets.sh.

set -euo pipefail

root="${1:-$(cd "$(dirname "$0")/.." && pwd)}"
bundle="$root/src-tauri/target/release/bundle"
cd "$bundle"

if [ ! -f SHA256SUMS ] || [ ! -f SHA256SUMS.asc ]; then
  echo "SHA256SUMS and SHA256SUMS.asc are missing; run scripts/sign-linux-assets.sh." >&2
  exit 1
fi

sha256sum --check SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS

appimage="$(find . -maxdepth 3 -type f -name '*.AppImage' | sort | head -n 1)"
if [ -z "$appimage" ]; then
  echo "No AppImage found under $bundle" >&2
  exit 1
fi

signature="$("$appimage" --appimage-signature 2>/dev/null || true)"
if [ -z "$signature" ] || grep -qi 'not signed' <<<"$signature"; then
  echo "The AppImage has no embedded signature (T-15)" >&2
  exit 1
fi

echo "Linux assets verified: SHA-256 manifest, detached GPG signature and AppImage signature."
