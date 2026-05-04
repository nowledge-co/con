# Built-in agent

The agent in con is a side panel for the terminal you are already using. It does
not replace the shell. It reads context, asks before acting, and works in view.

## When to use it

Use the agent when you want help that depends on terminal context:

- explain an error on screen
- plan a fix before changing files
- run a command after checking the current pane
- work inside an SSH or tmux session
- summarize what happened in a long terminal flow

Use the shell directly when you already know the command.

Provider and model choices live in [Settings](settings.md). Each tab keeps its
own active provider/model once you choose it. Global settings define the default
for new sessions and the models available in the picker.

## How it sees context

The agent starts from the focused pane. It can use nearby terminal state, pane
metadata, and the current workflow to understand where you are.

That context is useful, but it is not a license to act silently. Destructive or
high-impact actions still require approval.

When the agent streams markdown, con renders code, tables, math, and diagrams in
the panel instead of treating the answer as a separate web page. Long responses
stay scrollable so the terminal remains usable.

## Work with SSH and tmux

con is built for terminal-native workflows. The agent can help while you are
inside SSH, tmux, shells, editors, and coding-agent CLIs.

Keep the same rule in mind: the terminal remains the source of truth. If the
agent needs to know what is happening, it should inspect the pane before making
claims or taking action.

For external coding-agent orchestrators, use [con-cli and surfaces](con-cli.md).
That path lets other tools create pane-local worker sessions without changing
the built-in agent's own workflow.

## Stay in control

- Keep the pane you care about focused before asking.
- Ask for a plan first when the task is broad.
- Review commands before approving them.
- Use the terminal directly for simple commands.
- Hide the agent panel when you want a plain terminal.

con should feel like a serious terminal with help available, not a chat app
wrapped around a shell.
