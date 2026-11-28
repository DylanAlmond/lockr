#!/usr/bin/env bash
# Sign the Linux release assets (control T-15).
#
# Tauri embeds a GPG signature in the AppImage (SIGN=1 in the workflow). This
# script adds a detached signature over a SHA-256 manifest covering every
# Linux bundle, so a user can verify the .deb and .rpm too, and uploads the
# manifest, its signature and the public key to the draft release.
#
# Requires: the release key imported into GPG, GH_TOKEN, GPG_KEY_ID,
# GPG_PASSPHRASE.

set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
bundle="$root/src-tauri/target/release/bundle"
cd "$bundle"

mapfile -t assets < <(find . -maxdepth 3 -type f \
  \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) | sort)

if [ "${#assets[@]}" -eq 0 ]; then
  echo "No Linux bundles found under $bundle" >&2
  exit 1
fi

sha256sum "${assets[@]}" > SHA256SUMS
gpg --batch --yes --pinentry-mode loopback \
  --passphrase "$GPG_PASSPHRASE" \
  --local-user "$GPG_KEY_ID" \
  --detach-sign --armor SHA256SUMS
gpg --verify SHA256SUMS.asc SHA256SUMS
gpg --armor --export "$GPG_KEY_ID" > lockr-release-signing-key.asc

for asset in "${assets[@]}"; do
  echo "signed $(basename "$asset")"
done

version="$(node -p "require('$root/src-tauri/tauri.conf.json').version")"
tag="lockr-v$version"

gh release upload "$tag" \
  SHA256SUMS \
  SHA256SUMS.asc \
  lockr-release-signing-key.asc \
  --clobber
echo "uploaded SHA256SUMS, SHA256SUMS.asc and the public key to $tag"
