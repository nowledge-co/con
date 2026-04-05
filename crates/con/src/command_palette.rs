use gpui::*;
use gpui_component::input::InputState;
use gpui_component::scroll::ScrollableElement;
use gpui_component::{ActiveTheme, input::Input};

actions!(command_palette, [ToggleCommandPalette]);

/// A command palette action entry
#[derive(Clone)]
struct PaletteAction {
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
    category: &'static str,
}

const PALETTE_ACTIONS: &[PaletteAction] = &[
    PaletteAction {
        id: "new-tab",
        label: "New Tab",
        shortcut: "⌘T",
        category: "Terminal",
    },
    PaletteAction {
        id: "close-tab",
        label: "Close Tab",
        shortcut: "⌘W",
        category: "Terminal",
    },
    PaletteAction {
        id: "clear-terminal",
        label: "Clear Terminal",
        shortcut: "⌘K",
        category: "Terminal",
    },
    PaletteAction {
        id: "focus-terminal",
        label: "Focus Terminal",
        shortcut: "",
        category: "Terminal",
    },
    PaletteAction {
        id: "toggle-agent",
        label: "Toggle Agent Panel",
        shortcut: "⌘L",
        category: "Agent",
    },
    PaletteAction {
        id: "cycle-input-mode",
        label: "Cycle Input Mode",
        shortcut: "Tab",
        category: "Input",
    },
    PaletteAction {
        id: "split-right",
        label: "Split Right",
        shortcut: "⌘D",
        category: "Pane",
    },
    PaletteAction {
        id: "split-down",
        label: "Split Down",
        shortcut: "⇧⌘D",
        category: "Pane",
    },
    PaletteAction {
        id: "toggle-sidebar",
        label: "Toggle Sidebar",
        shortcut: "",
        category: "View",
    },
    PaletteAction {
        id: "settings",
        label: "Open Settings",
        shortcut: "⌘,",
        category: "Settings",
    },
    PaletteAction {
        id: "quit",
        label: "Quit",
        shortcut: "⌘Q",
        category: "App",
    },
];

/// Command palette overlay — searchable action list
pub struct CommandPalette {
    visible: bool,
    query: Entity<InputState>,
    query_text: String,
    selected_index: usize,
    focus_handle: FocusHandle,
    scroll_handle: ScrollHandle,
}

/// Emitted when the user selects an action
pub struct PaletteSelect {
    pub action_id: String,
}

impl EventEmitter<PaletteSelect> for CommandPalette {}

impl CommandPalette {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_placeholder("Type a command...", window, cx);
            state
        });

        Self {
            visible: false,
            query,
            query_text: String::new(),
            selected_index: 0,
            focus_handle: cx.focus_handle(),
            scroll_handle: ScrollHandle::new(),
        }
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.visible = !self.visible;
        if self.visible {
            self.query_text.clear();
            self.selected_index = 0;
            self.query.update(cx, |s, cx| {
                s.set_value("", window, cx);
                // Focus the input directly so the user can type immediately
                s.focus(window, cx);
            });
        }
        cx.notify();
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    fn filtered_actions(&self) -> Vec<&PaletteAction> {
        if self.query_text.is_empty() {
            return PALETTE_ACTIONS.iter().collect();
        }
        let query = self.query_text.to_lowercase();
        PALETTE_ACTIONS
            .iter()
            .filter(|a| {
                a.label.to_lowercase().contains(&query)
                    || a.category.to_lowercase().contains(&query)
                    || a.id.contains(&query)
            })
            .collect()
    }

    fn select_action(&mut self, cx: &mut Context<Self>) {
        let actions = self.filtered_actions();
        if let Some(action) = actions.get(self.selected_index) {
            let id = action.id.to_string();
            self.visible = false;
            cx.emit(PaletteSelect { action_id: id });
            cx.notify();
        }
    }
}

impl Focusable for CommandPalette {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div().id("palette-overlay");
        }

        // Read current query text from input state
        self.query_text = self.query.read(cx).value().to_string();
        let actions = self.filtered_actions();
        let selected = self.selected_index;

        let theme = cx.theme();

        let mut list = div()
            .id("palette-list")
            .flex()
            .flex_col()
            .max_h(px(320.0))
            .py(px(6.0))
            .px(px(6.0))
            .overflow_y_scroll()
            .track_scroll(&self.scroll_handle)
            .vertical_scrollbar(&self.scroll_handle);

        for (i, action) in actions.iter().enumerate() {
            let is_selected = i == selected;
            let idx = i;

            list = list.child(
                div()
                    .id(SharedString::from(format!("palette-{}", action.id)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .bg(if is_selected {
                        theme.primary
                    } else {
                        theme.transparent
                    })
                    .text_color(if is_selected {
                        theme.primary_foreground
                    } else {
                        theme.foreground
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.selected_index = idx;
                            this.select_action(cx);
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .items_center()
                            .gap(px(8.0))
                            .overflow_x_hidden()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(if is_selected {
                                        theme.primary_foreground
                                    } else {
                                        theme.muted_foreground
                                    })
                                    .min_w(px(60.0))
                                    .child(action.category),
                            )
                            .child(div().text_sm().child(action.label)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .flex_shrink_0()
                            .text_color(if is_selected {
                                theme.primary_foreground
                            } else {
                                theme.muted_foreground
                            })
                            .child(action.shortcut),
                    ),
            );
        }

        if actions.is_empty() {
            list = list.child(
                div()
                    .px(px(16.0))
                    .py(px(12.0))
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No matching commands"),
            );
        }

        // Scroll selected item into view
        self.scroll_handle.scroll_to_item(selected);

        let backdrop = div()
            .id("palette-backdrop")
            .occlude()
            .absolute()
            .size_full()
            .bg(theme.background.opacity(0.7))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.visible = false;
                    cx.notify();
                }),
            );

        let card = div()
            .absolute()
            .top(px(60.0))
            .left_0()
            .right_0()
            .mx_auto()
            .w(px(520.0))
            .rounded(px(12.0))
            .bg(theme.title_bar)
            .flex()
            .flex_col()
            .overflow_hidden()
            .occlude()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let actions = this.filtered_actions();
                let count = actions.len();
                match event.keystroke.key.as_str() {
                    "escape" => {
                        this.visible = false;
                        cx.notify();
                    }
                    "enter" => {
                        this.select_action(cx);
                    }
                    "up" => {
                        if count > 0 {
                            this.selected_index = if this.selected_index == 0 {
                                count - 1
                            } else {
                                this.selected_index - 1
                            };
                            cx.notify();
                        }
                    }
                    "down" => {
                        if count > 0 {
                            this.selected_index = (this.selected_index + 1) % count;
                            cx.notify();
                        }
                    }
                    _ => {}
                }
            }))
            .child(div().p(px(12.0)).child(Input::new(&self.query)))
            .child(list);

        div()
            .id("palette-overlay")
            .absolute()
            .size_full()
            .child(backdrop)
            .child(card)
    }
}
