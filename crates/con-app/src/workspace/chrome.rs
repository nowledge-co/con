use super::*;

pub(super) fn agent_panel_motion_target_for_agent_request(already_open: bool) -> Option<f32> {
    if already_open { None } else { Some(1.0) }
}

impl ConWorkspace {
    pub(super) const SECONDARY_PANE_OBSERVATION_LINES: usize = 40;

    pub(super) fn clamp_terminal_opacity(value: f32) -> f32 {
        value.clamp(0.25, 1.0)
    }

    pub(super) fn clamp_ui_opacity(value: f32) -> f32 {
        value.clamp(0.35, 1.0)
    }

    pub(super) fn clamp_background_image_opacity(value: f32) -> f32 {
        value.clamp(0.0, 1.0)
    }

    pub(super) fn remap_opacity(
        value: f32,
        input_floor: f32,
        output_floor: f32,
        exponent: f32,
    ) -> f32 {
        let normalized = ((value - input_floor) / (1.0 - input_floor)).clamp(0.0, 1.0);
        output_floor + (1.0 - output_floor) * normalized.powf(exponent)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn macos_major_version() -> Option<isize> {
        use objc::{class, msg_send, sel, sel_impl};

        unsafe {
            let process_info: *mut objc::runtime::Object =
                msg_send![class!(NSProcessInfo), processInfo];
            if process_info.is_null() {
                return None;
            }

            let version: NSOperatingSystemVersion = msg_send![process_info, operatingSystemVersion];
            Some(version.major_version)
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn macos_major_version() -> Option<isize> {
        None
    }

    pub(super) fn supports_terminal_glass() -> bool {
        Self::macos_major_version().is_none_or(|major| major >= 13)
    }

    pub(crate) fn effective_terminal_opacity(value: f32) -> f32 {
        if !Self::supports_terminal_glass() {
            return 1.0;
        }
        let clamped = Self::clamp_terminal_opacity(value);
        Self::remap_opacity(clamped, 0.25, 0.72, 1.55)
    }

    pub(super) fn effective_terminal_blur(value: bool) -> bool {
        value && Self::supports_terminal_glass()
    }

    pub(super) fn effective_ui_opacity(value: f32) -> f32 {
        if !Self::supports_terminal_glass() {
            return 1.0;
        }
        let clamped = Self::clamp_ui_opacity(value);
        Self::remap_opacity(clamped, 0.35, 0.84, 1.9)
    }

    pub(super) fn ui_surface_opacity(&self) -> f32 {
        Self::effective_ui_opacity(self.ui_opacity)
    }

    pub(super) fn has_active_tab(&self) -> bool {
        self.active_tab < self.tabs.len()
    }

    pub(super) fn elevated_ui_surface_opacity(&self) -> f32 {
        (self.ui_surface_opacity() + 0.02).min(0.98)
    }

    #[cfg(target_os = "macos")]
    pub(super) fn terminal_adjacent_chrome_duration(
        _open: bool,
        _open_ms: u64,
        _close_ms: u64,
    ) -> Duration {
        // Embedded Ghostty panes are native AppKit views under GPUI. Animating
        // layout next to them can expose a one-frame clear backing seam that no
        // GPUI border can reliably hide while preserving terminal glass.
        Duration::ZERO
    }

    #[cfg(not(target_os = "macos"))]
    pub(super) fn terminal_adjacent_chrome_duration(
        open: bool,
        open_ms: u64,
        close_ms: u64,
    ) -> Duration {
        Duration::from_millis(if open { open_ms } else { close_ms })
    }

    #[cfg(target_os = "macos")]
    pub(super) fn arm_chrome_transition_underlay(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        self.chrome_transition_underlay_until = Some(
            self.chrome_transition_underlay_until
                .map_or(until, |prev| prev.max(until)),
        );
    }

    #[cfg(target_os = "macos")]
    pub(super) fn extend_guard(until: &mut Option<Instant>, duration: Duration) {
        let next = Instant::now() + duration;
        *until = Some(until.map_or(next, |prev| prev.max(next)));
    }

    #[cfg(target_os = "macos")]
    pub(super) fn arm_agent_panel_snap_guard(&mut self, cx: &mut App) {
        Self::extend_guard(
            &mut self.agent_panel_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn arm_input_bar_snap_guard(&mut self, cx: &mut App) {
        Self::extend_guard(
            &mut self.input_bar_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn arm_top_chrome_snap_guard(&mut self, cx: &mut App) {
        Self::extend_guard(
            &mut self.top_chrome_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn arm_sidebar_snap_guard(&mut self, width: f32, cx: &mut App) {
        Self::extend_guard(
            &mut self.sidebar_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.sidebar_snap_guard_width = self.sidebar_snap_guard_width.max(width.max(0.0));
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    pub(super) fn snap_guard_active(until: &mut Option<Instant>, window: &mut Window) -> bool {
        Self::snap_guard_state(until, window).0
    }

    #[cfg(target_os = "macos")]
    pub(super) fn snap_guard_state(
        until: &mut Option<Instant>,
        window: &mut Window,
    ) -> (bool, bool) {
        let Some(deadline) = *until else {
            return (false, false);
        };

        if Instant::now() >= deadline {
            *until = None;
            (false, true)
        } else {
            window.request_animation_frame();
            (true, false)
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) fn sync_chrome_transition_underlay(&self, visible: bool, cx: &App) {
        if visible {
            if self.has_active_tab() {
                for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
                    terminal.set_native_transition_underlay_visible(true, cx);
                }
            }
            return;
        }

        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                terminal.set_native_transition_underlay_visible(false, cx);
            }
        }
    }
}
