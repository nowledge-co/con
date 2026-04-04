//! TerminalPane — unified wrapper over legacy TerminalView and GhosttyView.
//!
//! This enum allows the workspace and pane tree to work with either backend
//! through a common API. Grid-specific features (command blocks, OSC 133,
//! text search) gracefully degrade to defaults when using ghostty.

use std::sync::Arc;

use con_terminal::{Grid, TerminalTheme};
use gpui::*;
use parking_lot::Mutex;

use crate::terminal_view::TerminalView;

#[cfg(target_os = "macos")]
use crate::ghostty_view::GhosttyView;

/// A terminal pane backed by either the legacy Grid+vte renderer or
/// ghostty's GPU-accelerated Metal renderer.
pub enum TerminalPane {
    Legacy(Entity<TerminalView>),
    #[cfg(target_os = "macos")]
    Ghostty(Entity<GhosttyView>),
}

impl Clone for TerminalPane {
    fn clone(&self) -> Self {
        match self {
            Self::Legacy(e) => Self::Legacy(e.clone()),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => Self::Ghostty(e.clone()),
        }
    }
}

impl TerminalPane {
    // ── State queries ───────────────────────────────────────

    pub fn title(&self, cx: &App) -> Option<String> {
        match self {
            Self::Legacy(e) => e.read(cx).title(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.read(cx).title(),
        }
    }

    pub fn current_dir(&self, cx: &App) -> Option<String> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().current_dir.clone(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.read(cx).current_dir(),
        }
    }

    pub fn is_alive(&self, cx: &App) -> bool {
        match self {
            Self::Legacy(e) => e.read(cx).pty().lock().is_alive(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.read(cx).is_alive(),
        }
    }

    pub fn detected_remote_host(&self, cx: &App) -> Option<String> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().detected_remote_host(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => None, // ghostty doesn't expose this
        }
    }

    pub fn is_busy(&self, cx: &App) -> bool {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().is_busy(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => false, // ghostty doesn't expose OSC 133 state
        }
    }

    // ── Mutations ───────────────────────────────────────────

    /// Write bytes to the terminal PTY. Handles control characters correctly
    /// for both legacy (raw PTY fd) and ghostty (text + key event split).
    pub fn write(&self, data: &[u8], cx: &App) {
        match self {
            Self::Legacy(e) => e.read(cx).write_to_pty(data),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => {
                if let Some(terminal) = e.read(cx).terminal() {
                    terminal.write_to_pty(data);
                }
            }
        }
    }

    pub fn set_theme(&self, theme: &TerminalTheme, cx: &mut App) {
        match self {
            Self::Legacy(e) => {
                e.update(cx, |view, _cx| {
                    view.grid().lock().set_theme(theme);
                });
            }
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => {
                // Ghostty uses its own theme system via color scheme
                let is_dark = theme.name.to_lowercase().contains("dark");
                e.read(cx).terminal().map(|t| t.set_color_scheme(is_dark));
            }
        }
    }

    pub fn clear_scrollback(&self, cx: &mut App) {
        match self {
            Self::Legacy(e) => {
                e.update(cx, |view, _cx| {
                    view.grid().lock().clear_scrollback();
                });
            }
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => {} // not available via ghostty API
        }
    }

    pub fn set_suggestion(&self, suggestion: Option<String>, cx: &mut App) {
        match self {
            Self::Legacy(e) => {
                e.update(cx, |view, cx| {
                    view.set_suggestion(suggestion, cx);
                });
            }
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => {} // ghostty doesn't support ghost text
        }
    }

    // ── Grid access (legacy only) ─────────────────────────

    /// Access the Grid for legacy-only operations (command blocks, OSC 133, etc.)
    /// Returns None for ghostty.
    pub fn as_grid(&self, cx: &App) -> Option<Arc<Mutex<Grid>>> {
        match self {
            Self::Legacy(e) => Some(e.read(cx).grid().clone()),
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => None,
        }
    }

    /// Notify the GPUI entity that it needs re-rendering.
    pub fn notify(&self, cx: &mut App) {
        match self {
            Self::Legacy(e) => e.update(cx, |_, cx| cx.notify()),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.update(cx, |_, cx| cx.notify()),
        }
    }

    /// Show or hide the native NSView layer for z-order management.
    /// No-op for legacy terminals (they render via GPUI's Metal layer).
    pub fn set_native_view_visible(&self, _visible: bool, _cx: &App) {
        match self {
            Self::Legacy(_) => {} // GPUI-rendered, z-order handled by GPUI
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.read(_cx).set_visible(_visible),
        }
    }

    // ── GPUI integration ────────────────────────────────────

    pub fn entity_id(&self) -> EntityId {
        match self {
            Self::Legacy(e) => e.entity_id(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.entity_id(),
        }
    }

    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            Self::Legacy(e) => e.focus_handle(cx),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.focus_handle(cx),
        }
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle(cx).focus(window, cx);
    }

    /// Check if this pane has window focus.
    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle(cx).is_focused(window)
    }

    /// Render this pane as an AnyElement for inclusion in the pane tree.
    pub fn render_child(&self) -> AnyElement {
        match self {
            Self::Legacy(e) => e.clone().into_any_element(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => e.clone().into_any_element(),
        }
    }

    // ── Agent context helpers ───────────────────────────────

    /// Extract visible content lines for agent context.
    pub fn content_lines(&self, n: usize, cx: &App) -> Vec<String> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().content_lines(n),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => {
                e.read(cx).terminal()
                    .map(|t| t.read_screen_text(n))
                    .unwrap_or_default()
            }
        }
    }

    /// Extract recent lines (including scrollback).
    pub fn recent_lines(&self, n: usize, cx: &App) -> Vec<String> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().recent_lines(n),
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => {
                e.read(cx).terminal()
                    .map(|t| t.read_recent_lines(n))
                    .unwrap_or_default()
            }
        }
    }

    pub fn last_command(&self, cx: &App) -> Option<String> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().last_command.clone(),
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => None,
        }
    }

    pub fn last_exit_code(&self, cx: &App) -> Option<i32> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().last_exit_code,
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => None,
        }
    }

    /// Grid dimensions (cols, rows). Returns (80, 24) as default for ghostty.
    pub fn grid_size(&self, cx: &App) -> (usize, usize) {
        match self {
            Self::Legacy(e) => {
                let g = e.read(cx).grid().lock();
                (g.cols, g.rows)
            }
            #[cfg(target_os = "macos")]
            Self::Ghostty(e) => {
                let size = e.read(cx).terminal().map(|t| t.size());
                match size {
                    Some(s) => (s.columns as usize, s.rows as usize),
                    None => (80, 24),
                }
            }
        }
    }

    /// Search visible text. Returns empty for ghostty.
    pub fn search_text(&self, pattern: &str, limit: usize, cx: &App) -> Vec<(usize, String)> {
        match self {
            Self::Legacy(e) => e.read(cx).grid().lock().search_text(pattern, limit),
            #[cfg(target_os = "macos")]
            Self::Ghostty(_) => Vec::new(),
        }
    }
}

use crate::workspace::ConWorkspace;

/// Subscribe workspace to events from a TerminalPane.
/// Must be called from a `Context<ConWorkspace>`.
pub fn subscribe_terminal_pane(
    pane: &TerminalPane,
    window: &mut Window,
    cx: &mut Context<ConWorkspace>,
) {
    match pane {
        TerminalPane::Legacy(entity) => {
            cx.subscribe_in(entity, window, ConWorkspace::on_explain_command)
                .detach();
            cx.subscribe_in(entity, window, ConWorkspace::on_close_pane_request)
                .detach();
            cx.subscribe_in(entity, window, ConWorkspace::on_focus_changed)
                .detach();
            cx.subscribe_in(entity, window, ConWorkspace::on_input_changed)
                .detach();
        }
        #[cfg(target_os = "macos")]
        TerminalPane::Ghostty(entity) => {
            cx.subscribe_in(
                entity,
                window,
                ConWorkspace::on_ghostty_focus_changed,
            )
            .detach();
            cx.subscribe_in(
                entity,
                window,
                ConWorkspace::on_ghostty_process_exited,
            )
            .detach();
            cx.subscribe_in(
                entity,
                window,
                ConWorkspace::on_ghostty_title_changed,
            )
            .detach();
        }
    }
}
