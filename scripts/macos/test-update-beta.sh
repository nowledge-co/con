#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/macos/common.sh
source "$SCRIPT_DIR/common.sh"

require_cmd bash

if [[ -z "${CON_SPARKLE_PUBLIC_ED_KEY:-}" ]]; then
  fail "CON_SPARKLE_PUBLIC_ED_KEY is required to test Sparkle updates locally"
fi

export CON_CHANNEL="${CON_CHANNEL:-beta}"
if [[ "$CON_CHANNEL" != "beta" ]]; then
  fail "test-update-beta.sh only supports CON_CHANNEL=beta"
fi

# Default to an obviously old local build so any real CI release is newer.
export CON_BUILD_NUMBER="${CON_BUILD_NUMBER:-0}"

# Local update testing does not need notarization. Ad-hoc signing is enough.
export CON_ALLOW_ADHOC_SIGNING="${CON_ALLOW_ADHOC_SIGNING:-1}"
export CON_SKIP_NOTARIZATION="${CON_SKIP_NOTARIZATION:-1}"

# Derive the bundle/feed/output paths in this shell too; release.sh computes
# them internally, but child-shell exports do not propagate back here.
setup_release_env

log "Preparing Sparkle framework"
"$REPO_ROOT/scripts/sparkle/download.sh"

log "Building local beta app bundle for updater testing"
"$REPO_ROOT/scripts/macos/release.sh"

cat <<EOF

Updater test bundle ready:
  $CON_APP_BUNDLE_PATH

Important:
  - Do not use cargo run for updater testing; Sparkle is disabled there.
  - This app bundle targets channel: $CON_CHANNEL
  - Feed URL: ${CON_SPARKLE_FEED_URL:-unset}
  - CFBundleVersion: $CON_BUILD_NUMBER

Next:
  1. Open the app from Finder.
  2. Use "Check for Updates…".
  3. Confirm it sees a newer beta release from the appcast.

EOF
