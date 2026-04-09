#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/macos/common.sh
source "$SCRIPT_DIR/common.sh"

setup_release_env

require_cmd security
require_cmd base64

[[ -n "${APPLE_CERTIFICATE_P12_BASE64:-}" ]] || fail "APPLE_CERTIFICATE_P12_BASE64 is required"
[[ -n "${APPLE_CERTIFICATE_PASSWORD:-}" ]] || fail "APPLE_CERTIFICATE_PASSWORD is required"

keychain_name="${APPLE_KEYCHAIN_NAME:-build.keychain-db}"
keychain_password="${APPLE_KEYCHAIN_PASSWORD:-}"

if [[ -z "$keychain_password" ]]; then
  keychain_password="$(uuidgen)"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

cert_path="$tmp_dir/signing-cert.p12"
printf '%s' "$APPLE_CERTIFICATE_P12_BASE64" | decode_base64_to_file "$cert_path"

security delete-keychain "$keychain_name" >/dev/null 2>&1 || true

security create-keychain -p "$keychain_password" "$keychain_name"
security set-keychain-settings -lut 21600 "$keychain_name"
security unlock-keychain -p "$keychain_password" "$keychain_name"
security import "$cert_path" \
  -k "$keychain_name" \
  -P "$APPLE_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign \
  -T /usr/bin/security
security set-key-partition-list \
  -S apple-tool:,apple:,codesign: \
  -s \
  -k "$keychain_password" \
  "$keychain_name"
security list-keychains -d user -s "$keychain_name" login.keychain-db
security default-keychain -d user -s "$keychain_name"
security find-identity -v -p codesigning "$keychain_name"

log "Imported signing identity into $keychain_name"
