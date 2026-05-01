use con_core::session::{PaneLayoutState, PaneSplitDirection};
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

/// Unique identifier for a leaf pane.
///
/// Pane IDs remain the public, stable split-layout target used by the
/// built-in agent and the existing `panes.*` control API.
pub type PaneId = usize;

/// Unique identifier for a terminal surface hosted inside a pane.
///
/// A pane can hold multiple surfaces, but only its active surface participates
/// in pane-level agent context. This lets external orchestrators create
/// pane-local tabs without changing the benchmarked pane contract.
pub type SurfaceId = usize;

/// Unique identifier for a split node
pub type SplitId = usize;

#[derive(Clone)]
struct PaneSurface {
    id: SurfaceId,
    terminal: TerminalPane,
    title: Option<String>,
    owner: Option<String>,
    close_pane_when_last: bool,
}

impl PaneSurface {
    fn new(id: SurfaceId, terminal: TerminalPane) -> Self {
        Self {
            id,
            terminal,
            title: None,
            owner: None,
            close_pane_when_last: false,
        }
    }
}

#[derive(Clone)]
pub struct PaneSurfaceInfo {
    pub pane_id: PaneId,
    pub pane_index: usize,
    pub surface_id: SurfaceId,
    pub surface_index: usize,
    pub is_active: bool,
    pub is_focused_pane: bool,
    pub title: Option<String>,
    pub owner: Option<String>,
    pub close_pane_when_last: bool,
    pub terminal: TerminalPane,
}

#[derive(Debug, Clone)]
pub struct SurfaceCreateOptions {
    pub title: Option<String>,
    pub owner: Option<String>,
    pub close_pane_when_last: bool,
}

impl SurfaceCreateOptions {
    pub fn plain(title: Option<String>) -> Self {
        Self {
            title,
            owner: None,
            close_pane_when_last: false,
        }
    }
}

pub struct SurfaceCloseOutcome {
    pub pane_id: PaneId,
    pub terminal: TerminalPane,
    pub closed_pane: bool,
}

struct SurfaceCloseCandidate {
    pane_id: PaneId,
    terminal: TerminalPane,
    is_last_surface: bool,
    owner: Option<String>,
    close_pane_when_last: bool,
}

/// A node in the pane tree — either a pane slot or a split.
enum PaneNode {
    Leaf {
        id: PaneId,
        surfaces: Vec<PaneSurface>,
        active_surface_id: SurfaceId,
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
    zoomed_pane_id: Option<PaneId>,
    next_id: PaneId,
    next_surface_id: SurfaceId,
    next_split_id: SplitId,
    /// Active divider drag, if any
    pub dragging: Option<DragState>,
}

impl PaneTree {
    pub fn new(terminal: TerminalPane) -> Self {
        Self {
            root: PaneNode::Leaf {
                id: 0,
                surfaces: vec![PaneSurface::new(0, terminal)],
                active_surface_id: 0,
            },
            focused_pane_id: 0,
            zoomed_pane_id: None,
            next_id: 1,
            next_surface_id: 1,
            next_split_id: 0,
            dragging: None,
        }
    }

    pub fn from_state(
        layout: &PaneLayoutState,
        focused_pane_id: Option<PaneId>,
        make_terminal: &mut impl FnMut(Option<&str>) -> TerminalPane,
    ) -> Self {
        let mut restored_surface_id = 0;
        let mut root = Self::node_from_state(layout, make_terminal, &mut restored_surface_id);
        let mut next_split_id = 0;
        Self::normalize_split_ids(&mut root, &mut next_split_id);
        let next_id = Self::max_pane_id(&root).saturating_add(1);
        let next_surface_id = Self::max_surface_id(&root).saturating_add(1);
        let requested_focus = focused_pane_id.unwrap_or_else(|| Self::first_pane_id(&root));
        let focused_pane_id = if Self::find_terminal(&root, requested_focus).is_some() {
            requested_focus
        } else {
            Self::first_pane_id(&root)
        };

        Self {
            root,
            focused_pane_id,
            zoomed_pane_id: None,
            next_id,
            next_surface_id,
            next_split_id,
            dragging: None,
        }
    }

    pub fn to_state(&self, cx: &App) -> PaneLayoutState {
        Self::node_to_state(&self.root, cx)
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

    /// Get every terminal surface in the tree, including inactive pane-local tabs.
    pub fn all_surface_terminals(&self) -> Vec<&TerminalPane> {
        let mut result = Vec::new();
        Self::collect_all_surface_terminals(&self.root, &mut result);
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
        let new_surface_id = self.next_surface_id;
        self.next_surface_id += 1;
        let new_split_id = self.next_split_id;
        self.next_split_id += 1;
        Self::split_node(
            &mut self.root,
            self.focused_pane_id,
            direction,
            placement,
            new_id,
            new_surface_id,
            new_terminal,
            new_split_id,
            SurfaceCreateOptions::plain(None),
        );
        self.focused_pane_id = new_id;
        self.zoomed_pane_id = None;
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
        let new_surface_id = self.next_surface_id;
        self.next_surface_id += 1;
        let new_split_id = self.next_split_id;
        self.next_split_id += 1;
        if Self::split_node(
            &mut self.root,
            target_pane_id,
            direction,
            placement,
            new_id,
            new_surface_id,
            new_terminal,
            new_split_id,
            SurfaceCreateOptions::plain(None),
        ) {
            self.focused_pane_id = new_id;
            self.zoomed_pane_id = None;
        }
    }

    pub fn split_pane_with_surface_options(
        &mut self,
        target_pane_id: PaneId,
        direction: SplitDirection,
        placement: SplitPlacement,
        new_terminal: TerminalPane,
        options: SurfaceCreateOptions,
    ) -> Option<(PaneId, SurfaceId)> {
        let new_id = self.next_id;
        self.next_id += 1;
        let new_surface_id = self.next_surface_id;
        self.next_surface_id += 1;
        let new_split_id = self.next_split_id;
        self.next_split_id += 1;
        if Self::split_node(
            &mut self.root,
            target_pane_id,
            direction,
            placement,
            new_id,
            new_surface_id,
            new_terminal,
            new_split_id,
            options,
        ) {
            self.focused_pane_id = new_id;
            self.zoomed_pane_id = None;
            Some((new_id, new_surface_id))
        } else {
            None
        }
    }

    pub fn create_surface_in_pane(
        &mut self,
        pane_id: PaneId,
        terminal: TerminalPane,
        options: SurfaceCreateOptions,
    ) -> Option<SurfaceId> {
        let surface_id = self.next_surface_id;
        self.next_surface_id += 1;
        let mut surface = PaneSurface::new(surface_id, terminal);
        surface.title = options.title;
        surface.owner = options.owner;
        surface.close_pane_when_last = options.close_pane_when_last;

        if Self::push_surface(&mut self.root, pane_id, surface) {
            if self.zoomed_pane_id.is_some_and(|zoomed| zoomed != pane_id) {
                self.zoomed_pane_id = None;
            }
            self.focused_pane_id = pane_id;
            Some(surface_id)
        } else {
            None
        }
    }

    pub fn focus_surface(&mut self, surface_id: SurfaceId) -> bool {
        if let Some((pane_id, surface_changed)) = Self::activate_surface(&mut self.root, surface_id)
        {
            let focus_changed = self.focused_pane_id != pane_id;
            let zoom_changed = self.zoomed_pane_id.is_some_and(|zoomed| zoomed != pane_id);
            if zoom_changed {
                self.zoomed_pane_id = None;
            }
            self.focused_pane_id = pane_id;
            surface_changed || focus_changed || zoom_changed
        } else {
            false
        }
    }

    pub fn rename_surface(&mut self, surface_id: SurfaceId, title: Option<String>) -> bool {
        Self::rename_surface_node(&mut self.root, surface_id, title)
    }

    pub fn close_surface(
        &mut self,
        surface_id: SurfaceId,
        close_empty_owned_pane: bool,
    ) -> Option<SurfaceCloseOutcome> {
        let candidate = Self::surface_close_candidate(&self.root, surface_id)?;
        let outcome = if candidate.is_last_surface {
            let can_close_pane = close_empty_owned_pane
                && candidate.owner.is_some()
                && candidate.close_pane_when_last
                && self.pane_count() > 1;
            if !can_close_pane || !self.close_pane(candidate.pane_id) {
                return None;
            }
            SurfaceCloseOutcome {
                pane_id: candidate.pane_id,
                terminal: candidate.terminal,
                closed_pane: true,
            }
        } else {
            let removed = Self::remove_surface(&mut self.root, surface_id)?;
            SurfaceCloseOutcome {
                pane_id: removed.0,
                terminal: removed.1,
                closed_pane: false,
            }
        };
        if self.pane_count() <= 1
            || self
                .zoomed_pane_id
                .is_some_and(|zoomed_id| Self::find_terminal(&self.root, zoomed_id).is_none())
        {
            self.zoomed_pane_id = None;
        }
        if Self::find_terminal(&self.root, self.focused_pane_id).is_none() {
            self.focused_pane_id = Self::first_pane_id(&self.root);
        }
        Some(outcome)
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
                surfaces: vec![PaneSurface::new(0, placeholder_terminal)],
                active_surface_id: 0,
            },
        );
        self.root = Self::remove_leaf(old_root, pane_id);
        if Self::find_terminal(&self.root, self.focused_pane_id).is_none() {
            self.focused_pane_id = Self::first_pane_id(&self.root);
        }
        if self.pane_count() <= 1
            || self
                .zoomed_pane_id
                .is_some_and(|zoomed_id| Self::find_terminal(&self.root, zoomed_id).is_none())
        {
            self.zoomed_pane_id = None;
        }
        true
    }

    /// Set focus to a pane by ID without changing the current zoom target.
    pub fn focus(&mut self, pane_id: PaneId) {
        if Self::find_terminal(&self.root, pane_id).is_some() {
            self.focused_pane_id = pane_id;
        }
    }

    /// Terminal that should receive app-level focus when returning from UI.
    /// While zoomed, only the zoomed pane is visible, so focus that pane.
    pub fn visible_focus_terminal(&self) -> (PaneId, &TerminalPane) {
        if let Some(zoomed_id) = self.zoomed_pane_id {
            if let Some(terminal) = Self::find_terminal(&self.root, zoomed_id) {
                return (zoomed_id, terminal);
            }
        }

        (
            Self::first_pane_id(&self.root),
            Self::first_terminal(&self.root),
        )
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

    pub fn surface_terminals(&self) -> Vec<(PaneId, bool, TerminalPane)> {
        let mut result = Vec::new();
        Self::collect_surface_terminals(&self.root, &mut result);
        result
    }

    pub fn surface_infos(&self, target_pane_id: Option<PaneId>) -> Vec<PaneSurfaceInfo> {
        let mut result = Vec::new();
        let mut pane_index = 0;
        Self::collect_surface_infos(
            &self.root,
            target_pane_id,
            self.focused_pane_id,
            &mut pane_index,
            &mut result,
        );
        result
    }

    pub fn active_surface_id_for_pane(&self, pane_id: PaneId) -> Option<SurfaceId> {
        Self::find_active_surface_id(&self.root, pane_id)
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

    pub fn zoomed_pane_id(&self) -> Option<PaneId> {
        self.zoomed_pane_id
    }

    pub fn toggle_zoom_focused(&mut self) -> bool {
        if self.pane_count() <= 1 {
            self.zoomed_pane_id = None;
            return false;
        }

        if self.zoomed_pane_id.is_some() {
            self.zoomed_pane_id = None;
        } else {
            self.zoomed_pane_id = Some(self.focused_pane_id);
        }
        true
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
    pub fn render(
        &self,
        begin_drag_cb: impl Fn(SplitId, f32) + 'static,
        focus_surface_cb: impl Fn(SurfaceId, &mut Window, &mut App) + 'static,
        divider_color: Hsla,
        cx: &App,
    ) -> AnyElement {
        let focus_surface_cb = std::sync::Arc::new(focus_surface_cb);
        if let Some(zoomed_id) = self.zoomed_pane_id {
            if let Some(zoomed) = Self::render_zoomed_leaf(
                &self.root,
                zoomed_id,
                self.focused_pane_id,
                focus_surface_cb.clone(),
                divider_color,
                cx,
            ) {
                return zoomed;
            }
        }

        Self::render_node(
            &self.root,
            self.focused_pane_id,
            self.pane_count() > 1,
            std::sync::Arc::new(begin_drag_cb),
            focus_surface_cb,
            divider_color,
            cx,
        )
    }

    // --- Private helpers ---

    fn find_terminal(node: &PaneNode, target_id: PaneId) -> Option<&TerminalPane> {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } if *id == target_id => {
                Self::active_surface(surfaces, *active_surface_id).map(|surface| &surface.terminal)
            }
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { first, second, .. } => Self::find_terminal(first, target_id)
                .or_else(|| Self::find_terminal(second, target_id)),
        }
    }

    fn first_terminal(node: &PaneNode) -> &TerminalPane {
        match node {
            PaneNode::Leaf {
                surfaces,
                active_surface_id,
                ..
            } => {
                &Self::active_surface(surfaces, *active_surface_id)
                    .unwrap_or_else(|| surfaces.first().expect("pane leaf must have a surface"))
                    .terminal
            }
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
            PaneNode::Leaf {
                surfaces,
                active_surface_id,
                ..
            } => {
                if let Some(surface) = Self::active_surface(surfaces, *active_surface_id) {
                    result.push(&surface.terminal);
                }
            }
            PaneNode::Split { first, second, .. } => {
                Self::collect_terminals(first, result);
                Self::collect_terminals(second, result);
            }
        }
    }

    fn collect_all_surface_terminals<'a>(node: &'a PaneNode, result: &mut Vec<&'a TerminalPane>) {
        match node {
            PaneNode::Leaf { surfaces, .. } => {
                result.extend(surfaces.iter().map(|surface| &surface.terminal));
            }
            PaneNode::Split { first, second, .. } => {
                Self::collect_all_surface_terminals(first, result);
                Self::collect_all_surface_terminals(second, result);
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

    fn max_pane_id(node: &PaneNode) -> PaneId {
        match node {
            PaneNode::Leaf { id, .. } => *id,
            PaneNode::Split { first, second, .. } => {
                Self::max_pane_id(first).max(Self::max_pane_id(second))
            }
        }
    }

    fn max_surface_id(node: &PaneNode) -> SurfaceId {
        match node {
            PaneNode::Leaf { surfaces, .. } => {
                surfaces.iter().map(|surface| surface.id).max().unwrap_or(0)
            }
            PaneNode::Split { first, second, .. } => {
                Self::max_surface_id(first).max(Self::max_surface_id(second))
            }
        }
    }

    fn normalize_split_ids(node: &mut PaneNode, next_split_id: &mut SplitId) {
        match node {
            PaneNode::Leaf { .. } => {}
            PaneNode::Split {
                split_id,
                first,
                second,
                ..
            } => {
                *split_id = *next_split_id;
                *next_split_id += 1;
                Self::normalize_split_ids(first, next_split_id);
                Self::normalize_split_ids(second, next_split_id);
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
        new_surface_id: SurfaceId,
        new_terminal: TerminalPane,
        new_split_id: SplitId,
        options: SurfaceCreateOptions,
    ) -> bool {
        match node {
            PaneNode::Leaf { id, .. } if *id == target_id => {
                let mut surface = PaneSurface::new(new_surface_id, new_terminal.clone());
                surface.title = options.title;
                surface.owner = options.owner;
                surface.close_pane_when_last = options.close_pane_when_last;
                let old_node = std::mem::replace(
                    node,
                    PaneNode::Leaf {
                        id: new_id,
                        surfaces: vec![surface.clone()],
                        active_surface_id: new_surface_id,
                    },
                );
                let new_leaf = PaneNode::Leaf {
                    id: new_id,
                    surfaces: vec![surface],
                    active_surface_id: new_surface_id,
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
                    new_surface_id,
                    new_terminal.clone(),
                    new_split_id,
                    options.clone(),
                ) || Self::split_node(
                    second,
                    target_id,
                    direction,
                    placement,
                    new_id,
                    new_surface_id,
                    new_terminal,
                    new_split_id,
                    options,
                )
            }
            _ => false,
        }
    }

    fn push_surface(node: &mut PaneNode, pane_id: PaneId, surface: PaneSurface) -> bool {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } if *id == pane_id => {
                let surface_id = surface.id;
                surfaces.push(surface);
                *active_surface_id = surface_id;
                true
            }
            PaneNode::Leaf { .. } => false,
            PaneNode::Split { first, second, .. } => {
                Self::push_surface(first, pane_id, surface.clone())
                    || Self::push_surface(second, pane_id, surface)
            }
        }
    }

    fn activate_surface(node: &mut PaneNode, surface_id: SurfaceId) -> Option<(PaneId, bool)> {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => {
                if surfaces.iter().any(|surface| surface.id == surface_id) {
                    let changed = *active_surface_id != surface_id;
                    *active_surface_id = surface_id;
                    Some((*id, changed))
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => Self::activate_surface(first, surface_id)
                .or_else(|| Self::activate_surface(second, surface_id)),
        }
    }

    fn rename_surface_node(
        node: &mut PaneNode,
        surface_id: SurfaceId,
        title: Option<String>,
    ) -> bool {
        match node {
            PaneNode::Leaf { surfaces, .. } => {
                if let Some(surface) = surfaces.iter_mut().find(|surface| surface.id == surface_id)
                {
                    surface.title = title;
                    true
                } else {
                    false
                }
            }
            PaneNode::Split { first, second, .. } => {
                Self::rename_surface_node(first, surface_id, title.clone())
                    || Self::rename_surface_node(second, surface_id, title)
            }
        }
    }

    fn remove_surface(
        node: &mut PaneNode,
        surface_id: SurfaceId,
    ) -> Option<(PaneId, TerminalPane)> {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => {
                if surfaces.len() <= 1 {
                    return None;
                }
                let index = surfaces
                    .iter()
                    .position(|surface| surface.id == surface_id)?;
                let removed = surfaces.remove(index);
                if *active_surface_id == surface_id {
                    *active_surface_id = surfaces
                        .get(index.saturating_sub(1))
                        .or_else(|| surfaces.first())
                        .map(|surface| surface.id)
                        .expect("pane leaf must retain a surface");
                }
                Some((*id, removed.terminal))
            }
            PaneNode::Split { first, second, .. } => Self::remove_surface(first, surface_id)
                .or_else(|| Self::remove_surface(second, surface_id)),
        }
    }

    fn surface_close_candidate(
        node: &PaneNode,
        surface_id: SurfaceId,
    ) -> Option<SurfaceCloseCandidate> {
        match node {
            PaneNode::Leaf { id, surfaces, .. } => {
                let surface = surfaces.iter().find(|surface| surface.id == surface_id)?;
                Some(SurfaceCloseCandidate {
                    pane_id: *id,
                    terminal: surface.terminal.clone(),
                    is_last_surface: surfaces.len() <= 1,
                    owner: surface.owner.clone(),
                    close_pane_when_last: surface.close_pane_when_last,
                })
            }
            PaneNode::Split { first, second, .. } => {
                Self::surface_close_candidate(first, surface_id)
                    .or_else(|| Self::surface_close_candidate(second, surface_id))
            }
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
        focus_surface_cb: std::sync::Arc<dyn Fn(SurfaceId, &mut Window, &mut App) + 'static>,
        divider_color: Hsla,
        cx: &App,
    ) -> AnyElement {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => Self::render_leaf(
                *id,
                surfaces,
                *active_surface_id,
                focused_id,
                focus_surface_cb,
                cx,
            ),
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
                let focus_cb_first = focus_surface_cb.clone();
                let focus_cb_second = focus_surface_cb.clone();

                let first_el = Self::render_node(
                    first,
                    focused_id,
                    has_splits,
                    cb_first,
                    focus_cb_first,
                    divider_color,
                    cx,
                );
                let second_el = Self::render_node(
                    second,
                    focused_id,
                    has_splits,
                    cb_second,
                    focus_cb_second,
                    divider_color,
                    cx,
                );

                let divider_id = ElementId::Name(format!("divider-{}", sid).into());
                let divider = match dir {
                    SplitDirection::Horizontal => div()
                        .id(divider_id)
                        .relative()
                        .w(px(1.0))
                        .h_full()
                        .flex_shrink_0()
                        .bg(divider_color)
                        .child(
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(px(-2.0))
                                .w(px(5.0))
                                .cursor_col_resize()
                                .bg(gpui::transparent_black())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    move |event: &MouseDownEvent, _window, _cx| {
                                        cb_divider(sid, f32::from(event.position.x));
                                    },
                                ),
                        ),
                    SplitDirection::Vertical => div()
                        .id(divider_id)
                        .relative()
                        .h(px(1.0))
                        .w_full()
                        .flex_shrink_0()
                        .bg(divider_color)
                        .child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .top(px(-2.0))
                                .h(px(5.0))
                                .cursor_row_resize()
                                .bg(gpui::transparent_black())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    move |event: &MouseDownEvent, _window, _cx| {
                                        cb_divider(sid, f32::from(event.position.y));
                                    },
                                ),
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

    fn render_zoomed_leaf(
        node: &PaneNode,
        target_id: PaneId,
        focused_id: PaneId,
        focus_surface_cb: std::sync::Arc<dyn Fn(SurfaceId, &mut Window, &mut App) + 'static>,
        divider_color: Hsla,
        cx: &App,
    ) -> Option<AnyElement> {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } if *id == target_id => Some(Self::render_leaf(
                *id,
                surfaces,
                *active_surface_id,
                focused_id,
                focus_surface_cb,
                cx,
            )),
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { first, second, .. } => Self::render_zoomed_leaf(
                first,
                target_id,
                focused_id,
                focus_surface_cb.clone(),
                divider_color,
                cx,
            )
            .or_else(|| {
                Self::render_zoomed_leaf(
                    second,
                    target_id,
                    focused_id,
                    focus_surface_cb,
                    divider_color,
                    cx,
                )
            }),
        }
    }

    fn render_leaf(
        pane_id: PaneId,
        surfaces: &[PaneSurface],
        active_surface_id: SurfaceId,
        focused_id: PaneId,
        focus_surface_cb: std::sync::Arc<dyn Fn(SurfaceId, &mut Window, &mut App) + 'static>,
        cx: &App,
    ) -> AnyElement {
        let theme = cx.theme();
        let active = Self::active_surface(surfaces, active_surface_id)
            .unwrap_or_else(|| surfaces.first().expect("pane leaf must have a surface"));
        let terminal = active.terminal.render_child();
        if surfaces.len() <= 1 {
            return div().size_full().child(terminal).into_any_element();
        }

        let mut strip = div()
            .id(("surface-tab-strip", pane_id))
            .flex()
            .items_center()
            .gap(px(3.0))
            .h(px(24.0))
            .w_full()
            .px(px(6.0))
            .bg(theme
                .background
                .opacity(if pane_id == focused_id { 0.20 } else { 0.12 }))
            .overflow_x_scroll();

        for (index, surface) in surfaces.iter().enumerate() {
            let is_active = surface.id == active_surface_id;
            let title = surface
                .title
                .clone()
                .or_else(|| surface.terminal.title(cx))
                .unwrap_or_else(|| format!("Surface {}", index + 1));
            let color = if is_active {
                theme.foreground.opacity(0.86)
            } else {
                theme.foreground.opacity(0.46)
            };
            let bg = if is_active {
                theme.foreground.opacity(0.075)
            } else {
                theme.transparent
            };
            let sid = surface.id;
            let focus_cb = focus_surface_cb.clone();
            strip = strip.child(
                div()
                    .id(ElementId::Name(format!("surface-tab-{sid}").into()))
                    .flex()
                    .items_center()
                    .h(px(18.0))
                    .max_w(px(180.0))
                    .flex_shrink_0()
                    .px(px(6.0))
                    .bg(bg)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.foreground.opacity(0.10)))
                    .on_mouse_down(MouseButton::Left, move |_event, window, cx| {
                        focus_cb(sid, window, cx);
                    })
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .line_height(px(13.0))
                            .text_color(color)
                            .child(title),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(strip)
            .child(div().flex_1().min_h_0().overflow_hidden().child(terminal))
            .into_any_element()
    }

    fn find_focused_pane(node: &PaneNode, window: &Window, cx: &App) -> Option<PaneId> {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => {
                if let Some(surface) = Self::active_surface(surfaces, *active_surface_id)
                    && surface.terminal.is_focused(window, cx)
                {
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
            PaneNode::Leaf { id, surfaces, .. } => {
                *id == pane_id
                    && surfaces
                        .iter()
                        .any(|surface| surface.terminal.entity_id() == entity_id)
            }
            PaneNode::Split { first, second, .. } => {
                Self::check_terminal_pane_id(first, entity_id, pane_id)
                    || Self::check_terminal_pane_id(second, entity_id, pane_id)
            }
        }
    }

    fn find_pane_id_by_entity_id(node: &PaneNode, entity_id: EntityId) -> Option<PaneId> {
        match node {
            PaneNode::Leaf { id, surfaces, .. } => {
                if surfaces
                    .iter()
                    .any(|surface| surface.terminal.entity_id() == entity_id)
                {
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
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => {
                if let Some(surface) = Self::active_surface(surfaces, *active_surface_id) {
                    result.push((*id, surface.terminal.clone()));
                }
            }
            PaneNode::Split { first, second, .. } => {
                Self::collect_pane_terminals(first, result);
                Self::collect_pane_terminals(second, result);
            }
        }
    }

    fn collect_surface_terminals(node: &PaneNode, result: &mut Vec<(PaneId, bool, TerminalPane)>) {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => {
                result.extend(surfaces.iter().map(|surface| {
                    (
                        *id,
                        surface.id == *active_surface_id,
                        surface.terminal.clone(),
                    )
                }));
            }
            PaneNode::Split { first, second, .. } => {
                Self::collect_surface_terminals(first, result);
                Self::collect_surface_terminals(second, result);
            }
        }
    }

    fn collect_surface_infos(
        node: &PaneNode,
        target_pane_id: Option<PaneId>,
        focused_pane_id: PaneId,
        pane_index: &mut usize,
        result: &mut Vec<PaneSurfaceInfo>,
    ) {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => {
                *pane_index += 1;
                if target_pane_id.is_some_and(|target| target != *id) {
                    return;
                }
                result.extend(surfaces.iter().enumerate().map(|(surface_index, surface)| {
                    PaneSurfaceInfo {
                        pane_id: *id,
                        pane_index: *pane_index,
                        surface_id: surface.id,
                        surface_index: surface_index + 1,
                        is_active: surface.id == *active_surface_id,
                        is_focused_pane: *id == focused_pane_id,
                        title: surface.title.clone(),
                        owner: surface.owner.clone(),
                        close_pane_when_last: surface.close_pane_when_last,
                        terminal: surface.terminal.clone(),
                    }
                }));
            }
            PaneNode::Split { first, second, .. } => {
                Self::collect_surface_infos(
                    first,
                    target_pane_id,
                    focused_pane_id,
                    pane_index,
                    result,
                );
                Self::collect_surface_infos(
                    second,
                    target_pane_id,
                    focused_pane_id,
                    pane_index,
                    result,
                );
            }
        }
    }

    fn find_active_surface_id(node: &PaneNode, pane_id: PaneId) -> Option<SurfaceId> {
        match node {
            PaneNode::Leaf {
                id,
                active_surface_id,
                ..
            } if *id == pane_id => Some(*active_surface_id),
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { first, second, .. } => Self::find_active_surface_id(first, pane_id)
                .or_else(|| Self::find_active_surface_id(second, pane_id)),
        }
    }

    fn active_surface(
        surfaces: &[PaneSurface],
        active_surface_id: SurfaceId,
    ) -> Option<&PaneSurface> {
        surfaces
            .iter()
            .find(|surface| surface.id == active_surface_id)
            .or_else(|| surfaces.first())
    }

    fn node_to_state(node: &PaneNode, cx: &App) -> PaneLayoutState {
        match node {
            PaneNode::Leaf {
                id,
                surfaces,
                active_surface_id,
            } => PaneLayoutState::Leaf {
                pane_id: *id,
                cwd: Self::active_surface(surfaces, *active_surface_id)
                    .and_then(|surface| surface.terminal.current_dir(cx)),
            },
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => PaneLayoutState::Split {
                direction: match direction {
                    SplitDirection::Horizontal => PaneSplitDirection::Horizontal,
                    SplitDirection::Vertical => PaneSplitDirection::Vertical,
                },
                ratio: *ratio,
                first: Box::new(Self::node_to_state(first, cx)),
                second: Box::new(Self::node_to_state(second, cx)),
            },
        }
    }

    fn node_from_state(
        state: &PaneLayoutState,
        make_terminal: &mut impl FnMut(Option<&str>) -> TerminalPane,
        next_surface_id: &mut SurfaceId,
    ) -> PaneNode {
        match state {
            PaneLayoutState::Leaf { pane_id, cwd } => {
                let surface_id = *next_surface_id;
                *next_surface_id = (*next_surface_id).saturating_add(1);
                PaneNode::Leaf {
                    id: *pane_id,
                    surfaces: vec![PaneSurface::new(surface_id, make_terminal(cwd.as_deref()))],
                    active_surface_id: surface_id,
                }
            }
            PaneLayoutState::Split {
                direction,
                ratio,
                first,
                second,
            } => PaneNode::Split {
                split_id: 0,
                direction: match direction {
                    PaneSplitDirection::Horizontal => SplitDirection::Horizontal,
                    PaneSplitDirection::Vertical => SplitDirection::Vertical,
                },
                first: Box::new(Self::node_from_state(first, make_terminal, next_surface_id)),
                second: Box::new(Self::node_from_state(
                    second,
                    make_terminal,
                    next_surface_id,
                )),
                ratio: ratio.clamp(0.05, 0.95),
            },
        }
    }
}
