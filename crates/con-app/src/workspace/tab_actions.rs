use super::*;

impl ConWorkspace {
    pub(super) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        let old_active = self.active_tab;

        // Take the incoming tab's panel state
        let incoming = std::mem::replace(&mut self.tabs[index].panel_state, PanelState::new());
        // Swap into the panel, get the outgoing state back
        let outgoing = self
            .agent_panel
            .update(cx, |panel, cx| panel.swap_state(incoming, cx));
        // Stash outgoing state into the old tab
        self.tabs[old_active].panel_state = outgoing;

        self.active_tab = index;
        self.tabs[index].needs_attention = false;

        // Show new tab's ghostty NSViews and focus active surface
        for terminal in self.tabs[index].pane_tree.all_terminals() {
            terminal.ensure_surface(window, cx);
        }
        self.sync_tab_native_view_visibility(index, true, cx);
        for terminal in self.tabs[old_active].pane_tree.all_surface_terminals() {
            terminal.set_focus_state(false, cx);
        }
        let old_terminals: Vec<TerminalPane> = self.tabs[old_active]
            .pane_tree
            .all_surface_terminals()
            .into_iter()
            .cloned()
            .collect();
        cx.on_next_frame(window, move |_workspace, _window, cx| {
            for terminal in &old_terminals {
                terminal.set_native_view_visible(false, cx);
            }
        });
        let focused = self.tabs[index].pane_tree.focused_terminal();
        focused.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        Self::schedule_terminal_bootstrap_reassert(
            focused,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );

        self.sync_sidebar(cx);
        // Activating a tab is a strong signal the user cares about
        // it — refresh AI label/icon if context shifted since it was
        // last summarized.
        self.request_tab_summaries(cx);
        self.save_session(cx);
        cx.notify();
    }

    pub(super) fn activate_tab_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.activate_tab(index, window, cx);
        }
    }

    pub(super) fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = (self.active_tab + 1) % self.tabs.len();
        self.activate_tab(next, window, cx);
    }

    pub(super) fn previous_tab(
        &mut self,
        _: &PreviousTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.len() <= 1 {
            return;
        }
        let prev = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
        self.activate_tab(prev, window, cx);
    }

    pub(super) fn select_tab_index(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        self.activate_tab(index, window, cx);
    }

    pub(super) fn select_tab_1(
        &mut self,
        _: &SelectTab1,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(0, window, cx);
    }

    pub(super) fn select_tab_2(
        &mut self,
        _: &SelectTab2,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(1, window, cx);
    }

    pub(super) fn select_tab_3(
        &mut self,
        _: &SelectTab3,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(2, window, cx);
    }

    pub(super) fn select_tab_4(
        &mut self,
        _: &SelectTab4,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(3, window, cx);
    }

    pub(super) fn select_tab_5(
        &mut self,
        _: &SelectTab5,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(4, window, cx);
    }

    pub(super) fn select_tab_6(
        &mut self,
        _: &SelectTab6,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(5, window, cx);
    }

    pub(super) fn select_tab_7(
        &mut self,
        _: &SelectTab7,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(6, window, cx);
    }

    pub(super) fn select_tab_8(
        &mut self,
        _: &SelectTab8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(7, window, cx);
    }

    pub(super) fn select_tab_9(
        &mut self,
        _: &SelectTab9,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_tab_index(8, window, cx);
    }

    /// Focus the active terminal (used after modal close, etc.)
    pub(super) fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        self.active_terminal().focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
    }

    pub(super) fn focus_surface_in_active_tab(
        &mut self,
        surface_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        let was_zoomed = self.tabs[self.active_tab]
            .pane_tree
            .zoomed_pane_id()
            .is_some();
        #[cfg(not(target_os = "macos"))]
        let _ = was_zoomed;

        let changed = self.tabs[self.active_tab]
            .pane_tree
            .focus_surface(surface_id);
        if !changed {
            #[cfg(not(target_os = "macos"))]
            {
                return;
            }
            #[cfg(target_os = "macos")]
            if !self.tabs[self.active_tab]
                .pane_tree
                .surface_infos(None)
                .into_iter()
                .any(|surface| surface.surface_id == surface_id)
            {
                return;
            }
        }
        #[cfg(target_os = "macos")]
        self.mark_active_tab_terminal_native_layout_pending(cx);
        #[cfg(target_os = "macos")]
        self.notify_active_tab_terminal_views(cx);
        let terminal = self.tabs[self.active_tab]
            .pane_tree
            .focused_terminal()
            .clone();
        terminal.ensure_surface(window, cx);
        #[cfg(target_os = "macos")]
        self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
        #[cfg(not(target_os = "macos"))]
        self.sync_active_tab_native_view_visibility(cx);
        if changed {
            self.sync_sidebar(cx);
            self.save_session(cx);
        }
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        cx.notify();
    }

    pub(super) fn restore_terminal_focus_after_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        self.focus_terminal(window, cx);
        cx.on_next_frame(window, |workspace, window, cx| {
            if workspace.has_active_tab() && !workspace.settings_panel.read(cx).is_overlay_visible()
            {
                workspace.focus_terminal(window, cx);
                cx.notify();
            }
        });
    }

    /// Cancel all pending agent operations across all tabs.
    /// Must be called before cx.quit() to prevent shutdown hang.
    pub(super) fn cancel_all_sessions(&self) {
        for tab in &self.tabs {
            tab.session.cancel_current();
        }
    }

    /// Show or hide ghostty NSViews for z-order management.
    /// When showing, only the active tab's views are made visible.
    /// When hiding, all views are hidden (for modal overlays).
    pub(super) fn set_ghostty_views_visible(&self, visible: bool, cx: &App) {
        if visible {
            self.sync_active_tab_native_view_visibility(cx);
        } else {
            // Hide all tabs' views for modal z-order
            for tab in &self.tabs {
                for terminal in tab.pane_tree.all_surface_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }
    }

    // ── Ghostty event handlers ──────────────────────────────

    pub(crate) fn on_terminal_focus_changed(
        &mut self,
        entity: &Entity<GhosttyView>,
        _event: &GhosttyFocusChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if let Some(pane_id) = pane_tree.pane_id_for_entity(entity_id) {
            pane_tree.focus(pane_id);
            entity.focus_handle(cx).focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
        }
        cx.notify();
    }

    pub(crate) fn on_terminal_process_exited(
        &mut self,
        entity: &Entity<GhosttyView>,
        _event: &GhosttyProcessExited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Find which tab contains the dead terminal surface (may not be the active tab).
        let entity_id = entity.entity_id();
        let tab_idx = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some());
        let Some(tab_idx) = tab_idx else { return };

        let surface = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(None)
            .into_iter()
            .find(|surface| surface.terminal.entity_id() == entity_id);
        let Some(surface) = surface else { return };
        self.record_runtime_event_for_terminal(
            tab_idx,
            &surface.terminal,
            con_agent::context::PaneRuntimeEvent::ProcessExited,
        );

        let pane_surface_count = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(surface.pane_id))
            .len();
        if pane_surface_count > 1 {
            let should_focus_replacement =
                tab_idx == self.active_tab && surface.is_active && surface.is_focused_pane;
            if let Some(outcome) = self.tabs[tab_idx]
                .pane_tree
                .close_surface(surface.surface_id, false)
            {
                let terminal = outcome.terminal;
                terminal.set_native_view_visible(false, cx);
                terminal.shutdown_surface(cx);
            }
            if tab_idx == self.active_tab {
                self.sync_active_tab_native_view_visibility(cx);
                if should_focus_replacement {
                    let replacement = self.tabs[tab_idx].pane_tree.focused_terminal().clone();
                    replacement.ensure_surface(window, cx);
                    replacement.focus(window, cx);
                }
                self.sync_active_terminal_focus_states(cx);
            }
            self.save_session(cx);
            cx.notify();
            return;
        }

        if self.tabs[tab_idx].pane_tree.pane_count() > 1 {
            let _ = self.close_pane_in_tab(tab_idx, surface.pane_id, window, cx);
        } else if self.tabs.len() > 1 {
            // Last pane in this tab — close the tab.
            self.close_tab_by_index(tab_idx, window, cx);
        } else {
            // Last pane in this window — close this workspace only.
            // App-level quit would tear down sibling windows too.
            if self.is_quick_terminal {
                self.destroy_quick_terminal_window(window, cx);
                return;
            }
            self.close_window_from_last_tab(window, cx);
        }
        cx.notify();
    }

    pub(crate) fn on_terminal_title_changed(
        &mut self,
        _entity: &Entity<GhosttyView>,
        _event: &GhosttyTitleChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Title changed — sync sidebar and tab bar.
        self.sync_sidebar(cx);
        // The OSC title change is the most reliable signal that a
        // tab's purpose just shifted (`vim` → `bash`, `bash` → `htop`).
        // Re-ask the AI for an updated label/icon. The engine
        // dedupes on cache key so this is cheap if context didn't
        // actually change.
        self.request_tab_summaries(cx);
        cx.notify();
    }

    pub(crate) fn on_terminal_cwd_changed(
        &mut self,
        entity: &Entity<GhosttyView>,
        event: &GhosttyCwdChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _reported_cwd = event.0.as_deref();
        // Stale AI label would take priority over the new CWD basename in
        // smart_tab_presentation. Clear it so the tab reflects the new
        // directory immediately while request_tab_summaries re-derives a
        // fresh label.
        let entity_id = entity.entity_id();
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some())
        {
            tab.ai_label = None;
            tab.ai_icon = None;
        }
        // Shell integration reports cwd independently from title/output.
        // Persist immediately so restart continuity survives a later crash or
        // force-quit instead of depending on an unrelated tab/layout save.
        self.sync_sidebar(cx);
        self.save_session(cx);
        self.request_tab_summaries(cx);
        cx.notify();
    }

    pub(crate) fn on_terminal_split_requested(
        &mut self,
        entity: &Entity<GhosttyView>,
        event: &GhosttySplitRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        let Some(tab_idx) = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some())
        else {
            return;
        };
        let Some(origin_pane_id) = self.tabs[tab_idx].pane_tree.pane_id_for_entity(entity_id)
        else {
            return;
        };

        let (direction, placement) = match event.0 {
            con_ghostty::GhosttySplitDirection::Right => {
                (SplitDirection::Horizontal, SplitPlacement::After)
            }
            con_ghostty::GhosttySplitDirection::Down => {
                (SplitDirection::Vertical, SplitPlacement::After)
            }
            con_ghostty::GhosttySplitDirection::Left => {
                (SplitDirection::Horizontal, SplitPlacement::Before)
            }
            con_ghostty::GhosttySplitDirection::Up => {
                (SplitDirection::Vertical, SplitPlacement::Before)
            }
        };

        let origin_terminal = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(origin_pane_id))
            .into_iter()
            .find_map(|surface| {
                (surface.terminal.entity_id() == entity_id).then_some(surface.terminal)
            });
        let cwd = origin_terminal
            .as_ref()
            .and_then(|terminal| terminal.current_dir(cx));

        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        let was_zoomed = self.tabs[tab_idx].pane_tree.zoomed_pane_id().is_some();
        self.tabs[tab_idx].pane_tree.split_pane_with_placement(
            origin_pane_id,
            direction,
            placement,
            terminal.clone(),
        );
        let tab_was_active = tab_idx == self.active_tab;
        #[cfg(target_os = "macos")]
        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
        self.notify_tab_terminal_views(tab_idx, cx);
        if tab_was_active {
            terminal.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
        } else {
            terminal.set_focus_state(false, cx);
            self.sync_tab_native_view_visibility(tab_idx, false, cx);
        }
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            tab_was_active,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.record_runtime_event_for_terminal(
            tab_idx,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        self.save_session(cx);
        cx.notify();
    }

    /// Duplicate the tab at `index`, preserving pane layout and each pane's CWD.
    pub(super) fn duplicate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }

        // Serialize the current pane tree layout (with cwd for every pane).
        let layout = self.tabs[index].pane_tree.to_state(cx, false);
        let focused_pane_id = Some(self.tabs[index].pane_tree.focused_pane_id());

        // Rebuild the pane tree from the serialized layout, spawning a fresh
        // terminal in each pane's cwd.
        let ghostty_app = self.ghostty_app.clone();
        let font_size = self.font_size;
        let mut make_terminal =
            |cwd: Option<&str>, _screen_text: Option<&[String]>, _force: bool| {
                make_ghostty_terminal(&ghostty_app, cwd, None, font_size, window, cx)
            };
        let pane_tree = PaneTree::from_state(&layout, focused_pane_id, &mut make_terminal);

        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;
        let new_tab = Tab {
            pane_tree,
            title: format!("Terminal {}", summary_id + 1),
            user_label: self.tabs[index].user_label.clone(),
            ai_label: None,
            ai_icon: None,
            color: self.tabs[index].color,
            summary_id,
            needs_attention: false,
            session: AgentSession::new(),
            agent_routing: self.tabs[index].agent_routing.clone(),
            panel_state: PanelState::new(),
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        };
        let insert_at = index + 1;
        self.tabs.insert(insert_at, new_tab);
        if self.active_tab >= insert_at {
            self.active_tab += 1;
        }
        if self.sync_tab_strip_motion() {
            #[cfg(target_os = "macos")]
            self.arm_top_chrome_snap_guard(cx);
        }
        self.activate_tab(insert_at, window, cx);
        cx.notify();
    }

    pub(super) fn duplicate_tab_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.duplicate_tab(index, window, cx);
        }
    }

    /// Close all tabs except the one at `index`.
    pub(super) fn close_other_tabs(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || self.tabs.len() <= 1 {
            return;
        }
        // Close from the end to avoid index shifting.
        for i in (0..self.tabs.len()).rev() {
            if i != index {
                self.close_tab_by_index(i, window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn close_other_tabs_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.close_other_tabs(index, window, cx);
        }
    }

    /// Close all tabs to the right of `index` (exclusive).
    pub(super) fn close_tabs_to_right(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len().saturating_sub(1) {
            return;
        }
        // Close from the end to avoid index shifting.
        let last = self.tabs.len() - 1;
        for i in (index + 1..=last).rev() {
            self.close_tab_by_index(i, window, cx);
        }
        cx.notify();
    }

    pub(super) fn close_tabs_to_right_by_id(
        &mut self,
        tab_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.close_tabs_to_right(index, window, cx);
        }
    }

    /// Set (or clear) the accent color for a tab by index.
    pub(super) fn set_tab_color(
        &mut self,
        index: usize,
        color: Option<con_core::session::TabAccentColor>,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs[index].color = color;
        self.save_session(cx);
        self.sync_sidebar(cx);
        cx.notify();
    }

    pub(super) fn set_tab_color_by_id(
        &mut self,
        tab_id: u64,
        color: Option<con_core::session::TabAccentColor>,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tab_index_for_summary_id(tab_id) {
            self.set_tab_color(index, color, cx);
        }
    }
}
