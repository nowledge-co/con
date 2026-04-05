use gpui::*;
use gpui_component::{
    ActiveTheme, Icon,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent, InputState},
};

actions!(input_bar, [SubmitInput, EscapeInput]);

pub struct SkillAutocompleteChanged;
impl EventEmitter<SkillAutocompleteChanged> for InputBar {}

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

    fn label(&self) -> &str {
        match self {
            Self::Smart => "Auto",
            Self::Shell => "Shell",
            Self::Agent => "Agent",
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

pub struct InputBar {
    input_state: Entity<InputState>,
    mode: InputMode,
    cwd: String,
    panes: Vec<PaneInfo>,
    selected_pane_ids: Vec<usize>,
    focused_pane_id: usize,
    skills: Vec<SkillEntry>,
    /// Index of the highlighted skill in the filtered list (for arrow-key nav)
    skill_selection: usize,
    /// Tracks whether shift was held on the last enter keystroke.
    /// Set by observe_keystrokes (fires before PressEnter), consumed by PressEnter handler.
    shift_enter: bool,
    _subscriptions: Vec<Subscription>,
}

impl InputBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a command or ask AI...")
                .auto_grow(1, 6)
        });

        let _subscriptions = vec![
            // Track shift state on enter keystrokes — fires BEFORE PressEnter
            cx.observe_keystrokes(|this, event, _window, _cx| {
                if event.keystroke.key == "enter" {
                    this.shift_enter = event.keystroke.modifiers.shift;
                }
            }),
            cx.subscribe_in(&input_state, window, {
                move |this, _, ev: &InputEvent, window, cx| {
                    match ev {
                        InputEvent::Change => {
                            let matches = this.filtered_skills(cx);
                            if matches.is_empty() {
                                this.skill_selection = 0;
                            } else {
                                this.skill_selection =
                                    this.skill_selection.min(matches.len().saturating_sub(1));
                            }
                            cx.emit(SkillAutocompleteChanged);
                            cx.notify();
                        }
                        InputEvent::PressEnter { .. } => {
                            if this.shift_enter {
                                // Shift+Enter: newline already inserted by auto_grow
                                this.shift_enter = false;
                                return;
                            }

                            // Regular Enter: undo the newline auto_grow inserted, then submit
                            this.input_state.update(cx, |s, cx| {
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
                                let value = this.input_state.read(cx).value();
                                if !value.trim().is_empty() {
                                    cx.emit(SubmitInput);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }),
        ];

        Self {
            input_state,
            mode: InputMode::Smart,
            cwd: "~".to_string(),
            panes: Vec::new(),
            selected_pane_ids: Vec::new(),
            focused_pane_id: 0,
            skills: Vec::new(),
            skill_selection: 0,
            shift_enter: false,
            _subscriptions,
        }
    }

    pub fn take_content(&self, window: &mut Window, cx: &mut App) -> String {
        let value = self.input_state.read(cx).value().to_string();
        self.input_state.update(cx, |state, cx| {
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

    pub fn set_panes(&mut self, panes: Vec<PaneInfo>, focused_id: usize) {
        self.panes = panes;
        self.focused_pane_id = focused_id;
        let valid_ids: Vec<usize> = self.panes.iter().map(|p| p.id).collect();
        self.selected_pane_ids.retain(|id| valid_ids.contains(id));
    }

    pub fn set_skills(&mut self, skills: Vec<SkillEntry>) {
        self.skills = skills;
    }

    /// Return matching skills if the input starts with `/`.
    /// Public so the workspace can render the popup at overlay level.
    pub fn filtered_skills(&self, cx: &App) -> Vec<&SkillEntry> {
        let text = self.input_state.read(cx).value().to_string();
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
        self.input_state.update(cx, |s, cx| {
            s.set_value(&format!("/{name} "), window, cx);
        });
        self.skill_selection = 0;
        cx.emit(SkillAutocompleteChanged);
        cx.notify();
    }

    pub fn skill_popup_offset(&self, cx: &App) -> Pixels {
        let rows = self
            .input_state
            .read(cx)
            .value()
            .lines()
            .count()
            .clamp(1, 6);

        px(84.0 + (rows.saturating_sub(1) as f32 * 22.0))
    }

    pub fn cycle_mode(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.mode = self.mode.next();
        cx.notify();
    }

    fn toggle_pane_selection(&mut self, pane_id: usize, cx: &mut Context<Self>) {
        if let Some(pos) = self.selected_pane_ids.iter().position(|&id| id == pane_id) {
            self.selected_pane_ids.remove(pos);
        } else {
            self.selected_pane_ids.push(pane_id);
        }
        cx.notify();
    }

    fn toggle_select_all(&mut self, cx: &mut Context<Self>) {
        if self.selected_pane_ids.len() == self.panes.len() {
            // Deselect all — reverts to focused-pane-only
            self.selected_pane_ids.clear();
        } else {
            // Select all
            self.selected_pane_ids = self.panes.iter().map(|p| p.id).collect();
        }
        cx.notify();
    }

    #[allow(dead_code)]
    fn placeholder(&self) -> &str {
        match self.mode {
            InputMode::Smart => "Type a command or ask AI...",
            InputMode::Shell => "Type a shell command...",
            InputMode::Agent => "Ask the AI agent...",
        }
    }
}

impl EventEmitter<SubmitInput> for InputBar {}
impl EventEmitter<EscapeInput> for InputBar {}

impl Focusable for InputBar {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input_state.read(cx).focus_handle(cx).clone()
    }
}

impl Render for InputBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let has_multiple_panes = self.panes.len() > 1;
        let cwd = self.cwd.clone();

        let control_h = px(30.0);
        let pill_h = px(28.0);
        let control_radius = px(10.0);
        let inner_radius = px(8.0);
        let mode_label = self.mode.label().to_string();
        let mode_tint = self.mode.tint(cx);
        let mode_button = div()
            .id("mode-toggle")
            .flex()
            .items_center()
            .gap(px(6.0))
            .h(control_h)
            .px(px(10.0))
            .rounded(control_radius)
            .cursor_pointer()
            .bg(theme.background.opacity(0.76))
            .hover(|s| s.bg(theme.background.opacity(0.94)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.cycle_mode(window, cx);
                }),
            )
            .child(div().size(px(6.0)).rounded_full().bg(mode_tint))
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(mode_label),
            )
            .child(
                div()
                    .h(px(18.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .bg(theme.muted.opacity(0.10))
                    .text_size(px(9.5))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground.opacity(0.72))
                    .child("⇥"),
            );

        let all_selected =
            !self.panes.is_empty() && self.selected_pane_ids.len() == self.panes.len();
        let pane_area = if has_multiple_panes {
            let mut pills = div().flex().items_center().gap(px(2.0));

            for pane in &self.panes {
                let pane_id = pane.id;
                let is_target = if self.selected_pane_ids.is_empty() {
                    pane.id == self.focused_pane_id
                } else {
                    self.selected_pane_ids.contains(&pane.id)
                };

                let dot_color = if !pane.is_alive {
                    theme.danger
                } else if pane.is_busy {
                    theme.warning
                } else if pane.hostname.is_some() {
                    theme.primary
                } else {
                    theme.success
                };

                let label = if let Some(host) = &pane.hostname {
                    if host.len() > 12 {
                        format!("{}…", &host[..10])
                    } else {
                        host.clone()
                    }
                } else if pane.name.len() > 14 {
                    format!("{}…", &pane.name[..12])
                } else {
                    pane.name.clone()
                };

                let pill = div()
                    .id(SharedString::from(format!("pane-sel-{pane_id}")))
                    .flex()
                    .items_center()
                    .gap(px(5.0))
                    .h(pill_h)
                    .px(px(10.0))
                    .rounded(inner_radius)
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .bg(if is_target {
                        theme.background
                    } else {
                        theme.transparent
                    })
                    .text_color(if is_target {
                        theme.foreground
                    } else {
                        theme.muted_foreground.opacity(0.6)
                    })
                    .hover(|s| {
                        if is_target {
                            s
                        } else {
                            s.bg(theme.muted.opacity(0.10))
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_pane_selection(pane_id, cx);
                        }),
                    )
                    .child(div().size(px(6.0)).rounded_full().bg(dot_color))
                    .child(label);

                pills = pills.child(pill);
            }

            let all_btn = div()
                .id("pane-sel-all")
                .flex()
                .items_center()
                .justify_center()
                .h(pill_h)
                .px(px(8.0))
                .rounded(inner_radius)
                .text_size(px(10.0))
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
                    theme.muted_foreground.opacity(0.4)
                })
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.toggle_select_all(cx);
                    }),
                )
                .child("All");

            Some(
                div()
                    .flex()
                    .items_center()
                    .h(control_h)
                    .px(px(2.0))
                    .rounded(control_radius)
                    .bg(theme.background.opacity(0.70))
                    .gap(px(2.0))
                    .child(pills)
                    .child(all_btn),
            )
        } else {
            None
        };

        let send_button = Button::new("send-button")
            .icon(Icon::default().path("phosphor/arrow-up.svg"))
            .primary()
            .rounded(px(16.0))
            .tooltip("Send")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _, _, cx| {
                    cx.emit(SubmitInput);
                }),
            );

        div()
            .flex()
            .flex_col()
            .bg(theme.title_bar)
            .font_family(".SystemUIFont")
            .text_size(px(13.0))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let matches = this.filtered_skills(cx);
                let has_completions = !matches.is_empty();
                match event.keystroke.key.as_str() {
                    "escape" => {
                        if has_completions {
                            this.input_state
                                .update(cx, |s, cx| s.set_value("", window, cx));
                            this.skill_selection = 0;
                            cx.emit(SkillAutocompleteChanged);
                            cx.notify();
                        } else {
                            cx.emit(EscapeInput);
                        }
                    }
                    "tab" if has_completions => {
                        let idx = this.skill_selection.min(matches.len().saturating_sub(1));
                        let name = matches[idx].name.clone();
                        this.complete_skill(&name, window, cx);
                    }
                    "tab" if !event.keystroke.modifiers.shift => {
                        this.cycle_mode(window, cx);
                    }
                    "up" if has_completions => {
                        this.skill_selection = this.skill_selection.saturating_sub(1);
                        cx.emit(SkillAutocompleteChanged);
                        cx.notify();
                    }
                    "down" if has_completions => {
                        this.skill_selection =
                            (this.skill_selection + 1).min(matches.len().saturating_sub(1));
                        cx.emit(SkillAutocompleteChanged);
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .flex()
                    .items_end()
                    .gap(px(8.0))
                    .px(px(12.0))
                    .pt(px(8.0))
                    .pb(px(2.0))
                    .child(mode_button)
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(8.0))
                            .min_h(px(42.0))
                            .px(px(10.0))
                            .py(px(8.0))
                            .flex_1()
                            .rounded(control_radius)
                            .bg(theme.background.opacity(0.82))
                            .child(
                                div().flex_1().font_family("Ioskeley Mono").child(
                                    Input::new(&self.input_state)
                                        .appearance(false)
                                        .cleanable(false),
                                ),
                            )
                            .child(send_button),
                    )
                    .children(pane_area),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(18.0))
                    .px(px(14.0))
                    .pt(px(1.0))
                    .pb(px(6.0))
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.muted_foreground.opacity(0.4))
                            .child(cwd),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.muted_foreground.opacity(0.25))
                            .child("/ skills  ⇧↵ newline  ↵ send"),
                    ),
            )
    }
}
