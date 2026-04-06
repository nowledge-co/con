# wait_for idle mode unusable on remote panes

**Date**: 2026-04-06

## What happened

Agent SSH'd into two remote hosts via `create_pane`, ran `sudo apt update && sudo apt upgrade -y`, then called `wait_for` with `pattern: "completed"`. The apt commands finished successfully (visible in terminal), but "completed" never appears in apt output. The agent hung for 600 seconds on a pattern that would never match.

The agent couldn't use idle mode (no pattern) because remote panes lack shell integration — `wait_for` returned `no_shell_integration` immediately, which is useless.

## Root cause

Two compounding issues:

1. **wait_for idle mode required shell integration.** Without OSC 133 (shell integration), `is_busy` is always false. The original code returned `no_shell_integration` immediately, forcing the agent to use pattern mode.

2. **Pattern mode requires guessing output text.** The agent must predict what the command will output. It guessed "completed" — a word that appears nowhere in apt's output format. This is a fundamental UX failure: the system forced the agent into a mode that requires domain knowledge it doesn't have.

## Fix

Added **output quiescence detection** as a universal fallback for idle mode:

- When `wait_for` is called without a pattern on a pane without shell integration, it enters quiescence mode
- Polls `recent_lines(50)` every 500ms, normalizing each line with `trim_end()` before comparison
- If normalized output is unchanged for 4 consecutive polls (2 seconds), returns `status: "idle"`
- If output keeps changing (command still producing output), keeps waiting
- Empty/uninitialized baselines are deferred — counting only starts after the first non-empty snapshot

Three detection modes, transparent to the caller:

| Mode | When | Polling | Signal |
|------|------|---------|--------|
| Shell integration | has SI, no pattern | 100ms | `command_finished` / `!is_busy` |
| Quiescence | no SI, no pattern | 500ms | Normalized output unchanged for 2s |
| Pattern match | pattern provided | 500ms | Substring in last 50 lines |

The agent just calls `wait_for pane_index:2` — no pattern needed. The tool description now says "Prefer idle mode (no pattern) — it works universally."

## Implementation details

- Quiescence baseline is captured **inside** the async task (not before spawn) to avoid a race window between snapshot and first poll
- **Output normalization**: Each line is `trim_end()`'d before comparison. `ghostty_surface_read_text` returns lines with trailing whitespace that varies with cursor position (cursor column changes which cells contain spaces). Without trimming, the exact byte comparison (`current == last_snapshot`) fails spuriously — the content is identical but trailing whitespace differs, resetting the stability counter every poll. This was the root cause of quiescence hanging "forever" on SSH panes.
- **Empty baseline handling**: If the terminal isn't ready when the baseline is captured (surface not yet initialized, or `this.update()` fails), the baseline is empty. An empty baseline is never counted as "stable" — the first non-empty poll becomes the real baseline. This prevents both false "idle" (empty == empty) and permanent "changed" (empty != anything).
- `recent_lines()` returns plain text from ghostty's grid — no ANSI codes, so cursor blink doesn't cause false "still changing" detection
- Progress bars that rewrite lines via \\r show current state in the grid text and stop changing when done, so quiescence correctly detects their completion
- Empty pattern (`""`) is normalized to `None` at handler entry — Rust's `"".contains("")` is always true, which would cause instant false "matched" return. This ensures models that pass empty pattern fall through to idle/quiescence mode.
- Known limitation: commands that produce no output for >2s mid-execution will trigger a false "idle". The agent can call wait_for again if needed.

## What we learned

1. **Don't force the model to predict.** The old design forced the agent to guess output text for pattern mode. The quiescence approach works without predictions — it observes the terminal directly, same as a human watching for the prompt to return.

2. **Degrade gracefully, don't refuse.** Returning `no_shell_integration` immediately was a refusal to provide useful behavior. The right approach is to fall back to a less precise but still useful detection method.

3. **System-level fixes beat prompt patches.** The initial attempt was to add SSH-specific guidance to the system prompt ("don't wait for password," "use read_pane after SSH"). This would have added tokens, narrowed to one scenario, and required more patches for each new edge case. The quiescence detection solves the problem structurally.
