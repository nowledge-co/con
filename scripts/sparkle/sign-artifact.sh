#!/usr/bin/env bash
#
# Sign a release artifact for Sparkle appcast.
#
# Usage:
#   ./sign-artifact.sh <artifact-path>
#
# Environment:
#   SPARKLE_SIGNING_KEY — base64-encoded Ed25519 private key
#   SPARKLE_DIR         — path to downloaded Sparkle (default: .sparkle)
#
# Outputs to stdout:
#   edSignature="<base64>" length="<bytes>"

set -euo pipefail

artifact="${1:?usage: sign-artifact.sh <artifact-path>}"

[[ -f "$artifact" ]] || { echo "File not found: $artifact" >&2; exit 1; }
[[ -n "${SPARKLE_SIGNING_KEY:-}" ]] || { echo "SPARKLE_SIGNING_KEY is required" >&2; exit 1; }

SPARKLE_DIR="${SPARKLE_DIR:-$(cd "$(dirname "$0")/../.." && pwd)/.sparkle}"
sign_update="$SPARKLE_DIR/bin/sign_update"

[[ -x "$sign_update" ]] || { echo "sign_update not found at $sign_update — run download.sh first" >&2; exit 1; }

# sign_update reads the private key from -s flag (base64) or --ed-key-file
"$sign_update" "$artifact" -s "$SPARKLE_SIGNING_KEY"
