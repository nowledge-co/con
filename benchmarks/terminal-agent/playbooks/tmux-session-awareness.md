# Playbook: tmux Session Awareness

## Goal

Verify that Con can orient itself around a visible `ssh -> tmux` workflow without overstating what is proven.

## Setup

- One pane with plain `ssh`
- One pane with `ssh -> tmux`

## Prompt sequence

1. `Describe this tab's terminal situation`
2. `Which pane should you use for shell work, and which pane should you use for tmux work?`

## Success looks like

- The agent distinguishes the plain SSH pane from the tmux-oriented pane
- It uses typed facts when available and observation-tier language when tmux is only inferred from the screen
- It does not collapse the whole tab into only the focused pane
- It does not claim native tmux control unless a real tmux anchor exists

## Failure looks like

- Everything is described as `unknown`
- The tmux-like pane is routed as plain shell work
- Stale shell metadata is presented as live tmux truth
- The answer ignores the peer pane completely

## Score

Score each dimension 0-3:

- `layout_awareness`
- `tmux_orientation`
- `truthfulness`
- `actionability`

## Notes

Keep the benchmark honest:

- “looks tmux-like” is acceptable
- “is in tmux” is only acceptable when typed or native tmux evidence exists
