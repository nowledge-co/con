#!/usr/bin/env bash
#
# Final release gate.
#
# This script runs from release-finalize.yml after the macOS, Linux, and
# Windows release workflows all reported success but before the draft release
# is made public. It verifies the public contract that fresh installers and
# in-app updaters rely on:
#
#   - every expected GitHub Release asset exists and is non-empty
#   - checksum files reference the artifacts they are supposed to protect
#   - stable/beta appcasts point at the just-built assets
#   - gh-pages carries the installer scripts used by fresh installs / updates
#
# If any of those invariants fail, the draft stays private.

set -euo pipefail

fail() {
  printf '[release-gate] ERROR: %s\n' "$*" >&2
  exit 1
}

log() {
  printf '[release-gate] %s\n' "$*"
}

tag="${1:?usage: verify-release-gate.sh <tag> <gh-pages-dir>}"
pages_dir="${2:?usage: verify-release-gate.sh <tag> <gh-pages-dir>}"
repo="${GH_REPO:-${GITHUB_REPOSITORY:-}}"
[[ -n "$repo" ]] || fail "GH_REPO or GITHUB_REPOSITORY is required"

version="${tag#v}"
channel="stable"
case "$version" in
  *-beta.*) channel="beta" ;;
  *-dev.*) channel="dev" ;;
esac

mac_prefix="con"
if [[ "$channel" == "beta" ]]; then
  mac_prefix="con-Beta"
fi

required_assets=(
  "${mac_prefix}-${version}-macos-arm64.dmg"
  "${mac_prefix}-${version}-macos-arm64.zip"
  "SHA256SUMS-macos-arm64.txt"
  "${mac_prefix}-${version}-macos-x86_64.dmg"
  "${mac_prefix}-${version}-macos-x86_64.zip"
  "SHA256SUMS-macos-x86_64.txt"
  "con-${version}-linux-x86_64.tar.gz"
  "SHA256SUMS-linux.txt"
  "con-${version}-windows-x86_64.zip"
  "SHA256SUMS-windows.txt"
)

log "checking release assets for $tag"
release_json="$(gh release view "$tag" --json isDraft,assets)"
is_draft="$(jq -r '.isDraft' <<<"$release_json")"
[[ "$is_draft" == "true" ]] || fail "$tag is not a draft; refusing to run final gate"

for asset in "${required_assets[@]}"; do
  size="$(jq -r --arg name "$asset" '.assets[] | select(.name == $name) | .size' <<<"$release_json" | head -1)"
  [[ -n "$size" && "$size" != "null" ]] || fail "missing release asset: $asset"
  [[ "$size" -gt 0 ]] || fail "empty release asset: $asset"
  log "asset OK: $asset (${size} bytes)"
done

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

log "checking checksum manifests"
gh release download "$tag" \
  --pattern "SHA256SUMS-macos-arm64.txt" \
  --pattern "SHA256SUMS-macos-x86_64.txt" \
  --pattern "SHA256SUMS-linux.txt" \
  --pattern "SHA256SUMS-windows.txt" \
  --dir "$tmp" >/dev/null

grep -F "${mac_prefix}-${version}-macos-arm64" "$tmp/SHA256SUMS-macos-arm64.txt" >/dev/null \
  || fail "SHA256SUMS-macos-arm64.txt does not reference arm64 macOS artifacts"
grep -F "${mac_prefix}-${version}-macos-x86_64" "$tmp/SHA256SUMS-macos-x86_64.txt" >/dev/null \
  || fail "SHA256SUMS-macos-x86_64.txt does not reference x86_64 macOS artifacts"
grep -F "con-${version}-linux-x86_64.tar.gz" "$tmp/SHA256SUMS-linux.txt" >/dev/null \
  || fail "SHA256SUMS-linux.txt does not reference the Linux tarball"
grep -F "con-${version}-windows-x86_64.zip" "$tmp/SHA256SUMS-windows.txt" >/dev/null \
  || fail "SHA256SUMS-windows.txt does not reference the Windows ZIP"

[[ -f "$pages_dir/install.sh" ]] || fail "gh-pages install.sh missing"
[[ -f "$pages_dir/install.ps1" ]] || fail "gh-pages install.ps1 missing"
grep -F "con-cli" "$pages_dir/install.sh" >/dev/null \
  || fail "gh-pages install.sh does not expose con-cli"
grep -F "con-cli.exe" "$pages_dir/install.ps1" >/dev/null \
  || fail "gh-pages install.ps1 does not expose con-cli.exe"

if [[ "$channel" == "dev" ]]; then
  log "dev tag: appcast promotion checks skipped"
  exit 0
fi

require_appcast() {
  local file="$1"
  local asset="$2"
  local short_version="$3"

  [[ -f "$file" ]] || fail "missing appcast: $file"
  python3 - "$file" "$asset" "$short_version" <<'PYTHON'
import sys
import xml.etree.ElementTree as ET

file, asset, short_version = sys.argv[1:]
sparkle = "http://www.andymatuschak.org/xml-namespaces/sparkle"

try:
    root = ET.parse(file).getroot()
except Exception as err:
    raise SystemExit(f"{file} is not valid XML: {err}")

for item in root.findall("./channel/item"):
    short = item.find(f"{{{sparkle}}}shortVersionString")
    enclosure = item.find("enclosure")
    if short is None or short.text != short_version or enclosure is None:
        continue

    url = enclosure.get("url") or ""
    signature = enclosure.get(f"{{{sparkle}}}edSignature") or ""
    length = enclosure.get("length") or ""
    if asset not in url:
        continue
    if not signature:
        raise SystemExit(f"{file} item for {asset} is missing edSignature")
    if not length or int(length) <= 0:
        raise SystemExit(f"{file} item for {asset} is missing positive length")
    break
else:
    raise SystemExit(
        f"{file} does not contain {asset} with shortVersionString={short_version}"
    )
PYTHON

  log "appcast OK: $(basename "$file") -> $asset"
}

marketing_version="${version%%[-+]*}"

log "checking appcasts for $channel"
require_appcast \
  "$pages_dir/appcast/${channel}-macos-arm64.xml" \
  "${mac_prefix}-${version}-macos-arm64.dmg" \
  "$marketing_version"
require_appcast \
  "$pages_dir/appcast/${channel}-macos-x86_64.xml" \
  "${mac_prefix}-${version}-macos-x86_64.dmg" \
  "$marketing_version"
require_appcast \
  "$pages_dir/appcast/${channel}-linux-x86_64.xml" \
  "con-${version}-linux-x86_64.tar.gz" \
  "$version"
require_appcast \
  "$pages_dir/appcast/${channel}-windows-x86_64.xml" \
  "con-${version}-windows-x86_64.zip" \
  "$version"

log "release gate passed for $tag"
