#!/usr/bin/env bash
#
# Download the official Sparkle framework release.
#
# Produces:
#   $SPARKLE_DIR/Sparkle.framework   — embed in .app/Contents/Frameworks/
#   $SPARKLE_DIR/bin/sign_update     — sign artifacts for appcast
#   $SPARKLE_DIR/bin/generate_appcast
#   $SPARKLE_DIR/bin/generate_keys
#
# Environment:
#   SPARKLE_VERSION   — version to download (default: 2.9.1)
#   SPARKLE_DIR       — output directory (default: .sparkle)

set -euo pipefail

SPARKLE_VERSION="${SPARKLE_VERSION:-2.9.1}"
SPARKLE_DIR="${SPARKLE_DIR:-$(cd "$(dirname "$0")/../.." && pwd)/.sparkle}"

# SHA256 checksums of official Sparkle releases (sparkle-project/Sparkle).
# Update this when bumping SPARKLE_VERSION.
sparkle_sha256() {
  case "$1" in
    2.9.1) echo "c0dde519fd2a43ddfc6a1eb76aec284d7d888fe281414f9177de3164d98ba4c7" ;;
    *)     echo "" ;;
  esac
}

expected_sha="$(sparkle_sha256 "$SPARKLE_VERSION")"

# Official Sparkle project: https://github.com/sparkle-project/Sparkle
url="https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_VERSION}/Sparkle-${SPARKLE_VERSION}.tar.xz"

if [[ -d "$SPARKLE_DIR/Sparkle.framework" && -x "$SPARKLE_DIR/bin/sign_update" ]]; then
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
for tool in sign_update generate_appcast generate_keys; do
  if [[ -f "$tmp/extract/bin/$tool" ]]; then
    cp "$tmp/extract/bin/$tool" "$SPARKLE_DIR/bin/$tool"
    chmod +x "$SPARKLE_DIR/bin/$tool"
  fi
done

# Record version so cache invalidation works
printf '%s' "$SPARKLE_VERSION" >"$SPARKLE_DIR/.version"

echo "[sparkle] Installed to $SPARKLE_DIR"
echo "  Framework: $SPARKLE_DIR/Sparkle.framework"
echo "  Tools:     $SPARKLE_DIR/bin/{sign_update,generate_appcast,generate_keys}"
