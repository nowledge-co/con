#!/usr/bin/env bash
#
# Generate an Ed25519 key pair for Sparkle appcast signing.
#
# Prerequisites:
#   Run ./scripts/sparkle/download.sh first to get Sparkle's tools.
#
# Produces two values:
#   1. Private key (base64) — store as SPARKLE_SIGNING_KEY GitHub secret
#   2. Public key (base64)  — store as SPARKLE_PUBLIC_ED_KEY GitHub secret
#
# IMPORTANT: Only Sparkle's own generate_keys tool produces keys in the
# correct format.  OpenSSL/libsodium keys are NOT compatible.
#
# Environment:
#   SPARKLE_DIR — path to downloaded Sparkle (default: .sparkle)

set -euo pipefail

SPARKLE_DIR="${SPARKLE_DIR:-$(cd "$(dirname "$0")/../.." && pwd)/.sparkle}"
generate_keys="$SPARKLE_DIR/bin/generate_keys"

if [[ ! -x "$generate_keys" ]]; then
  # Try sign_update which also has key generation via -g
  sign_update="$SPARKLE_DIR/bin/sign_update"
  if [[ -x "$sign_update" ]]; then
    echo "[sparkle-keygen] Generating key pair via sign_update --generate"
    echo ""
    "$sign_update" --generate
    echo ""
    echo "Store the private key as GitHub secret: SPARKLE_SIGNING_KEY"
    echo "Store the public key as GitHub secret:  SPARKLE_PUBLIC_ED_KEY"
    echo ""
    echo "The public key is also baked into Info.plist via CON_SPARKLE_PUBLIC_ED_KEY."
    exit 0
  fi

  echo "ERROR: Sparkle tools not found at $SPARKLE_DIR" >&2
  echo "Run ./scripts/sparkle/download.sh first." >&2
  exit 1
fi

echo "[sparkle-keygen] Generating key pair via generate_keys"
echo ""
"$generate_keys"
echo ""
echo "Store the private key as GitHub secret: SPARKLE_SIGNING_KEY"
echo "Store the public key as GitHub secret:  SPARKLE_PUBLIC_ED_KEY"
echo ""
echo "The public key is also baked into Info.plist via CON_SPARKLE_PUBLIC_ED_KEY."
