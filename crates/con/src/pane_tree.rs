use gpui::*;
use gpui_component::ActiveTheme;

use crate::terminal_pane::TerminalPane;

/// Split direction for pane layout
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SplitPlacement {
    Before,
    After,
}

/// Unique identifier for a leaf pane
type PaneId = usize;

/// Unique identifier for a split node
pub type SplitId = usize;

/// A node in the pane tree — either a single terminal or a split
enum PaneNode {
    Leaf {
        id: PaneId,
        terminal: TerminalPane,
    },
    Split {
        split_id: SplitId,
        direction: SplitDirection,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
        ratio: f32,
    },
}

/// Active drag state for a split divider
#[derive(Clone)]
pub struct DragState {
    /// Which split node is being dragged
    pub split_id: SplitId,
    /// The ratio at the moment the drag began
    pub start_ratio: f32,
    /// Cursor position (in the axis of the split) when the drag began, in px
    pub start_pos: f32,
}

/// Manages a tree of split panes within a single tab
pub struct PaneTree {
    root: PaneNode,
    focused_pane_id: PaneId,
    next_id: PaneId,
    next_split_id: SplitId,
    /// Active divider drag, if any
    pub dragging: Option<DragState>,
}

impl PaneTree {
    pub fn new(terminal: TerminalPane) -> Self {
        Self {
            root: PaneNode::Leaf { id: 0, terminal },
            focused_pane_id: 0,
            next_id: 1,
            next_split_id: 0,
            dragging: None,
        }
    }

    /// Get the focused terminal
    pub fn focused_terminal(&self) -> &TerminalPane {
        Self::find_terminal(&self.root, self.focused_pane_id)
            .unwrap_or_else(|| Self::first_terminal(&self.root))
    }

    /// Get all terminals in the tree
    pub fn all_terminals(&self) -> Vec<&TerminalPane> {
        let mut result = Vec::new();
        Self::collect_terminals(&self.root, &mut result);
        result
    }

    /// Split the focused pane
    pub fn split(&mut self, direction: SplitDirection, new_terminal: TerminalPane) {
        self.split_with_placement(direction, SplitPlacement::After, new_terminal);
    }

    pub fn split_with_placement(
        &mut self,
        direction: SplitDirection,
        placement: SplitPlacement,
        new_terminal: TerminalPane,
    ) {
        let new_id = self.next_id;
        self.next_id += 1;
        let new_split_id = self.next_split_id;
        self.next_split_id += 1;
        Self::split_node(
            &mut self.root,
            self.focused_pane_id,
            direction,
            placement,
            new_id,
            new_terminal,
            new_split_id,
        );
        self.focused_pane_id = new_id;
    }

    pub fn split_pane_with_placement(
        &mut self,
        target_pane_id: PaneId,
        direction: SplitDirection,
        placement: SplitPlacement,
        new_terminal: TerminalPane,
    ) {
        let new_id = self.next_id;
        self.next_id += 1;
        let new_split_id = self.next_split_id;
        self.next_split_id += 1;
        if Self::split_node(
            &mut self.root,
            target_pane_id,
            direction,
            placement,
            new_id,
            new_terminal,
            new_split_id,
        ) {
            self.focused_pane_id = new_id;
        }
    }

    /// Close the focused pane, returning true if the tree still has panes
    pub fn close_focused(&mut self) -> bool {
        self.close_pane(self.focused_pane_id)
    }

    /// Close a specific pane by ID, returning true if the tree still has panes
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        if self.pane_count() <= 1 {
            return false;
        }
        let placeholder_terminal = Self::first_terminal(&self.root).clone();
        let old_root = std::mem::replace(
            &mut self.root,
            PaneNode::Leaf {
                id: 0,
                terminal: placeholder_terminal,
            },
        );
        self.root = Self::remove_leaf(old_root, pane_id);
        if Self::find_terminal(&self.root, self.focused_pane_id).is_none() {
            self.focused_pane_id = Self::first_pane_id(&self.root);
        }
        true
    }

    /// Set focus to a pane by ID
    #[allow(dead_code)]
    pub fn focus(&mut self, pane_id: PaneId) {
        if Self::find_terminal(&self.root, pane_id).is_some() {
            self.focused_pane_id = pane_id;
        }
    }

    /// Update focused pane based on which terminal currently has window focus.
    pub fn sync_focus(&mut self, window: &Window, cx: &App) {
        if let Some(id) = Self::find_focused_pane(&self.root, window, cx) {
            self.focused_pane_id = id;
        }
    }

    /// Get all pane IDs with their terminal panes
    pub fn pane_terminals(&self) -> Vec<(PaneId, TerminalPane)> {
        let mut result = Vec::new();
        Self::collect_pane_terminals(&self.root, &mut result);
        result
    }

    pub fn focused_pane_id(&self) -> PaneId {
        self.focused_pane_id
    }

    /// Find the pane ID for a given terminal by entity ID
    pub fn pane_id_for_entity(&self, entity_id: EntityId) -> Option<PaneId> {
        Self::find_pane_id_by_entity_id(&self.root, entity_id)
    }

    /// Find the pane ID for a given terminal pane
    pub fn pane_id_for_terminal(&self, terminal: &TerminalPane) -> Option<PaneId> {
        self.pane_id_for_entity(terminal.entity_id())
    }

    /// Check if a given terminal belongs to a specific pane ID
    pub fn terminal_has_pane_id(&self, terminal: &TerminalPane, pane_id: PaneId) -> bool {
        Self::check_terminal_pane_id(&self.root, terminal.entity_id(), pane_id)
    }

    /// Number of panes
    pub fn pane_count(&self) -> usize {
        Self::count_leaves(&self.root)
    }

    /// Start a drag on the given split divider.
    pub fn begin_drag(&mut self, split_id: SplitId, start_pos: f32) {
        if let Some((ratio, _dir)) = Self::find_split_ratio(&self.root, split_id) {
            self.dragging = Some(DragState {
                split_id,
                start_ratio: ratio,
                start_pos,
            });
        }
    }

    /// Update the drag: move the divider based on new cursor position and total container size.
    pub fn update_drag(&mut self, current_pos: f32, total_size: f32) -> bool {
        let drag = match &self.dragging {
            Some(d) => d.clone(),
            None => return false,
        };
        if total_size <= 0.0 {
            return false;
        }
        let delta = current_pos - drag.start_pos;
        let new_ratio = (drag.start_ratio + delta / total_size).clamp(0.05, 0.95);
        Self::set_split_ratio(&mut self.root, drag.split_id, new_ratio)
    }

    /// Finish a drag operation.
    pub fn end_drag(&mut self) {
        self.dragging = None;
    }

    /// Whether a drag is currently in progress
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Returns the split direction of the currently-dragged split, if any.
    pub fn dragging_direction(&self) -> Option<SplitDirection> {
        let drag = self.dragging.as_ref()?;
        Self::find_split_ratio(&self.root, drag.split_id).map(|(_, dir)| dir)
    }

    /// Render the pane tree as a GPUI element.
    pub fn render(&self, begin_drag_cb: impl Fn(SplitId, f32) + 'static, cx: &App) -> AnyElement {
        Self::render_node(
            &self.root,
            self.focused_pane_id,
            self.pane_count() > 1,
            std::sync::Arc::new(begin_drag_cb),
            cx,
        )
    }

    // --- Private helpers ---

    fn find_terminal(node: &PaneNode, target_id: PaneId) -> Option<&TerminalPane> {
        match node {
            PaneNode::Leaf { id, terminal } if *id == target_id => Some(terminal),
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { first, second, .. } => Self::find_terminal(first, target_id)
                .or_else(|| Self::find_terminal(second, target_id)),
        }
    }

    fn first_terminal(node: &PaneNode) -> &TerminalPane {
        match node {
            PaneNode::Leaf { terminal, .. } => terminal,
            PaneNode::Split { first, .. } => Self::first_terminal(first),
        }
    }

    #[allow(dead_code)]
    fn first_pane_id(node: &PaneNode) -> PaneId {
        match node {
            PaneNode::Leaf { id, .. } => *id,
            PaneNode::Split { first, .. } => Self::first_pane_id(first),
        }
    }

    fn collect_terminals<'a>(node: &'a PaneNode, result: &mut Vec<&'a TerminalPane>) {
        match node {
            PaneNode::Leaf { terminal, .. } => result.push(terminal),
            PaneNode::Split { first, second, .. } => {
                Self::collect_terminals(first, result);
                Self::collect_terminals(second, result);
            }
        }
    }

    fn count_leaves(node: &PaneNode) -> usize {
        match node {
            PaneNode::Leaf { .. } => 1,
            PaneNode::Split { first, second, .. } => {
                Self::count_leaves(first) + Self::count_leaves(second)
            }
        }
    }

    fn find_split_ratio(node: &PaneNode, target_id: SplitId) -> Option<(f32, SplitDirection)> {
        match node {
            PaneNode::Leaf { .. } => None,
            PaneNode::Split {
                split_id,
                ratio,
                direction,
                first,
                second,
            } => {
                if *split_id == target_id {
                    Some((*ratio, *direction))
                } else {
                    Self::find_split_ratio(first, target_id)
                        .or_else(|| Self::find_split_ratio(second, target_id))
                }
            }
        }
    }

    fn set_split_ratio(node: &mut PaneNode, target_id: SplitId, new_ratio: f32) -> bool {
        match node {
            PaneNode::Leaf { .. } => false,
            PaneNode::Split {
                split_id,
                ratio,
                first,
                second,
                ..
            } => {
                if *split_id == target_id {
                    if (*ratio - new_ratio).abs() > 0.001 {
                        *ratio = new_ratio;
                        true
                    } else {
                        false
                    }
                } else {
                    Self::set_split_ratio(first, target_id, new_ratio)
                        || Self::set_split_ratio(second, target_id, new_ratio)
                }
            }
        }
    }

    /// Returns `true` if the target was found and split.
    fn split_node(
        node: &mut PaneNode,
        target_id: PaneId,
        direction: SplitDirection,
        placement: SplitPlacement,
        new_id: PaneId,
        new_terminal: TerminalPane,
        new_split_id: SplitId,
    ) -> bool {
        match node {
            PaneNode::Leaf { id, .. } if *id == target_id => {
                let old_node = std::mem::replace(
                    node,
                    PaneNode::Leaf {
                        id: new_id,
                        terminal: new_terminal.clone(),
                    },
                );
                let new_leaf = PaneNode::Leaf {
                    id: new_id,
                    terminal: new_terminal,
                };
                let (first, second) = match placement {
                    SplitPlacement::Before => (Box::new(new_leaf), Box::new(old_node)),
                    SplitPlacement::After => (Box::new(old_node), Box::new(new_leaf)),
                };
                *node = PaneNode::Split {
                    split_id: new_split_id,
                    direction,
                    first,
                    second,
                    ratio: 0.5,
                };
                true
            }
            PaneNode::Split { first, second, .. } => {
                Self::split_node(
                    first,
                    target_id,
                    direction,
                    placement,
                    new_id,
                    new_terminal.clone(),
                    new_split_id,
                ) || Self::split_node(
                    second,
                    target_id,
                    direction,
                    placement,
                    new_id,
                    new_terminal,
                    new_split_id,
                )
            }
            _ => false,
        }
    }

    #[allow(dead_code)]
    fn remove_leaf(node: PaneNode, target_id: PaneId) -> PaneNode {
        match node {
            PaneNode::Split {
                split_id,
                direction,
                first,
                second,
                ratio,
            } => {
                if matches!(first.as_ref(), PaneNode::Leaf { id, .. } if *id == target_id) {
                    return *second;
                }
                if matches!(second.as_ref(), PaneNode::Leaf { id, .. } if *id == target_id) {
                    return *first;
                }
                PaneNode::Split {
                    split_id,
                    direction,
                    first: Box::new(Self::remove_leaf(*first, target_id)),
                    second: Box::new(Self::remove_leaf(*second, target_id)),
                    ratio,
                }
            }
            leaf => leaf,
        }
    }

    fn render_node(
        node: &PaneNode,
        focused_id: PaneId,
        has_splits: bool,
        begin_drag_cb: std::sync::Arc<dyn Fn(SplitId, f32) + 'static>,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();

        match node {
            PaneNode::Leaf { terminal, .. } => div()
                .size_full()
                .child(terminal.render_child())
                .into_any_element(),
            PaneNode::Split {
                split_id,
                direction,
                first,
                second,
                ratio,
            } => {
                let ratio = *ratio;
                let sid = *split_id;
                let dir = *direction;

                let cb_first = begin_drag_cb.clone();
                let cb_second = begin_drag_cb.clone();
                let cb_divider = begin_drag_cb.clone();

                let first_el = Self::render_node(first, focused_id, has_splits, cb_first, cx);
                let second_el = Self::render_node(second, focused_id, has_splits, cb_second, cx);

                let divider_id = ElementId::Name(format!("divider-{}", sid).into());
                let divider = match dir {
                    SplitDirection::Horizontal => div()
                        .id(divider_id)
                        .w(px(6.0))
                        .h_full()
                        .flex_shrink_0()
                        .cursor_col_resize()
                        .flex()
                        .justify_center()
                        .bg(theme.muted.opacity(0.035))
                        .child(
                            div()
                                .w(px(1.0))
                                .h_full()
                                .bg(theme.muted_foreground.opacity(0.12)),
                        )
                        .hover(|s| s.bg(theme.muted.opacity(0.07)))
                        .on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, _window, _cx| {
                                cb_divider(sid, f32::from(event.position.x));
                            },
                        ),
                    SplitDirection::Vertical => div()
                        .id(divider_id)
                        .h(px(6.0))
                        .w_full()
                        .flex_shrink_0()
                        .cursor_row_resize()
                        .flex()
                        .items_center()
                        .bg(theme.muted.opacity(0.035))
                        .child(
                            div()
                                .h(px(1.0))
                                .w_full()
                                .bg(theme.muted_foreground.opacity(0.12)),
                        )
                        .hover(|s| s.bg(theme.muted.opacity(0.07)))
                        .on_mouse_down(
                            MouseButton::Left,
                            move |event: &MouseDownEvent, _window, _cx| {
                                cb_divider(sid, f32::from(event.position.y));
                            },
                        ),
                };

                let make_pane = |child: AnyElement, basis: f32| -> Div {
                    let mut d = div()
                        .flex_grow()
                        .flex_shrink()
                        .flex_basis(relative(basis))
                        .overflow_hidden();
                    d = match dir {
                        SplitDirection::Horizontal => d.min_w_0().h_full(),
                        SplitDirection::Vertical => d.min_h_0().w_full(),
                    };
                    d.child(child)
                };

                let first_sized = make_pane(first_el, ratio);
                let second_sized = make_pane(second_el, 1.0 - ratio);

                let mut container = div().flex().size_full();
                container = match dir {
                    SplitDirection::Horizontal => container.flex_row(),
                    SplitDirection::Vertical => container.flex_col(),
                };

                container
                    .child(first_sized)
                    .child(divider)
                    .child(second_sized)
                    .into_any_element()
            }
        }
    }

    fn find_focused_pane(node: &PaneNode, window: &Window, cx: &App) -> Option<PaneId> {
        match node {
            PaneNode::Leaf { id, terminal } => {
                if terminal.is_focused(window, cx) {
                    Some(*id)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => Self::find_focused_pane(first, window, cx)
                .or_else(|| Self::find_focused_pane(second, window, cx)),
        }
    }

    fn check_terminal_pane_id(node: &PaneNode, entity_id: EntityId, pane_id: PaneId) -> bool {
        match node {
            PaneNode::Leaf { id, terminal } => *id == pane_id && terminal.entity_id() == entity_id,
            PaneNode::Split { first, second, .. } => {
                Self::check_terminal_pane_id(first, entity_id, pane_id)
                    || Self::check_terminal_pane_id(second, entity_id, pane_id)
            }
        }
    }

    fn find_pane_id_by_entity_id(node: &PaneNode, entity_id: EntityId) -> Option<PaneId> {
        match node {
            PaneNode::Leaf { id, terminal } => {
                if terminal.entity_id() == entity_id {
                    Some(*id)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                Self::find_pane_id_by_entity_id(first, entity_id)
                    .or_else(|| Self::find_pane_id_by_entity_id(second, entity_id))
            }
        }
    }

    fn collect_pane_terminals(node: &PaneNode, result: &mut Vec<(PaneId, TerminalPane)>) {
        match node {
            PaneNode::Leaf { id, terminal } => {
                result.push((*id, terminal.clone()));
            }
            PaneNode::Split { first, second, .. } => {
                Self::collect_pane_terminals(first, result);
                Self::collect_pane_terminals(second, result);
            }
        }
    }
}
