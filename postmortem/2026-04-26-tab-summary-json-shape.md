# 2026-04-26 — Tab summarizer never renamed tabs against Moonshot Kimi K2.6

## What happened

After landing the AI tab-summarizer (vertical tabs, `TabSummaryEngine`),
real-world testing against the user's configured Moonshot **Kimi K2.6**
(`k2p6`) model never produced an AI label. Tabs stayed on their
heuristic name (`bash`, `Shell`) regardless of what command ran in the
pane. Logs showed every request being **parsed-rejected**:

```
WARN con_core::tab_summary tab_summary parse rejected tab_id=0
  raw="The user wants me to label a terminal tab... [3000+ chars
        of chain-of-thought, ending mid-sentence at]
        \"Let me double-check constraints:\\n-\""
```

## Root cause

Two layered failures, both rooted in how reasoning models behave when
asked to follow free-form output rules.

### 1. The model deliberates instead of answering

The prompt asked for a single line `LABEL|ICON`. K2.6 (per
`models.dev/.../moonshotai/models/kimi-k2.6.toml`: `reasoning =
true`, `interleaved.field = "reasoning_content"`) treats every rule as
a constraint to argue about, not follow. A `free -h` request burned
~1500 tokens debating whether the icon should be `pulse` (system
monitor) or `terminal` (one-shot command), then hit the token budget
**mid-deliberation** and emitted no answer at all. Doubling the budget
just gave the model more room to deliberate. We confirmed this from
rig's `gen_ai.completion` log entries showing 2700+ output tokens of
reasoning content that never reached a `LABEL|ICON` line.

### 2. `prompt_typed` doesn't constrain Moonshot

The first fix attempt was rig's `prompt_typed::<TabSummaryJson>`,
which sends `response_format: { type: "json_schema", … }` to the
model. On Moonshot this hits:

```
WARN rig::providers::moonshot] Structured outputs currently not
   supported for Moonshot
```

…so `response_format` is silently dropped at the rig provider layer
and the model is **not** schema-constrained on the wire. Worse, the
model still emits something *that looks like* JSON — but wrapped in
a markdown code fence:

```
gen_ai.completion="```json
{
  \"label\": \"Shell\",
  \"icon\": \"terminal\"
}
```"
```

…and rig's `TypedPromptRequest::send` calls `serde_json::from_str`
straight on the raw response, which fails with `expected value at
line 1 column 1` because of the fence prefix.

### 3. The pump-driven re-trigger only fires during active output

Even when a request *did* succeed, the engine cached the result and
the workspace's "ask the engine to re-check" trigger was wired only to
`pump_ghostty_views() == true`. That signal goes false the instant a
command finishes scrolling, so once a tab's first label landed
("Shell" for an empty pane), no later command would ever re-trigger
the engine — the user saw a tab labeled "Shell" forever.

## Fix

Shipped together as one fix because each piece is necessary on its
own:

1. **Switched the response shape from `LABEL|ICON` to JSON** — same
   information, but JSON is robust to extra whitespace, code fences,
   and trailing commentary, so a tolerant parser can recover the
   answer even when the model adds surrounding prose. Schema:
   `{"label": "...", "icon": "..."}`.
2. **Wrote a bracket-balanced JSON extractor** (`parse_summary_json`
   in `tab_summary.rs`). Walks the response for the first `{` and the
   matching `}`, ignoring everything else. Naturally strips
   ```` ```json ... ``` ```` fences, reasoning preamble, and trailing
   commentary. Six unit tests cover code-fenced JSON, unlabeled
   fences, JSON with prose preamble, embedded curly braces in string
   values, and clean / no-JSON inputs.
3. **Kept the streaming completion path** (`AgentProvider::complete_with_options`)
   instead of `prompt_typed`, because:
   - `prompt_typed` is a no-op on providers that ignore `response_format`
     (Moonshot today).
   - Even when honored, several providers wrap the response in a
     markdown fence anyway, which `TypedPromptRequest::send` can't
     parse.
   - The streaming path already correctly reads `reasoning_content`
     for K2.6-class reasoning models.
4. **Added a periodic AI re-summary poll** (3 s tick) in the
   workspace's main async loop. The pump-driven trigger stays
   (`pump_ghostty_views()`); the periodic tick is a safety net for
   when the pump goes quiet but the tab's effective context has
   changed. The engine's per-tab cache + 5 s success budget keep this
   cheap.

We did NOT need to bump the token budget further — the JSON shape is
tight enough (~30 chars of useful content) that even thinking models
emit it within a few hundred tokens before deliberating runs them
out.

## Verification

End-to-end against live Moonshot K2.6:

```
[07:42:02] tab_summary parsed tab_id=0 label="Shell" icon=Terminal       (empty pane)
[07:43:34] tab_summary parsed tab_id=0 label="Git"   icon=FileCode       (after `cd /workspace && git status`)
```

Sidebar visibly updated `Shell → Git` with the file-code icon, no
restart, no manual refresh. Six new unit tests on the JSON parser, all
24 `con-core::tab_summary` tests green. Full workspace check `RUSTFLAGS="-D
warnings"` clean; 139 tests pass.

## What we learned

- **Reasoning models will fight free-form rules.** "Output exactly one
  line in this format" is a constraint a thinking model debates.
  Schema-shaped output (JSON object, even without true schema
  enforcement) reframes the task into "fill these slots", which the
  model treats as the work itself instead of an opinion to argue
  about.
- **`response_format` is best-effort across providers.** Don't trust
  rig's `prompt_typed` to constrain output on every provider — verify
  with the provider's docs / models.dev capability flags. Even when
  it's accepted, models still wrap responses in markdown fences;
  always parse defensively.
- **Pump-driven UI triggers need a periodic backstop.** Anything keyed
  on "user produced output" misses the case "context drifted while
  the pane sat idle" (e.g., the user navigates the tab away and back).
  A 3 s safety-net poll is cheap when the engine itself caches.
- **Markdown code fences are the LLM equivalent of trailing newlines
  on a CSV.** Any code that consumes raw model output and feeds it to
  a strict parser (`serde_json::from_str`, `toml::from_str`) needs to
  strip them — assuming the model "follows the instructions" is a
  losing bet at scale.
