# Tool output double-encoded JSON

**Date**: 2026-04-06

## What happened

Flow logs showed garbled tool results: `\\"[\\n  {\\n    \\\\"index\\\\"`. The agent received escaped-JSON-inside-escaped-JSON instead of clean structured data. Tools like `list_panes`, `batch_exec`, and `tmux_inspect` returned data the model had to parse through two layers of escaping.

## Root cause

Rig's `DynTool` trait adapter serializes every tool's `Output` via `serde_json::to_string(&output)`. When `Output = String` and the tool pre-serializes with `serde_json::to_string_pretty(&data)`, the result is double-encoded:

```
Tool returns: "{\"index\":1,\"title\":\"~\"}"     (a String containing JSON)
DynTool wraps: "\"{\\\"index\\\":1,\\\"title\\\":\\\"~\\\"}\""  (JSON-encoded String)
```

The model receives a string literal containing escaped JSON, not a JSON object.

## Fix

Changed affected tools from `Output = String` to `Output = serde_json::Value`, using `serde_json::to_value()` instead of `serde_json::to_string_pretty()`. When DynTool serializes a `Value`, it produces clean JSON — no double layer.

## What we learned

1. **Know your serialization boundary.** When a framework handles serialization (rig's DynTool), the tool should return structured data, not pre-serialized strings. The `Output` type is a semantic contract — `String` means "this is a string value," not "this is pre-formatted JSON."

2. **Flow logs are essential.** This bug was invisible without the `tool_result` preview logging added in the same session. The agent appeared to work (models are resilient to garbled input), but was burning tokens parsing escaped JSON and occasionally misinterpreting fields.
