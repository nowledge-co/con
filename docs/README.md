# con docs

con is a terminal first, with agent help only when you ask for it. The agent
works from the terminal context you already have on screen.

Choose the guide for what you want to do.

## Get started

| Need | Read |
| --- | --- |
| Install con | [Install](install.md) |
| Learn the main controls | [Quick controls](quick-controls.md) |
| Use the agent panel | [Built-in agent](agent.md) |
| Save a project layout | [Workspace profiles](workspace-layout-profiles-guide.md) |
| See the app | [Screenshots](screenshots.md) |
| See what changed | [Changelog](../CHANGELOG.md) |

## What belongs here

These public docs are for people using con as their terminal. They cover
installation, everyday controls, the agent panel, workspace profiles, and
release notes.

Contributor setup, design notes, implementation records, benchmarks, and
research notes stay in the source repo. They are useful for building con, but
they are not part of the end-user docs path.

## Source of truth

The public docs navigation comes from [`docs/manifest.json`](manifest.json).
When a PR adds, renames, or removes a public docs page, update the manifest in
that PR. CI checks the manifest, and merges to `main` rebuild
`con.nowledge.co/docs`.
