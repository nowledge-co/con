#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${1:-$ROOT_DIR/dist/xctrace}"
mkdir -p "$OUT_DIR"

TRACE_NAME="con-terminal-resize-$(date +%Y%m%d-%H%M%S).trace"
TRACE_PATH="$OUT_DIR/$TRACE_NAME"

echo "Recording Time Profiler trace to:"
echo "  $TRACE_PATH"
echo
echo "Workflow:"
echo "  1. Con will launch under xctrace."
echo "  2. Reproduce 'claude --resume' and the bad live resize gesture."
echo "  3. Stop recording with Ctrl+C in this terminal."
echo

export CON_GHOSTTY_PROFILE=1
export RUST_LOG="${RUST_LOG:-con::perf=info,con_ghostty::perf=info,con=warn,con_core=warn,con_agent=warn}"

xcrun xctrace record \
  --template 'Time Profiler' \
  --output "$TRACE_PATH" \
  --launch -- \
  cargo run -p con
