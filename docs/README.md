# Documentation

This is the map for Con's docs. Start with the user guides unless you are
building or changing Con itself.

The source of truth for the public docs navigation is
[`docs/manifest.json`](manifest.json). When you add, rename, or remove a page,
update that manifest in the same PR. CI validates the manifest, and changes
merged to `main` trigger a rebuild of `con.nowledge.co/docs`.

## User Guides

| Need | Read |
| --- | --- |
| See what Con looks like | [Screenshot gallery](screenshots.md) |
| Save and reuse a project layout | [Workspace layout profiles](workspace-layout-profiles-guide.md) |
| See what changed in the latest beta | [Release notes](../CHANGELOG.md) |
| Install Con | [Install section in README](../README.md#install) |
| Learn the main shortcuts | [Quick controls in README](../README.md#2-min-know-how) |

## Product And Design

| Need | Read |
| --- | --- |
| Understand the product direction | [Architecture and vision](../DESIGN.md) |
| Understand the UI principles | [Design language](design/con-design-language.md) |
| Understand the UX model | [Product and flow spec](design/con-ux-product-spec.md) |
| Understand visual system details | [Visual spec](design/con-ui-visual-spec.md) |

## Developer Docs

| Need | Read |
| --- | --- |
| Build, run, and test locally | [Contributor quickstart](../HACKING.md) |
| Understand the agent harness | [Agent harness](impl/agent-harness.md) |
| Use the local control API | [Socket API](impl/socket-api.md) |
| Validate the CLI/control plane | [con-cli E2E](impl/con-cli-e2e.md) |
| Understand pane-local surfaces | [Pane surfaces](impl/pane-surfaces.md) |
| Understand restorable workspaces internals | [Restorable workspaces](impl/restorable-workspaces.md) |
| Understand terminal rendering | [Terminal rendering](impl/terminal-rendering.md) |
| Package macOS releases | [macOS release flow](impl/macos-release.md) |
| Track Windows port status | [Windows port](impl/windows-port.md) |
| Track Linux port status | [Linux port](impl/linux-port.md) |

## Research Notes

These are internal study notes. They are useful when changing architecture, but
most users should not need them.

- [GPUI](study/gpui.md)
- [Ghostty VT](study/ghostty-vt.md)
- [Rig](study/rig.md)
- [Socket control patterns](study/socket-control-patterns.md)
- [Markdown renderer architecture](study/markdown-renderer-architecture.md)
