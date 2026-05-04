# con docs

con is a terminal with an agent built into the work surface. The terminal stays
the center of the app; the agent, input bar, workspace restore, and automation
tools are there when they make the terminal easier to use.

Start with the page that matches what you are trying to do.

## Start

| Need | Read |
| --- | --- |
| Install con | [Install](install.md) |
| Learn the main controls | [Quick controls](quick-controls.md) |
| Work with tabs, panes, broadcast, and surfaces | [Terminal workflows](terminal-workflows.md) |
| Connect providers, tune appearance, and edit shortcuts | [Settings](settings.md) |

## Use con every day

| Need | Read |
| --- | --- |
| Use the agent panel without leaving the terminal | [Built-in agent](agent.md) |
| Save or share a project layout | [Workspace profiles](workspace-layout-profiles-guide.md) |
| Drive con from scripts or external agents | [con-cli and surfaces](con-cli.md) |
| See the app | [Screenshots](screenshots.md) |
| See what changed | [Changelog](../CHANGELOG.md) |

## Platform status

- macOS is the primary beta platform.
- Windows is in preview.
- Linux is in preview.

Platform-specific limits are tracked in the source repository:
[Windows](https://github.com/nowledge-co/con-terminal/issues/34) and
[Linux](https://github.com/nowledge-co/con-terminal/issues/18).

## Contributor docs

These public docs are for people using con. If you want to build or change con
itself, start with the contributor quickstart in the source repository. The
implementation notes in `docs/impl/` and `docs/study/` are written for
contributors, not for the hosted docs navigation.

## Source of truth

The public docs navigation comes from [`docs/manifest.json`](manifest.json).
When a PR adds, renames, or removes a public docs page, update the manifest in
that PR. CI checks the manifest, and merges to `main` rebuild
`con.nowledge.co/docs`.
