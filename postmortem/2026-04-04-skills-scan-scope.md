# Skills Scan Scope Not Honored

## What happened

The slash-command skill picker did not reliably reflect the skill scopes configured in Settings. Users could add expected locations such as `~/.codex/skills` or use the built-in `Agents` preset and still fail to see the skills they expected.

## Root cause

- The filesystem scanner in `con-agent` only looked for `dir/*/SKILL.md`, so nested skill layouts such as `~/.codex/skills/.system/<skill>/SKILL.md` were skipped.
- The Settings preset for global Agents skills pointed at `~/.config/agents/skills`, but the actual shared agent path in use was `~/.agents/skills`.
- Saving Settings only invalidated the cached scan state and depended on a later render/CWD update to rescan, which made scope changes feel inconsistent.

## Fix applied

- Changed skill discovery to recurse through configured roots until it finds nested `SKILL.md` directories.
- Added a regression test covering nested skill directories.
- Corrected and expanded the Settings scope presets to match real common locations, including `~/.agents/skills` and `~/.codex/skills`.
- Triggered an immediate rescan after Settings save using the active terminal CWD.

## What we learned

- Skill ecosystems do not all share the same one-level directory layout, so discovery code needs to tolerate nested roots.
- Settings presets must match the real path conventions used by adjacent tools or users will interpret the failure as scanner breakage.
- When configuration changes affect autocomplete, rescan should happen immediately rather than waiting for incidental UI churn.
