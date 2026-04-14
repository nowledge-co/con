# Playbook: Remote tmux Dev Loop

## Goal

Verify that Con can operate inside an `ssh -> tmux` workspace as a real terminal operator: choose a tmux shell target, edit files there, run code there, and keep long-running work separate.

## Setup

- One pane already attached to `ssh haswell` and `tmux`
- Remote workspace root such as `~/tmp/con-bench`
- Python 3 or another basic interpreter available on the remote host

## Prompt sequence

1. `Describe this tmux workspace and prepare a clean tmux shell target for file work.`
2. `In that tmux target, create hello.py that prints READY_TMUX_HELLO and run it.`
3. `Edit hello.py to accept a name argument and rerun it with Alice.`
4. `Start sleep 30 in a separate tmux target, then reuse the original file-work target to show hello.py still works.`
5. `Check whether claude, codex, or opencode is installed in this tmux workspace, and explain which target you would use next.`

## Success looks like

- The agent uses tmux-native targeting where available instead of treating the whole Con pane as one shell
- File creation, edit, and rerun happen in one stable tmux shell target
- The long-running sleep target stays separate from the file-work target
- The agent can explain which tmux target is for file work and which is for long-running work
- Missing agent CLI tools are reported honestly instead of guessed

## Failure looks like

- Outer-pane raw input is used as the primary tmux control path when tmux-native control exists
- The file-work target changes every turn for no reason
- The sleep target and file-work target get confused
- The agent claims tmux facts that are only weak screen observations
- Missing CLI state is hidden or reported as installed without proof

## Score

Score each dimension 0-3:

- `tmux_targeting`
- `target_stability`
- `execution_correctness`
- `separation_of_work`
- `truthfulness`
