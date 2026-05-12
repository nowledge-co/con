#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/macos/common.sh
source "$SCRIPT_DIR/common.sh"

setup_release_env

require_cmd codesign
require_cmd spctl
require_cmd xcrun

cli_binary="$CON_APP_BUNDLE_PATH/Contents/MacOS/con-cli"
if [[ ! -x "$cli_binary" ]]; then
  fail "con-cli missing from app bundle: $cli_binary"
fi

terminfo_dir="$CON_APP_BUNDLE_PATH/Contents/Resources/terminfo"
if [[ ! -d "$terminfo_dir" ]]; then
  fail "Ghostty terminfo directory missing from app bundle: $terminfo_dir"
fi
terminfo_entry="$(find "$terminfo_dir" -type f -name xterm-ghostty -print -quit)"
if [[ -z "$terminfo_entry" || ! -r "$terminfo_entry" ]]; then
  fail "Ghostty xterm-ghostty terminfo entry missing from app bundle: $terminfo_dir"
fi

log "Verifying code signature for $CON_APP_BUNDLE_PATH"
codesign --verify --deep --strict --verbose=2 "$CON_APP_BUNDLE_PATH"

if [[ "${CON_ALLOW_ADHOC_SIGNING:-0}" == "1" ]]; then
  log "Skipping Gatekeeper verification because ad-hoc signing is enabled"
else
  spctl -a -vv --type exec "$CON_APP_BUNDLE_PATH"
fi

if [[ -f "$CON_DMG_PATH" ]]; then
  log "Verifying DMG signature for $CON_DMG_PATH"
  codesign --verify --verbose=2 "$CON_DMG_PATH"
fi

if [[ "${CON_ALLOW_ADHOC_SIGNING:-0}" != "1" && "${CON_SKIP_NOTARIZATION:-0}" != "1" ]] && have_notary_credentials; then
  xcrun stapler validate "$CON_APP_BUNDLE_PATH"
  xcrun stapler validate "$CON_DMG_PATH"
fi

log "Verification complete"
