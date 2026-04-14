#!/usr/bin/env bash
#
# Download the official Sparkle framework release.
#
# Produces:
#   $SPARKLE_DIR/Sparkle.framework   — embed in .app/Contents/Frameworks/
#   $SPARKLE_DIR/bin/sign_update     — sign artifacts for appcast
#   $SPARKLE_DIR/bin/generate_appcast
#
# Environment:
#   SPARKLE_VERSION   — version to download (default: 2.7.5)
#   SPARKLE_DIR       — output directory (default: .sparkle)

set -euo pipefail

SPARKLE_VERSION="${SPARKLE_VERSION:-2.7.5}"
SPARKLE_DIR="${SPARKLE_DIR:-$(cd "$(dirname "$0")/../.." && pwd)/.sparkle}"

# SHA256 checksums of official Sparkle releases.
# Update this when bumping SPARKLE_VERSION.
declare -A SPARKLE_SHA256=(
  ["2.7.5"]="8ba2e3db6f0c4aa2fa62a31b3aa16e73e53c88c1a720a3c9fbffc9a87efab569"
)

expected_sha="${SPARKLE_SHA256[$SPARKLE_VERSION]:-}"

# Official Sparkle project: https://github.com/sparkle-project/Sparkle
url="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"

if [[ -d "$SPARKLE_DIR/Sparkle.framework" && -x "$SPARKLE_DIR/bin/sign_update" ]]; then
  # Verify the cached version matches what we want
  if [[ -f "$SPARKLE_DIR/.version" ]] && [[ "$(cat "$SPARKLE_DIR/.version")" == "$SPARKLE_VERSION" ]]; then
    echo "[sparkle] Already present at $SPARKLE_DIR (v$SPARKLE_VERSION)"
    exit 0
  fi
fi

echo "[sparkle] Downloading Sparkle $SPARKLE_VERSION from sparkle-project/Sparkle"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

curl -fSL "$url" -o "$tmp/Sparkle.tar.xz"

# Verify SHA256 if we have a known checksum
if [[ -n "$expected_sha" ]]; then
  actual_sha="$(shasum -a 256 "$tmp/Sparkle.tar.xz" | awk '{print $1}')"
  if [[ "$actual_sha" != "$expected_sha" ]]; then
    echo "[sparkle] ERROR: SHA256 mismatch" >&2
    echo "  expected: $expected_sha" >&2
    echo "  actual:   $actual_sha" >&2
    exit 1
  fi
  echo "[sparkle] SHA256 verified"
else
  echo "[sparkle] WARNING: no known SHA256 for version $SPARKLE_VERSION — skipping verification"
fi

mkdir -p "$tmp/extract"
tar -xf "$tmp/Sparkle.tar.xz" -C "$tmp/extract"

rm -rf "$SPARKLE_DIR"
mkdir -p "$SPARKLE_DIR/bin"

# Framework for embedding
cp -R "$tmp/extract/Sparkle.framework" "$SPARKLE_DIR/Sparkle.framework"

# CLI tools for CI
cp "$tmp/extract/bin/sign_update"      "$SPARKLE_DIR/bin/sign_update"
cp "$tmp/extract/bin/generate_appcast" "$SPARKLE_DIR/bin/generate_appcast"
chmod +x "$SPARKLE_DIR/bin/sign_update" "$SPARKLE_DIR/bin/generate_appcast"

# Record version so cache invalidation works
printf '%s' "$SPARKLE_VERSION" >"$SPARKLE_DIR/.version"

echo "[sparkle] Installed to $SPARKLE_DIR"
echo "  Framework: $SPARKLE_DIR/Sparkle.framework"
echo "  Tools:     $SPARKLE_DIR/bin/{sign_update,generate_appcast}"
