#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

export CON_GHOSTTY_PROFILE=1
export RUST_LOG="${RUST_LOG:-con::perf=info,con_ghostty::perf=info,con=warn,con_core=warn,con_agent=warn}"

echo "Profiling terminal host path with:"
echo "  CON_GHOSTTY_PROFILE=$CON_GHOSTTY_PROFILE"
echo "  RUST_LOG=$RUST_LOG"
echo
echo "Reproduce:"
echo "  1. Start a heavy TUI such as 'claude --resume'"
echo "  2. Drag-resize the window for 3-5 seconds"
echo "  3. Capture con::perf and con_ghostty::perf lines"
echo

cargo run -p con "$@"
