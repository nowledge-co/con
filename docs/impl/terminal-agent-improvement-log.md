# Terminal Agent Improvement Log

Tracked benchmark-backed iteration notes for Con's terminal agent.

## 2026-04-10 11:01 UTC · operator-local-codex-devloop · 12/15 · release_floor

Strong same-target continuity, but the Codex trust prompt and final repair completion still require extra handling.

Score breakdown:
- Target Preparation: 2/3
- Target Reuse: 3/3
- Workspace Correctness: 3/3
- Execution Loop: 2/3
- Follow-up Repair: 2/3

Product changes:
- Added benchmark/operator timeouts so stuck agent asks fail cleanly instead of hanging the whole run.
- Added a tracked improvement log and trend-chart support for repeated benchmark iterations.

Lessons:
- Operator runs must handle coding-cli trust prompts without losing target continuity.
- The benchmark should distinguish partial execution success from a fully closed repair loop.

Next focus:
- Reduce trust-prompt friction in coding-cli target preparation.
- Make the repair step verify green completion before the operator run ends.

Notes:
- A live operator rerun exposed an unbounded con-cli agent ask during the Codex repair step; that failure is now part of the loop design.

