use gpui::*;
use gpui_component::{
    ActiveTheme,
    input::{Input, InputEvent, InputState, Position},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    Sizable as _,
};

actions!(
    input_bar,
    [
        SubmitInput,
        EscapeInput,
        AcceptSuggestion,
        AcceptSuggestionOrMoveRight,
        AcceptSuggestionOrMoveEnd,
        DeletePreviousWord
    ]
);

pub struct SkillAutocompleteChanged;
impl EventEmitter<SkillAutocompleteChanged> for InputBar {}
pub struct InputEdited;
impl EventEmitter<InputEdited> for InputBar {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Smart,
    Shell,
    Agent,
}

impl InputMode {
    fn next(self) -> Self {
        match self {
            Self::Smart => Self::Shell,
            Self::Shell => Self::Agent,
            Self::Agent => Self::Smart,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Smart => "phosphor/magic-wand-duotone.svg",
            Self::Shell => "phosphor/terminal.svg",
            Self::Agent => "phosphor/oven-duotone.svg",
        }
    }

    fn tint(self, cx: &App) -> Hsla {
        match self {
            Self::Smart => cx.theme().muted_foreground,
            Self::Shell => cx.theme().success,
            Self::Agent => cx.theme().primary,
        }
    }
}

/// Pane info for the pane selector
#[derive(Clone)]
pub struct PaneInfo {
    pub id: usize,
    /// Display name (title, or cwd basename, or "Pane N")
    pub name: String,
    /// SSH hostname if connected via SSH
    pub hostname: Option<String>,
    /// Whether a command is currently running
    pub is_busy: bool,
    /// Whether the PTY process is still alive
    pub is_alive: bool,
}

/// A skill available for slash-command completion.
#[derive(Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuggestionSource {
    History,
    Ai,
}

pub struct InputBar {
    agent_input_state: Entity<InputState>,
    shell_input_state: Entity<InputState>,
    pane_target_select: Entity<SelectState<SearchableVec<String>>>,
    mode: InputMode,
    cwd: String,
    panes: Vec<PaneInfo>,
    selected_pane_ids: Vec<usize>,
    last_single_target_id: Option<usize>,
    focused_pane_id: usize,
    skills: Vec<SkillEntry>,
    /// Index of the highlighted skill in the filtered list (for arrow-key nav)
    skill_selection: usize,
    inline_suggestion_prefix: Option<String>,
    inline_suggestion_suffix: Option<String>,
    inline_suggestion_source: Option<SuggestionSource>,
    /// Tracks whether shift was held on the last enter keystroke.
    /// Set by observe_keystrokes (fires before PressEnter), consumed by PressEnter handler.
    shift_enter: bool,
    ui_opacity: f32,
    _subscriptions: Vec<Subscription>,
}

impl InputBar {
    pub fn init(cx: &mut App) {
        cx.bind_keys([
            KeyBinding::new(
                "right",
                AcceptSuggestionOrMoveRight,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "tab",
                AcceptSuggestion,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "ctrl-e",
                AcceptSuggestionOrMoveEnd,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "cmd-e",
                AcceptSuggestionOrMoveEnd,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "cmd-w",
                DeletePreviousWord,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "cmd-right",
                AcceptSuggestionOrMoveEnd,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "end",
                AcceptSuggestionOrMoveEnd,
                Some("ConCommandInput > Input"),
            ),
            KeyBinding::new(
                "ctrl-w",
                DeletePreviousWord,
                Some("ConCommandInput > Input"),
            ),
        ]);
    }

    fn make_pane_target_select(
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<SelectState<SearchableVec<String>>> {
        cx.new(|cx| {
            SelectState::new(SearchableVec::new(Vec::<String>::new()), None, window, cx)
                .searchable(true)
        })
    }

    fn current_input_state(&self) -> Entity<InputState> {
        match self.mode {
            InputMode::Agent => self.agent_input_state.clone(),
            InputMode::Smart | InputMode::Shell => self.shell_input_state.clone(),
        }
    }

    fn truncate_label(text: &str, truncate_at: usize, ellipsis_threshold: usize) -> String {
        if text.len() > ellipsis_threshold {
            format!("{}…", &text[..text.floor_char_boundary(truncate_at)])
        } else {
            text.to_string()
        }
    }

    fn pane_target_label(pane: &PaneInfo) -> String {
        let base = if let Some(host) = &pane.hostname {
            Self::truncate_label(host, 16, 22)
        } else if pane.name.is_empty() {
            format!("Pane {}", pane.id)
        } else {
            Self::truncate_label(&pane.name, 16, 22)
        };
        let status = if !pane.is_alive {
            "offline"
        } else if pane.is_busy {
            "busy"
        } else if pane.hostname.is_some() {
            "remote"
        } else {
            "local"
        };
        format!("{base} · {status} · #{}", pane.id)
    }

    fn current_single_target_id(&self) -> usize {
        self.selected_pane_ids
            .first()
            .copied()
            .unwrap_or(self.focused_pane_id)
    }

    fn sync_pane_target_select(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let labels: Vec<String> = self.panes.iter().map(Self::pane_target_label).collect();
        let selected_id = if !self.panes.is_empty() && self.selected_pane_ids.len() == self.panes.len()
        {
            self.last_single_target_id.unwrap_or(self.focused_pane_id)
        } else {
            self.current_single_target_id()
        };
        let selected_label = self
            .panes
            .iter()
            .find(|pane| pane.id == selected_id)
            .map(Self::pane_target_label);

        self.pane_target_select.update(cx, |select, cx| {
            select.set_items(SearchableVec::new(labels), window, cx);
            if let Some(label) = &selected_label {
                select.set_selected_value(label, window, cx);
            }
        });
    }

    fn subscribe_input_state(
        input_state: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let tracked_state = input_state.clone();
        let tracked_entity_id = tracked_state.entity_id();
        cx.subscribe_in(&tracked_state, window, {
            move |this, _, ev: &InputEvent, window, cx| {
                if tracked_entity_id != this.current_input_state().entity_id() {
                    return;
                }

                match ev {
                    InputEvent::Change => {
                        this.clear_inline_suggestion();
                        let matches = this.filtered_skills(cx);
                        if matches.is_empty() {
                            this.skill_selection = 0;
                        } else {
                            this.skill_selection =
                                this.skill_selection.min(matches.len().saturating_sub(1));
                        }
                        cx.emit(SkillAutocompleteChanged);
                        cx.emit(InputEdited);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => {
                        if this.shift_enter {
                            this.shift_enter = false;
                            return;
                        }

                        let active_state = this.current_input_state();
                        active_state.update(cx, |s, cx| {
                            let cursor = s.cursor();
                            let val = s.value().to_string();
                            if cursor > 0 && val.as_bytes().get(cursor - 1) == Some(&b'\n') {
                                let mut cleaned = val[..cursor - 1].to_string();
                                cleaned.push_str(&val[cursor..]);
                                s.set_value(&cleaned, window, cx);
                            }
                        });

                        let matches = this.filtered_skills(cx);
                        if !matches.is_empty() {
                            let idx = this.skill_selection.min(matches.len().saturating_sub(1));
                            let name = matches[idx].name.clone();
                            this.complete_skill(&name, window, cx);
                        } else {
                            let value = this.current_input_state().read(cx).value();
                            if !value.trim().is_empty() {
                                cx.emit(SubmitInput);
                            }
                        }
                    }
                    _ => {}
                }
            }
        })
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let agent_input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Ask anything…")
                .auto_grow(1, 6)
        });
        let shell_input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a command or ask AI…")
                .code_editor("bash")
                .multi_line(false)
                .folding(false)
        });
        shell_input_state.update(cx, |state, cx| {
            // Warm the single-line shell highlighter up-front so first focus/type
            // does not pay parser initialization latency on the UI path.
            state.set_value(":", window, cx);
            state.set_value("", window, cx);
            state.set_cursor_position(Position::new(0, 0), window, cx);
        });
        let pane_target_select = Self::make_pane_target_select(window, cx);

        let _subscriptions = vec![
            // Track shift state on enter keystrokes — fires BEFORE PressEnter
            cx.observe_keystrokes(|this, event, _window, _cx| {
                if event.keystroke.key == "enter" {
                    this.shift_enter = event.keystroke.modifiers.shift;
                }
            }),
            cx.subscribe_in(
                &pane_target_select,
                window,
                |this, _, event: &SelectEvent<SearchableVec<String>>, _, cx| {
                    if let SelectEvent::Confirm(Some(label)) = event {
                        this.select_pane_target_by_label(label, cx);
                    }
                },
            ),
            Self::subscribe_input_state(&agent_input_state, window, cx),
            Self::subscribe_input_state(&shell_input_state, window, cx),
        ];

        Self {
            agent_input_state,
            shell_input_state,
            pane_target_select,
            mode: InputMode::Smart,
            cwd: "~".to_string(),
            panes: Vec::new(),
            selected_pane_ids: Vec::new(),
            last_single_target_id: None,
            focused_pane_id: 0,
            skills: Vec::new(),
            skill_selection: 0,
            inline_suggestion_prefix: None,
            inline_suggestion_suffix: None,
            inline_suggestion_source: None,
            shift_enter: false,
            ui_opacity: 0.90,
            _subscriptions,
        }
    }

    pub fn take_content(&self, window: &mut Window, cx: &mut App) -> String {
        let input_state = self.current_input_state();
        let value = input_state.read(cx).value().to_string();
        input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        value
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    pub fn target_pane_ids(&self) -> Vec<usize> {
        if self.selected_pane_ids.is_empty() {
            vec![self.focused_pane_id]
        } else {
            self.selected_pane_ids.clone()
        }
    }

    pub fn set_cwd(&mut self, cwd: String) {
        self.cwd = cwd;
    }

    pub fn set_panes(
        &mut self,
        panes: Vec<PaneInfo>,
        focused_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.panes = panes;
        self.focused_pane_id = focused_id;
        let valid_ids: Vec<usize> = self.panes.iter().map(|p| p.id).collect();
        self.selected_pane_ids.retain(|id| valid_ids.contains(id));
        if let Some(last_single) = self.last_single_target_id {
            if !valid_ids.contains(&last_single) {
                self.last_single_target_id = None;
            }
        }
        self.sync_pane_target_select(window, cx);
    }

    pub fn set_skills(&mut self, skills: Vec<SkillEntry>) {
        self.skills = skills;
    }

    pub fn current_text(&self, cx: &App) -> String {
        self.current_input_state().read(cx).value().to_string()
    }

    pub fn clear_inline_suggestion(&mut self) {
        self.inline_suggestion_prefix = None;
        self.inline_suggestion_suffix = None;
        self.inline_suggestion_source = None;
    }

    fn set_inline_suggestion(
        &mut self,
        prefix: &str,
        suggestion: &str,
        source: SuggestionSource,
    ) {
        if prefix.is_empty() || suggestion.is_empty() {
            self.clear_inline_suggestion();
            return;
        }

        let suffix = if let Some(stripped) = suggestion.strip_prefix(prefix) {
            stripped
        } else {
            suggestion
        };
        if suffix.is_empty() || suffix.contains('\n') || suffix.contains('\r') {
            self.clear_inline_suggestion();
            return;
        }

        self.inline_suggestion_prefix = Some(prefix.to_string());
        self.inline_suggestion_suffix = Some(suffix.to_string());
        self.inline_suggestion_source = Some(source);
    }

    pub fn set_history_inline_suggestion(&mut self, prefix: &str, suggestion: &str) {
        self.set_inline_suggestion(prefix, suggestion, SuggestionSource::History);
    }

    pub fn set_ai_inline_suggestion(&mut self, prefix: &str, suggestion: &str) {
        self.set_inline_suggestion(prefix, suggestion, SuggestionSource::Ai);
    }

    pub fn accept_inline_suggestion(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(suffix) = self.inline_suggestion_suffix.clone() else {
            return false;
        };

        let input_state = self.current_input_state();
        let text = input_state.read(cx).value().to_string();
        let cursor = input_state.read(cx).cursor();
        if cursor != text.len() {
            self.clear_inline_suggestion();
            return false;
        }

        let mut completed = text;
        completed.push_str(&suffix);
        let completed_chars = completed.chars().count() as u32;
        input_state.update(cx, |s, cx| {
            s.set_value(&completed, window, cx);
            s.set_cursor_position(Position::new(0, completed_chars), window, cx);
        });
        self.clear_inline_suggestion();
        cx.emit(InputEdited);
        cx.notify();
        true
    }

    /// Return matching skills if the input starts with `/`.
    /// Public so the workspace can render the popup at overlay level.
    pub fn filtered_skills(&self, cx: &App) -> Vec<&SkillEntry> {
        let text = self.current_input_state().read(cx).value().to_string();
        let trimmed = text.trim();
        if !trimmed.starts_with('/') {
            return Vec::new();
        }
        let query = &trimmed[1..].to_lowercase();
        if query.contains(' ') {
            // Already has args — no autocomplete
            return Vec::new();
        }
        self.skills
            .iter()
            .filter(|s| query.is_empty() || s.name.to_lowercase().starts_with(query))
            .collect()
    }

    pub fn skill_selection(&self) -> usize {
        self.skill_selection
    }

    pub fn complete_skill(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.current_input_state().update(cx, |s, cx| {
            s.set_value(&format!("/{name} "), window, cx);
        });
        self.skill_selection = 0;
        self.clear_inline_suggestion();
        cx.emit(SkillAutocompleteChanged);
        cx.emit(InputEdited);
        cx.notify();
    }

    pub fn skill_popup_offset(&self, cx: &App) -> Pixels {
        let rows = self
            .current_input_state()
            .read(cx)
            .value()
            .lines()
            .count()
            .clamp(1, 6);

        px(56.0 + (rows.saturating_sub(1) as f32 * 20.0))
    }

    pub fn cycle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_mode(self.mode.next(), window, cx);
        self.clear_inline_suggestion();
        cx.emit(InputEdited);
        cx.notify();
    }

    fn select_pane_target_by_label(&mut self, label: &str, cx: &mut Context<Self>) {
        let Some(pane_id) = self
            .panes
            .iter()
            .find(|pane| Self::pane_target_label(pane) == label)
            .map(|pane| pane.id)
        else {
            return;
        };

        if pane_id == self.focused_pane_id {
            self.selected_pane_ids.clear();
            self.last_single_target_id = Some(pane_id);
        } else {
            self.selected_pane_ids = vec![pane_id];
            self.last_single_target_id = Some(pane_id);
        }
        self.clear_inline_suggestion();
        cx.emit(InputEdited);
        cx.notify();
    }

    fn toggle_select_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_pane_ids.len() == self.panes.len() {
            self.selected_pane_ids = self
                .last_single_target_id
                .filter(|id| *id != self.focused_pane_id)
                .map(|id| vec![id])
                .unwrap_or_default();
        } else {
            self.last_single_target_id = Some(self.current_single_target_id());
            self.selected_pane_ids = self.panes.iter().map(|p| p.id).collect();
            self.set_mode(InputMode::Shell, window, cx);
        }
        self.sync_pane_target_select(window, cx);
        self.clear_inline_suggestion();
        cx.emit(InputEdited);
        cx.notify();
    }

    fn set_mode(&mut self, mode: InputMode, window: &mut Window, cx: &mut Context<Self>) {
        let current_state = self.current_input_state();
        let current_value = current_state.read(cx).value().to_string();
        let current_position = current_state.read(cx).cursor_position();
        let was_focused = current_state.read(cx).focus_handle(cx).is_focused(window);

        self.mode = mode;
        let placeholder = self.placeholder().to_string();
        let next_state = self.current_input_state();
        next_state.update(cx, |s, cx| {
            s.set_placeholder(&placeholder, window, cx);
            s.set_value(&current_value, window, cx);
            s.set_cursor_position(current_position, window, cx);
            if was_focused {
                s.focus(window, cx);
            }
        });
    }

    pub fn set_ui_opacity(&mut self, opacity: f32) {
        self.ui_opacity = opacity.clamp(0.35, 1.0);
    }

    fn move_cursor_to_line_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input_state = self.current_input_state();
        let cursor_position = input_state.read(cx).cursor_position();
        let line_index = cursor_position.line as usize;
        let line_length = input_state
            .read(cx)
            .value()
            .lines()
            .nth(line_index)
            .map(|line| line.chars().count() as u32)
            .unwrap_or(cursor_position.character);

        input_state.update(cx, |state, cx| {
            state.set_cursor_position(Position::new(cursor_position.line, line_length), window, cx);
        });
    }

    fn move_cursor_right(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input_state = self.current_input_state();
        let text = input_state.read(cx).value().to_string();
        let cursor = input_state.read(cx).cursor();
        if cursor >= text.len() {
            return;
        }

        let next_offset = text[cursor..]
            .chars()
            .next()
            .map(|ch| cursor + ch.len_utf8())
            .unwrap_or(cursor);
        let next_position = Self::position_for_offset(&text, next_offset);

        input_state.update(cx, |state, cx| {
            state.set_cursor_position(next_position, window, cx);
        });
    }

    fn delete_previous_word(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let input_state = self.current_input_state();
        let text = input_state.read(cx).value().to_string();
        let cursor = input_state.read(cx).cursor();
        if cursor == 0 {
            return;
        }

        let mut boundary = cursor;
        while boundary > 0 {
            let prev = text[..boundary]
                .char_indices()
                .last()
                .map(|(idx, ch)| (idx, ch))
                .unwrap();
            if !prev.1.is_whitespace() {
                break;
            }
            boundary = prev.0;
        }
        while boundary > 0 {
            let prev = text[..boundary]
                .char_indices()
                .last()
                .map(|(idx, ch)| (idx, ch))
                .unwrap();
            if prev.1.is_whitespace() {
                break;
            }
            boundary = prev.0;
        }

        let mut updated = text[..boundary].to_string();
        updated.push_str(&text[cursor..]);
        let boundary_position = Self::position_for_offset(&updated, boundary);

        input_state.update(cx, |state, cx| {
            state.set_value(&updated, window, cx);
            state.set_cursor_position(boundary_position, window, cx);
        });
        self.clear_inline_suggestion();
        cx.emit(InputEdited);
        cx.notify();
    }

    fn position_for_offset(text: &str, offset: usize) -> Position {
        let safe_offset = offset.min(text.len());
        let prefix = &text[..safe_offset];
        let line = prefix.bytes().filter(|b| *b == b'\n').count() as u32;
        let character = prefix
            .rsplit_once('\n')
            .map(|(_, tail)| tail.chars().count() as u32)
            .unwrap_or_else(|| prefix.chars().count() as u32);
        Position::new(line, character)
    }

    fn placeholder(&self) -> &str {
        match self.mode {
            InputMode::Smart => "Type a command or ask AI…",
            InputMode::Shell => "Run a command…",
            InputMode::Agent => "Ask anything…",
        }
    }
}

impl EventEmitter<SubmitInput> for InputBar {}
impl EventEmitter<EscapeInput> for InputBar {}

impl Focusable for InputBar {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.current_input_state().read(cx).focus_handle(cx).clone()
    }
}

impl Render for InputBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let input_state = self.current_input_state();
        let has_multiple_panes = self.panes.len() > 1;
        let has_text = !input_state.read(cx).value().trim().is_empty();
        let input_value = input_state.read(cx).value().to_string();
        let input_cursor = input_state.read(cx).cursor();

        let mode_tint = self.mode.tint(cx);

        // ── Mode prefix — icon-only, minimal ──
        let mode_prefix = div()
            .id("mode-toggle")
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .bg(mode_tint.opacity(0.08))
            .hover(|s| s.bg(mode_tint.opacity(0.14)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.cycle_mode(window, cx);
                }),
            )
            .child(
                svg()
                    .path(self.mode.icon())
                    .size(px(14.0))
                    .text_color(mode_tint),
            );

        // ── Pane target control — scales to many panes without growing the row ──
        let all_selected =
            !self.panes.is_empty() && self.selected_pane_ids.len() == self.panes.len();
        let pane_row = if has_multiple_panes {
            let target_count = if all_selected {
                self.panes.len()
            } else {
                1
            };

            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .h(px(24.0))
                    .px(px(4.0))
                    .rounded(px(7.0))
                    .bg(theme.muted.opacity(0.08))
                    .min_w(px(0.0))
                    .max_w(px(244.0))
                    .child(
                        div()
                            .min_w(px(0.0))
                            .max_w(px(176.0))
                            .flex_1()
                            .child(
                                Select::new(&self.pane_target_select)
                                    .placeholder("Focused pane")
                                    .small(),
                            ),
                    )
                    .child(
                        div()
                            .h(px(14.0))
                            .w(px(1.0))
                            .bg(theme.border.opacity(0.55)),
                    )
                    .child(
                        div()
                            .id("pane-sel-all")
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap(px(4.0))
                            .h(px(18.0))
                            .px(px(6.0))
                            .rounded(px(5.0))
                            .flex_shrink_0()
                            .text_size(px(9.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .cursor_pointer()
                            .bg(if all_selected {
                                theme.primary.opacity(0.12)
                            } else {
                                theme.transparent
                            })
                            .text_color(if all_selected {
                                theme.primary
                            } else {
                                theme.muted_foreground.opacity(0.5)
                            })
                            .hover(|s| s.bg(theme.muted.opacity(0.10)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, window, cx| {
                                    this.toggle_select_all(window, cx);
                                }),
                            )
                            .child(
                                svg()
                                    .path("phosphor/broadcast-duotone.svg")
                                    .size(px(11.0))
                                    .text_color(if all_selected {
                                        theme.primary
                                    } else {
                                        theme.muted_foreground.opacity(0.45)
                                    }),
                            )
                            .child(if all_selected {
                                format!("{target_count} panes")
                            } else {
                                "Broadcast".to_string()
                            }),
                    ),
            )
        } else {
            None
        };

        // ── Send button — inside container, right edge ──
        let send_button = div()
            .id("send-button")
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.0))
            .rounded(px(12.0))
            .cursor_pointer()
            .flex_shrink_0()
            .bg(if has_text {
                theme.primary
            } else {
                theme.muted.opacity(0.12)
            })
            .hover(|s| {
                if has_text {
                    s.bg(theme.primary_hover)
                } else {
                    s.bg(theme.muted.opacity(0.18))
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _, _, cx| {
                    cx.emit(SubmitInput);
                }),
            )
            .child(
                svg()
                    .path("phosphor/arrow-up.svg")
                    .size(px(12.0))
                    .text_color(if has_text {
                        theme.primary_foreground
                    } else {
                        theme.muted_foreground.opacity(0.4)
                    }),
            );

        // Font: mono for shell/smart, system for agent
        let input_font = match self.mode {
            InputMode::Agent => theme.font_family.clone(),
            _ => theme.mono_font_family.clone(),
        };
        let input_text_size = match self.mode {
            InputMode::Agent => px(14.0),
            _ => px(13.0),
        };
        let input_line_height = match self.mode {
            InputMode::Agent => rems(1.15),
            _ => rems(1.25),
        };
        let input_vertical_offset = match self.mode {
            InputMode::Agent => px(-2.0),
            _ => px(0.0),
        };
        let show_inline_suggestion = self.mode != InputMode::Agent
            && input_cursor == input_value.len()
            && !input_value.is_empty()
            && !input_value.contains('\n')
            && !input_value.trim_start().starts_with('/')
            && self
                .inline_suggestion_prefix
                .as_deref()
                .is_some_and(|prefix| prefix == input_value)
            && self
                .inline_suggestion_suffix
                .as_deref()
                .is_some_and(|suffix| !suffix.is_empty());
        let ghost_tint = match self.inline_suggestion_source {
            Some(SuggestionSource::Ai) => theme.primary.opacity(0.42),
            _ => theme.muted_foreground.opacity(0.42),
        };
        let ghost_prefix = input_value.replace(' ', "\u{00A0}");
        let ghost_suffix = self
            .inline_suggestion_suffix
            .clone()
            .unwrap_or_default()
            .replace(' ', "\u{00A0}");

        let input_field = div()
            .flex_1()
            .min_h(px(24.0))
            .relative()
            .key_context("ConCommandInput")
            .on_action(cx.listener(
                |this, _: &AcceptSuggestion, window, cx| {
                    let _ = this.accept_inline_suggestion(window, cx);
                },
            ))
            .on_action(cx.listener(
                |this, _: &AcceptSuggestionOrMoveEnd, window, cx| {
                    if !this.accept_inline_suggestion(window, cx) {
                        this.move_cursor_to_line_end(window, cx);
                    }
                },
            ))
            .on_action(cx.listener(
                |this, _: &AcceptSuggestionOrMoveRight, window, cx| {
                    if !this.accept_inline_suggestion(window, cx) {
                        this.move_cursor_right(window, cx);
                    }
                },
            ))
            .on_action(cx.listener(|this, _: &DeletePreviousWord, window, cx| {
                this.delete_previous_word(window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let matches = this.filtered_skills(cx);
                let has_completions = !matches.is_empty();
                let mods = event.keystroke.modifiers;
                let key = event.keystroke.key.as_str();

                match key {
                    "tab" => {
                        if has_completions {
                            let idx = this.skill_selection.min(matches.len().saturating_sub(1));
                            let name = matches[idx].name.clone();
                            this.complete_skill(&name, window, cx);
                            window.prevent_default();
                            cx.stop_propagation();
                        } else if this.accept_inline_suggestion(window, cx) {
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }
                    "escape" => {
                        if has_completions {
                            this.current_input_state()
                                .update(cx, |s, cx| s.set_value("", window, cx));
                            this.skill_selection = 0;
                            this.clear_inline_suggestion();
                            cx.emit(SkillAutocompleteChanged);
                            cx.emit(InputEdited);
                            cx.notify();
                            window.prevent_default();
                            cx.stop_propagation();
                        } else if this.inline_suggestion_suffix.is_some() {
                            this.clear_inline_suggestion();
                            cx.emit(InputEdited);
                            cx.notify();
                            window.prevent_default();
                            cx.stop_propagation();
                        } else {
                            cx.emit(EscapeInput);
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }
                    "up" if has_completions => {
                        this.skill_selection = this.skill_selection.saturating_sub(1);
                        cx.emit(SkillAutocompleteChanged);
                        cx.notify();
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                    "down" if has_completions => {
                        this.skill_selection =
                            (this.skill_selection + 1).min(matches.len().saturating_sub(1));
                        cx.emit(SkillAutocompleteChanged);
                        cx.notify();
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                    "right" if !mods.control && !mods.platform && !mods.alt => {
                        if this.accept_inline_suggestion(window, cx) {
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }
                    "end" => {
                        if this.accept_inline_suggestion(window, cx) {
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }
                    "e" if mods.control || mods.platform => {
                        if !this.accept_inline_suggestion(window, cx) {
                            this.move_cursor_to_line_end(window, cx);
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                    "right" if mods.platform => {
                        if this.accept_inline_suggestion(window, cx) {
                            window.prevent_default();
                            cx.stop_propagation();
                        }
                    }
                    "w" if mods.control || mods.platform => {
                        this.delete_previous_word(window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .font_family(input_font.clone())
            .text_size(input_text_size)
            .children(show_inline_suggestion.then(|| {
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_0()
                    .bottom_0()
                    .px(px(12.0))
                    .flex()
                    .items_center()
                    .line_height(input_line_height)
                    .top(input_vertical_offset)
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .opacity(0.0)
                                    .child(ghost_prefix),
                            )
                            .child(div().text_color(ghost_tint).child(ghost_suffix)),
                    )
            }))
            .child(
                div()
                    .relative()
                    .top(input_vertical_offset)
                    .child(if self.mode == InputMode::Agent {
                        Input::new(&input_state)
                            .appearance(false)
                            .cleanable(false)
                            .font_family(input_font)
                            .text_size(input_text_size)
                            .line_height(input_line_height)
                            .pl(px(0.0))
                            .h(px(24.0))
                            .into_any_element()
                    } else {
                        Input::new(&input_state)
                            .appearance(false)
                            .cleanable(false)
                            .font_family(input_font)
                            .text_size(input_text_size)
                            .line_height(input_line_height)
                            .h(px(24.0))
                            .into_any_element()
                    }),
            );

        // ── Main layout — flat bar, no rounded bubble ──
        div()
            .flex()
            .flex_col()
            .bg(theme.title_bar.opacity(self.ui_opacity))
            .font_family(theme.font_family.clone())
            .text_size(input_text_size)
            // ── Flat container ──
            .child(
                div()
                    .px(px(12.0))
                    .py(px(7.0))
                    .min_h(px(42.0))
                    .flex()
                    .items_center()
                    .h(px(30.0))
                    .gap(px(8.0))
                    .child(mode_prefix)
                    .children(pane_row)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(input_field),
                    )
                    .child(send_button),
            )
    }
}
