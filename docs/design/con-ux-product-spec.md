# con UX Product Spec

## Purpose

This document is the primary product and UX handoff for the next design phase of **con**.

It is written for product designers, interaction designers, and prototypers who need the full picture:

- what con is
- who it is for
- what workflows matter most
- what the current MVP already suggests
- what the prototype should solve next

This document is complemented by:

- `docs/design/con-design-language.md`
- `docs/design/con-ui-visual-spec.md`

---

## Source Material

This brief is grounded in:

- `DESIGN.md`
- `docs/impl/*`
- `docs/study/*`
- `crates/con-app/src/*`
- the current MVP screenshots in `.context/attachments/`
- the Nowledge Labs reference screenshots and design guide provided in this workspace

Those references inform the brief, but this document defines the design direction for **con**, not for the reference products.

---

## Product Definition

**con** is an open-source, native terminal emulator with a built-in AI harness.

Its ambition is not simply to add a chatbot to a terminal.
Its ambition is to become the best environment for modern terminal-native work:

- local coding
- remote ops
- SSH and tmux
- terminal-first agent workflows
- built-in assistance that is visible, trustworthy, and deeply integrated

The core product promise is:

**you should be able to stay in terminal flow while still getting the benefits of AI assistance, structure, and automation.**

---

## Product Thesis

### What con should be

con should feel like:

- a premium native terminal
- a calm, focused workstation
- a trustworthy agent environment
- a tool built for expert users, without becoming hostile to newer ones

### What con should not be

con should not feel like:

- a chat app with terminal decoration
- a browser-heavy IDE clone
- a dashboard made of widgets and cards
- a block-based shell abstraction that breaks raw terminal workflows

### Core truth

**The PTY is canonical. The shell is real. The agent is a layer.**

Everything in the product should reinforce this.

---

## Current MVP Assessment

The current MVP already establishes a promising foundation.

### What exists today

- a native GPUI window shell
- one terminal surface rendered from PTY + grid state
- one bottom input bar with `Smart`, `Shell`, and `Agent` modes
- one right-side agent panel with streamed messages and simple steps
- one settings overlay for provider configuration
- terminal-context wiring for the harness, including cwd, recent output, and skills

### What the MVP gets right

- the terminal is still the center of the product
- the built-in agent is integrated into the shell, not bolted on externally
- the UI is already sparse and directionally calm
- the architecture separates terminal, harness, and UI well

### What is still missing

- real tabs and split-pane behavior
- a mature composer and input model
- a structured assistant timeline instead of a flat message log
- approval, diff, and artifact review surfaces
- clear pane/session/remote metadata
- a coherent visual system with semantic tokens and component rules

The prototype should build on the MVP's architecture without inheriting its rough interaction patterns.

---

## Design Intent

The best version of con should combine:

- the **clarity and operational trust** of a serious terminal
- strong discoverability and compositional command UX
- the **restraint, spacing, and typography discipline** seen in the Nowledge Labs reference UI

The result should be light-first by default (with an equally polished dark mode), quieter, and more terminal-native than either reference.

### Nowledge UI Patterns to Adopt

From studying the Nowledge production interface:

1. **Segmented Controls** for mode switching (see Settings > Theme/Language)
   - Soft container with active segment lifted via background + shadow
   - Used for: composer routing, theme toggle, view modes

2. **Section Labels** in ALL CAPS at 11px, muted
   - Clear visual hierarchy without heavy dividers
   - Used for: settings groups, assistant sections, metadata categories

3. **Tag Pills** with soft radius and optional status dot
   - Compact (11-12px), horizontally grouped
   - Used for: context chips, entity tags, filter states

4. **Keyboard Shortcut Badges** (kbd styling)
   - Inline, subtle border, monospace
   - Used for: settings, command palette hints, hover tooltips

5. **Clean List Items** with title/description/metadata hierarchy
   - Title: medium weight, description: muted, metadata: very muted
   - Right-aligned actions (ratings, timestamps)
   - Used for: command history, search results, assistant tasks

6. **Pure White Cards** (light mode) / Subtle Elevated Cards (dark mode)
   - Hairline borders that almost disappear
   - 12-16px padding, generous internal spacing
   - Used for: approval sheets, tool cards, settings sections

---

## Primary Users

### 1. AI-native software engineer

Works in Git repos, package managers, test runners, and agent CLIs.
Wants to ask for help, apply edits, inspect diffs, and keep working without leaving terminal flow.

### 2. Infra / ops engineer

Works with remote machines, logs, deploys, tmux sessions, and long-running processes.
Needs clarity around host context, safety, and background status.

### 3. Power terminal user

Values keyboard flow, correctness, low visual noise, and speed.
Will reject the product if it feels slower, louder, or less predictable than a serious terminal.

---

## Jobs To Be Done

Users come to con to:

- do normal terminal work without friction
- recover quickly from failures and confusing output
- ask for explanation or action at the right moment
- run AI tools inside the terminal environment, not outside it
- stay oriented across tabs, panes, repos, and remote sessions
- trust what the built-in agent is doing before it acts

---

## Experience Pillars

### 1. Terminal flow comes first

A user should always feel one shortcut away from pure shell work.
con must remain excellent even when the built-in agent is ignored.

### 2. Assistance should be ambient before it becomes explicit

The product should start quiet.
The user invites depth.
This matters more than flashy first-use AI behavior.

### 3. Structured work beats endless output

One of the best lessons from modern terminal UX is that users benefit when work becomes easier to parse and resume.
con should help structure work where it adds value, without replacing raw output.

### 4. Trust is the product

For a built-in agent, trust is not a compliance feature. It is the product experience.
The user should always know what the system sees, intends, executes, and changes.

### 5. Design should lower stress, not add novelty

The visual system should reduce cognitive load through rhythm, contrast, and calmness.

---

## Product Model

The product should be understood as five layers.

### 1. Workspace shell

The top-level application frame:

- window
- titlebar and tabs
- split layout
- session identity
- global actions

### 2. Terminal surfaces

The actual work panes.
Each terminal pane owns:

- a PTY session
- a current process or shell
- cwd and repo context
- remote/local status
- scrollback and current selection
- extracted metadata when available

### 3. Assistive overlays

Small enhancements that help without taking over:

- command palette
- search
- selection actions
- inline explain/fix actions
- notifications
- hover metadata

### 4. Agent surfaces

Places where built-in agent work becomes visible and actionable:

- docked assistant panel
- inline explain/fix surfaces
- task timeline
- approval sheet
- diff/artifact preview

### 5. Settings and system surfaces

Global preferences and operational controls:

- providers and models
- keybindings
- appearance
- socket / automation permissions
- session restore behavior

---

## Core Interaction Model

## A. Window, tabs, and panes

### Expected structure

- a window contains tabs
- a tab contains one split tree
- a split leaf contains one terminal pane
- each tab may have a companion assistant state
- each pane may expose local metadata and attention states

### Design requirements

- active pane focus must always be obvious
- switching tabs restores the last focused pane and companion state
- panes can be named automatically or manually
- pane states are subtle but visible: active, running, failed, remote, agent-busy
- closing or replacing a busy pane should feel careful but lightweight

### Why this matters

Without a strong workspace model, all AI features feel bolted on.
The shell architecture must feel mature first.

---

## B. Composer

The current tri-mode input bar is a useful MVP experiment, but not the right long-term interaction.

### Recommended prototype direction

Move to a **unified composer** with explicit routing.

### Routing modes

- `Command` — run directly in the shell
- `Ask` — send to built-in assistant
- `Action` — slash commands, skills, and automation shortcuts

### Rules

- smart routing may exist, but it must remain legible and overridable
- routing should be visible before send
- the composer must support attachments and context visibility
- `Tab` should not be the primary mode-switching model

### Composer responsibilities

The composer should support:

- single-line command entry
- multiline asks
- context attachments
- slash suggestions
- recent input recall
- clear keyboard focus

This is one of the most important prototype surfaces.

---

## C. Built-in assistant

The assistant should have three levels of presence.

### 1. Ambient

The user is mostly working in terminal.
AI is available but unobtrusive.
Examples:

- explain selection
- explain last failure
- summarize long output

### 2. Companion

A docked right panel opens for ongoing work.
Use when the agent is planning, iterating, or presenting outputs.

### 3. Intervention

A stronger modal or sheet appears only when the product must ask for explicit confirmation or highlight a high-impact decision.

### Assistant panel model

The assistant panel should not be designed as a generic chat log.
It should be designed as a **task timeline** containing:

- user intent
- attached context
- agent plan
- tool calls
- outputs
- diffs or artifacts
- final answer

That structure is essential for trust and resumability.

---

## D. Context model

Context quality is one of con's biggest strategic opportunities.

### Context sources

The system should be able to use, and show, the following:

- active pane
- cwd and repo
- branch and status
- visible selection
- last command and exit status
- recent output
- `AGENTS.md` and project skills
- SSH status
- tmux status
- task-specific files, diffs, or artifacts

### UX rule

Attached context should be visible, inspectable, and removable before send.

### Recommended representation

Compact context pills above or inside the composer.
Each pill can open detail when needed.

---

## E. Tool execution and approvals

This is a defining product surface.

### The user should always know

- what action will run
- where it will run
- whether it is read-only, editing, or high-impact
- whether it affects local or remote state
- what the result was

### Approval classes

#### Read-only

Safe by default.
Examples: search, read, inspect, summarize.

#### Local write

Usually one-tap approve or subject to session policy.
Examples: edit file, create file, stage diff, run formatter.

#### High-impact or remote-sensitive

Always explicit.
Examples: delete recursively, deploy, install system dependencies, authenticated remote operations.

### UI implication

Tool calls should appear as structured cards, not hidden log lines.

---

## F. SSH, tmux, and external agent CLIs

This is where con has to be more deliberate than many reference products.

### Core rule

These workflows should feel clearer in con, not wrapped by con.

### Expected behavior

- SSH state is visible and scoped clearly
- remote host identity is obvious without being loud
- tmux is respected instead of abstracted away
- external agent CLIs can run directly in a pane without con hijacking them
- built-in assistance remains available, but never competes for terminal ownership

### Required state model

The product should think in scopes, not labels.

A pane may contain a stack like:

1. local shell
2. SSH connection
3. remote shell
4. tmux session
5. external agent CLI

The UI and the built-in agent should consume that same scope model.

### Product rules

- Never present shell cwd or last command as if they describe the visible app when the pane is in tmux or another TUI.
- Prefer calm scope indicators over noisy badges.
- When certainty is low, show less and say less.
- When an action is remote-sensitive, approvals must make the remote scope explicit.
- When an external agent CLI is active, con should support it with orientation, notifications, and safe approvals, not compete for control.
- Never let one address space masquerade as another. A con pane, a tmux pane, and an editor buffer are different targets and must be surfaced that way.
- Hidden local execution, visible shell execution, tmux-native control, and raw TUI input are different action types and must not share one generic "run command" path.
- Tool availability should follow typed capabilities, not prose heuristics. If a pane does not expose `exec_visible_shell`, the product must not imply that command execution is available there.

### Minimum visible cues

- local vs remote
- multiplexer session when known
- whether the pane is at a shell prompt or inside another interactive runtime
- external agent CLI identity when confidence is high

### Failure to avoid

The worst failure is false confidence.

Users will tolerate uncertainty.
They will not tolerate the terminal confidently naming the wrong host, session, or active tool.

### Why this matters

This is where terminal-native credibility is won or lost.

---

## G. Notifications and resume-ability

con should help users resume interrupted work gracefully.

### Notification classes

- task complete
- task failed
- approval required
- pane needs attention
- remote disconnect
- background task finished in another tab

### Surface priority

1. pane-level signal
2. tab-level signal
3. subtle in-window toast
4. system notification only when justified

The goal is recovery, not interruption.

---

## North Star Flows

## 1. Launch into work

Open the app and arrive in a calm, ready terminal state immediately.
AI should not be the first thing competing for attention.

## 2. Run a command and recover from failure

The user runs a command, it fails, and con offers quick routes to explain, fix, or ask about the result without breaking shell flow.

## 3. Start a multi-step agent task

The user asks for a coding or ops task.
The product opens a companion task timeline with context, plan, approvals, and outputs.

## 4. Work with an external agent CLI

The user launches an external agent CLI directly in a pane.
con provides metadata, notifications, and smoother environment support without trying to replace the tool.

## 5. Work remotely via SSH / tmux

The user is clearly oriented around host and session scope, and dangerous or high-impact actions stay explicit.

## 6. Return after interruption

The user comes back to a tab, background task, or pane and can immediately tell what happened and what still needs attention.

---

## Prototype Priorities

### Priority 1 — Nail the shell

Prototype:

- titlebar and tab strip
- pane focus and split layout
- project/branch/remote metadata
- calm terminal-first composition

### Priority 2 — Nail the composer

Prototype:

- unified routing model
- context pills
- multiline ask state
- keyboard and focus behavior

### Priority 3 — Nail the assistant timeline

Prototype:

- plan state
- tool-call cards
- approval states
- final response and resume state

### Priority 4 — Nail trust moments

Prototype:

- failed command recovery
- file edit approval
- remote-sensitive operation
- background completion and attention states

### Priority 5 — Nail external-tool coexistence

Prototype:

- external agent CLI in-pane behavior
- SSH and tmux affordances

---

## Deliverables for the Design Team

The next design pass should produce:

- a workspace shell spec
- a unified composer spec
- an assistant timeline spec
- an approval and diff review spec
- an SSH / tmux / external-agent metadata spec
- a notification system spec
- a motion spec
- 5–7 prototype screens that demonstrate the north star flows

---

## Open Questions

- Should assistant history be tab-scoped, workspace-scoped, or both?
- Should the composer belong to the active pane or the whole tab?
- How much command segmentation should be visible by default in a terminal-native UI?
- What is the lightest approval pattern that still feels safe?
- How should con represent remote risk without becoming noisy?
- How should the assistant collapse when inactive so that it feels present but not distracting?

---

## Final Direction

The design team should optimize for this feeling:

**con is the terminal you trust enough to live in all day — fast enough for experts, calm enough for long sessions, and transparent enough for AI-native work.**
