#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/macos/common.sh
source "$SCRIPT_DIR/common.sh"

setup_release_env

require_cmd codesign
require_cmd ditto
require_cmd hdiutil
require_cmd rsync
require_cmd shasum

"$SCRIPT_DIR/build-app.sh"

sign_identity_value="$(signing_identity)"

sign_code() {
  local path="$1"
  log "Signing $path"
  codesign --force --sign "$sign_identity_value" --timestamp --options runtime "$path"
}

sign_container() {
  local path="$1"
  log "Signing $path"
  codesign --force --sign "$sign_identity_value" --timestamp "$path"
}

sign_app_bundle() {
  local app_path="$1"

  # Signing must proceed inside-out: deepest nested code first,
  # then enclosing bundles, then the app itself.
  #
  # Strategy: collect every signable target with its depth, sort
  # deepest-first, sign in that order.  This avoids recursive
  # functions and the double-signing they risk.

  local frameworks_dir="$app_path/Contents/Frameworks"

  # 1. Sign all individual executables and libraries inside embedded
  #    frameworks (XPC helpers, nested dylibs, etc.).
  if [[ -d "$frameworks_dir" ]]; then
    while IFS= read -r nested; do
      sign_code "$nested"
    done < <(
      find "$frameworks_dir" -type f \
        \( -name '*.dylib' -o -name '*.so' -o -perm -111 \) \
        ! -path '*/Resources/*' \
        | sort
    )

    # 2. Sign XPC service bundles (inside frameworks).
    while IFS= read -r -d '' xpc; do
      sign_code "$xpc"
    done < <(find "$frameworks_dir" -name '*.xpc' -print0 2>/dev/null || true)

    # 3. Sign framework bundles themselves.
    while IFS= read -r -d '' fw; do
      sign_code "$fw"
    done < <(find "$frameworks_dir" -maxdepth 1 -name '*.framework' -print0 2>/dev/null || true)
  fi

  # 4. Sign loose executables in the app (the main binary, etc.),
  #    excluding anything already covered by the framework pass.
  while IFS= read -r nested; do
    sign_code "$nested"
  done < <(
    find "$app_path/Contents" -type f \
      \( -name '*.dylib' -o -name '*.so' -o -perm -111 \) \
      ! -path '*/Resources/*' \
      ! -path '*/Frameworks/*' \
      | sort
  )

  # 5. Sign the top-level app bundle.
  sign_code "$app_path"
}

package_dmg() {
  local staging_dir
  staging_dir="$(mktemp -d "$CON_DIST_ROOT/dmg.XXXXXX")"

  rsync -a "$CON_APP_BUNDLE_PATH" "$staging_dir/"
  ln -s /Applications "$staging_dir/Applications"

  rm -f "$CON_DMG_PATH"
  hdiutil create \
    -volname "$CON_APP_NAME" \
    -srcfolder "$staging_dir" \
    -fs HFS+ \
    -format UDZO \
    -ov \
    "$CON_DMG_PATH"

  rm -rf "$staging_dir"
}

mkdir -p "$CON_DIST_ROOT"
rm -f "$CON_APP_ZIP_PATH" "$CON_DMG_PATH" "$CON_CHECKSUM_PATH"

sign_app_bundle "$CON_APP_BUNDLE_PATH"

ditto -c -k --keepParent "$CON_APP_BUNDLE_PATH" "$CON_APP_ZIP_PATH"
notarytool_submit "$CON_APP_ZIP_PATH"

if [[ "${CON_SKIP_NOTARIZATION:-0}" != "1" ]] && have_notary_credentials; then
  log "Stapling app bundle"
  xcrun stapler staple -v "$CON_APP_BUNDLE_PATH"
fi

package_dmg
sign_container "$CON_DMG_PATH"
notarytool_submit "$CON_DMG_PATH"

if [[ "${CON_SKIP_NOTARIZATION:-0}" != "1" ]] && have_notary_credentials; then
  log "Stapling dmg"
  xcrun stapler staple -v "$CON_DMG_PATH"
fi

"$SCRIPT_DIR/verify.sh"

(
  cd "$CON_DIST_ROOT"
  shasum -a 256 "$(basename "$CON_APP_ZIP_PATH")" "$(basename "$CON_DMG_PATH")" >"$(basename "$CON_CHECKSUM_PATH")"
)

log "Release artifacts:"
log "  app: $CON_APP_BUNDLE_PATH"
log "  zip: $CON_APP_ZIP_PATH"
log "  dmg: $CON_DMG_PATH"
log "  sha256: $CON_CHECKSUM_PATH"
