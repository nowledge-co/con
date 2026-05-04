# Sidebar Rename Blur + Terminal Refocus Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make sidebar rename save on blur like horizontal tab-strip rename, and return focus to the active terminal after successful rename from either entry point.

**Architecture:** Keep rename ownership aligned with existing boundaries. `SessionSidebar` owns inline input lifecycle, blur/cancel semantics, and emits `SidebarRename`; `ConWorkspace` remains the persistence authority and owns terminal refocus after successful commits. Use small helper functions for commit/cancel remapping rather than broader refactors.

**Tech Stack:** Rust, GPUI, gpui-component `InputState` / `InputEvent`, existing `SessionSidebar` and `ConWorkspace` event flow.

---

## File Map

- Modify: `crates/con-app/src/sidebar.rs`
  - Add blur-save handling to sidebar rename
  - Add Escape-vs-blur cancel protection for sidebar rename input
  - Optionally add a tiny pure helper for rename normalization/cancel behavior if needed for tests
- Modify: `crates/con-app/src/workspace.rs`
  - Refocus active terminal after successful horizontal tab rename
  - Refocus active terminal after successful sidebar rename persistence
  - Reuse an extracted helper if it keeps focus logic concise
- Test: `crates/con-app/src/workspace.rs` test module
  - Add tiny pure helper tests only if new workspace helper logic warrants it
- Test: `crates/con-app/src/sidebar.rs` test module or local helper tests
  - Add pure behavior tests for blur-save/cancel semantics if lightweight

### Task 1: Sidebar rename blur-save and Escape protection

**Files:**
- Modify: `crates/con-app/src/sidebar.rs:321-366`
- Test: `crates/con-app/src/sidebar.rs` (new `#[cfg(test)]` helper tests near file end if needed)

- [ ] **Step 1: Write the failing test**

Add a pure helper and tests that express the desired sidebar behavior before wiring UI events. Insert a minimal helper test block near the end of `crates/con-app/src/sidebar.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::normalize_sidebar_rename_label;

    #[test]
    fn normalize_sidebar_rename_label_trims_and_clears_blank_values() {
        assert_eq!(normalize_sidebar_rename_label(""), None);
        assert_eq!(normalize_sidebar_rename_label("   \t  \n"), None);
        assert_eq!(normalize_sidebar_rename_label("  hello  "), Some("hello".to_string()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p con normalize_sidebar_rename_label_trims_and_clears_blank_values
```

Expected: FAIL with unresolved function `normalize_sidebar_rename_label`.

- [ ] **Step 3: Write minimal implementation**

In `crates/con-app/src/sidebar.rs`, add a small helper and wire `begin_rename` to use one commit path for Enter and Blur, while preserving Escape cancel protection.

Add or adapt the sidebar rename state to carry a cancel marker:

```rust
struct RenameState {
    index: usize,
    session_id: u64,
    input: Entity<InputState>,
}
```

Add a sidebar-local cancel marker field on `SessionSidebar`:

```rust
rename_cancelled_session_id: Option<u64>,
```

Add the normalization helper near the bottom of the file:

```rust
fn normalize_sidebar_rename_label(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
```

Update `begin_rename` so Enter and Blur share the same commit path:

```rust
cx.subscribe_in(&input, window, {
    move |this, input_entity, event: &InputEvent, _window, cx| match event {
        InputEvent::PressEnter { .. } | InputEvent::Blur => {
            if this.rename_cancelled_session_id == Some(session_id) {
                this.rename_cancelled_session_id = None;
                return;
            }
            let value = input_entity.read(cx).value().to_string();
            let label = normalize_sidebar_rename_label(&value);
            cx.emit(SidebarRename { session_id, label });
            this.rename = None;
            cx.notify();
        }
        _ => {}
    }
})
.detach();
```

Reset the cancel marker when a rename begins:

```rust
self.rename_cancelled_session_id = None;
```

Update `cancel_rename` to mark the active session canceled before clearing state:

```rust
fn cancel_rename(&mut self, cx: &mut Context<Self>) {
    if let Some(rename) = self.rename.take() {
        self.rename_cancelled_session_id = Some(rename.session_id);
        cx.notify();
    }
}
```

Initialize the new field in the `SessionSidebar::new` constructor:

```rust
rename_cancelled_session_id: None,
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p con normalize_sidebar_rename_label_trims_and_clears_blank_values
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/con-app/src/sidebar.rs
git commit -m "fix: save sidebar tab rename on blur"
```

### Task 2: Refocus active terminal after successful rename commits

**Files:**
- Modify: `crates/con-app/src/workspace.rs:4600-4628`
- Modify: `crates/con-app/src/workspace.rs:4905-4925`
- Test: `crates/con-app/src/workspace.rs` test module only if a new pure helper is extracted

- [ ] **Step 1: Write the failing test**

Extract a tiny pure helper only if needed to make the behavior explicit. If no pure helper is necessary, skip new unit tests and instead use build + manual verification because terminal focus is side-effectful GPUI behavior. Add this comment in the plan execution notes while implementing:

```rust
// No pure unit test here: terminal refocus is driven by live GPUI focus side effects.
// Verify by build + manual interaction after wiring the commit handlers.
```

- [ ] **Step 2: Run current targeted tests as baseline**

Run:

```bash
cargo test -p con workspace::tests
```

Expected: PASS. This confirms the current rename/reorder helper coverage is green before focus changes.

- [ ] **Step 3: Write minimal implementation**

In `crates/con-app/src/workspace.rs`, add a tiny helper to refocus the active terminal from commit paths:

```rust
fn refocus_active_terminal(&self, window: &mut Window, cx: &mut Context<Self>) {
    if let Some(tab) = self.tabs.get(self.active_tab) {
        tab.pane_tree.focused_terminal().focus(window, cx);
    }
}
```

Update horizontal rename commit to accept `window` and refocus after successful commit:

```rust
fn commit_tab_rename(
    &mut self,
    index: usize,
    value: String,
    window: &mut Window,
    cx: &mut Context<Self>,
) {
    if self.tab_rename_cancelled_index == Some(index) {
        self.tab_rename_cancelled_index = None;
        self.tab_rename = None;
        cx.notify();
        return;
    }

    let Some(tab) = self.tabs.get_mut(index) else {
        self.tab_rename = None;
        self.tab_rename_cancelled_index = None;
        cx.notify();
        return;
    };

    let label = normalize_tab_user_label(&value);
    let changed = tab.user_label != label;
    tab.user_label = label;
    self.tab_rename = None;
    self.tab_rename_cancelled_index = None;

    if changed {
        self.sync_sidebar(cx);
        self.save_session(cx);
    }
    self.refocus_active_terminal(window, cx);
    cx.notify();
}
```

Update the inline input subscription to pass `window` through:

```rust
move |this, input_entity, event: &InputEvent, window, cx| match event {
    InputEvent::PressEnter { .. } | InputEvent::Blur => {
        let value = input_entity.read(cx).value().to_string();
        this.commit_tab_rename(index, value, window, cx);
    }
    _ => {}
}
```

Update sidebar rename persistence to refocus after successful update:

```rust
fn on_sidebar_rename(
    &mut self,
    _sidebar: &Entity<SessionSidebar>,
    event: &SidebarRename,
    window: &mut Window,
    cx: &mut Context<Self>,
) {
    let new_label = event.label.as_deref().and_then(normalize_tab_user_label);
    let Some(index) = self.tabs.iter().position(|tab| tab.summary_id == event.session_id) else {
        return;
    };
    if self.tabs[index].user_label == new_label {
        return;
    }
    self.tabs[index].user_label = new_label;
    self.sync_sidebar(cx);
    self.save_session(cx);
    self.refocus_active_terminal(window, cx);
    cx.notify();
}
```

- [ ] **Step 4: Run verification**

Run:

```bash
cargo test -p con workspace::tests && cargo build -p con
```

Expected: tests PASS and build succeeds.

- [ ] **Step 5: Commit**

```bash
git add crates/con-app/src/workspace.rs
git commit -m "fix: refocus terminal after tab rename"
```

### Task 3: Manual interaction verification

**Files:**
- No code changes expected

- [ ] **Step 1: Run the app**

Run:

```bash
cargo run -p con
```

Expected: Con launches successfully.

- [ ] **Step 2: Verify sidebar blur-save**

Manual checks:

1. Open sidebar rename.
2. Type `demo-name`.
3. Click outside the input.
4. Confirm the tab name updates.
5. Open sidebar rename again.
6. Clear the name to blank.
7. Click outside the input.
8. Confirm the custom label clears and smart naming returns.

Expected: blur saves, blank clears label.

- [ ] **Step 3: Verify Escape cancel**

Manual checks:

1. Open sidebar rename.
2. Type `should-not-save`.
3. Press Escape.
4. Click elsewhere if needed.
5. Confirm the old label remains unchanged.

Expected: no save after Escape, including any subsequent blur.

- [ ] **Step 4: Verify terminal refocus**

Manual checks:

1. Rename from horizontal tab strip with Enter.
2. Immediately type a visible command like `pwd`.
3. Confirm the command goes to the terminal without extra click.
4. Rename from horizontal tab strip by blur.
5. Immediately type `echo ok`.
6. Confirm the command goes to the terminal.
7. Rename from sidebar with Enter.
8. Immediately type `ls`.
9. Confirm the command goes to the terminal.
10. Rename from sidebar by blur.
11. Immediately type `date`.
12. Confirm the command goes to the terminal.

Expected: all keystrokes land in the active terminal immediately after successful rename.

- [ ] **Step 5: Commit (only if manual verification required follow-up code changes)**

```bash
git status
```

Expected: clean working tree. If not clean because of follow-up fixes, commit with an exact message describing the fix, for example:

```bash
git add crates/con-app/src/sidebar.rs crates/con-app/src/workspace.rs
git commit -m "fix: preserve rename focus semantics"
```
