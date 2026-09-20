//! Developer inspection drawn outside the application's composition and semantics.

use std::fmt::Debug;

use cranpose_core::NodeId;
use cranpose_render_common::Renderer;
use cranpose_ui::{KeyCode, KeyEvent, KeyEventType, LayoutTree, SemanticsTree};
use cranpose_ui_graphics::{Rect, Size};

use crate::{AppShell, RootSurface, ShellApp, SurfaceMut};

#[path = "inspector_draw.rs"]
mod draw;

/// The application's visual presentation while inspecting accessibility.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorMode {
    /// Draw the application normally.
    #[default]
    Normal,
    /// Draw accessibility bounds and reading-order numbers over the application.
    Overlay,
    /// Cover the application with its accessible controls and labels.
    Accessibility,
}

/// An inspector control, independent of the application's actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorAction {
    /// Open or close the inspector.
    Toggle,
    /// Show the normal application.
    Normal,
    /// Show accessibility outlines.
    Overlay,
    /// Show only the accessibility representation.
    Accessibility,
    /// Select a control by its screen bounds without activating it.
    Pick,
    /// Select the previous accessible element.
    Previous,
    /// Select the next accessible element.
    Next,
    /// Scroll the selected element's properties toward the beginning.
    DetailsUp,
    /// Scroll the selected element's properties toward the end.
    DetailsDown,
    /// Select an element at its reading-order index.
    Select(usize),
}

/// A sanitized platform-projection element for developer inspection.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorNode {
    /// The application node owning this accessible element.
    pub node_id: NodeId,
    /// The identity of a virtual canvas child, when present.
    pub canvas_key: Option<u64>,
    /// Accessible bounds in surface-local logical pixels.
    pub bounds: Rect,
    /// Accessible name and role for the reading-order list.
    pub label: String,
    /// Accessible properties and actions, with password values excluded.
    pub details: String,
    /// Whether the application reports this element as focused.
    pub focused: bool,
    /// An actionable naming issue to mark in the overlay.
    pub issue: bool,
}

/// A visible inspector control and its logical hit bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectorControl {
    /// The operation performed by this control.
    pub action: InspectorAction,
    /// Bounds shared by rendering and pointer dispatch.
    pub bounds: Rect,
}

/// A read-only snapshot of developer UI, separate from application semantics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InspectorState {
    /// Whether the inspector panel is open.
    pub open: bool,
    /// The selected visual mode.
    pub mode: InspectorMode,
    /// Whether the next application press selects an element.
    pub picking: bool,
    /// Elements in the shared projection's reading order.
    pub nodes: Vec<InspectorNode>,
    /// The selected element's index in `nodes`.
    pub selected: Option<usize>,
    /// The first visible line of the selected element's properties.
    pub detail_offset: usize,
    /// Visible developer controls for deterministic robot input.
    pub controls: Vec<InspectorControl>,
}

/// Projects a surface's trees using the same policy as its platform bridge.
pub type InspectorProjector = fn(&LayoutTree, &SemanticsTree) -> Vec<InspectorNode>;

#[derive(Default)]
pub(crate) struct DeveloperInspector {
    pub(crate) state: InspectorState,
    revision: Option<u64>,
    viewport: Option<Size>,
    dirty: bool,
    pub(crate) pointer_captured: bool,
    keyboard: bool,
    installed: bool,
}

impl DeveloperInspector {
    fn apply(&mut self, action: InspectorAction) {
        match action {
            InspectorAction::Toggle => {
                self.state.open = !self.state.open;
                self.state.picking = false;
                self.keyboard = self.state.open;
                if !self.state.open {
                    self.state.mode = InspectorMode::Normal;
                    self.state.nodes.clear();
                    self.state.selected = None;
                    self.revision = None;
                }
            }
            InspectorAction::Normal => self.state.mode = InspectorMode::Normal,
            InspectorAction::Overlay => self.state.mode = InspectorMode::Overlay,
            InspectorAction::Accessibility => self.state.mode = InspectorMode::Accessibility,
            InspectorAction::Pick => self.state.picking = !self.state.picking,
            InspectorAction::Previous => self.select_relative(false),
            InspectorAction::Next => self.select_relative(true),
            InspectorAction::DetailsUp => {
                self.state.detail_offset = self.state.detail_offset.saturating_sub(3)
            }
            InspectorAction::DetailsDown => {
                let count = draw::detail_line_count(&self.state, self.viewport.unwrap_or_default());
                self.state.detail_offset =
                    (self.state.detail_offset + 3).min(count.saturating_sub(1));
            }
            InspectorAction::Select(index) => {
                self.state.selected = (index < self.state.nodes.len()).then_some(index);
                self.state.detail_offset = 0;
            }
        }
        self.dirty = true;
    }

    fn select_relative(&mut self, forward: bool) {
        self.state.detail_offset = 0;
        let len = self.state.nodes.len();
        if len == 0 {
            self.state.selected = None;
            return;
        }
        self.state.selected = Some(match self.state.selected {
            Some(index) if forward => (index + 1) % len,
            Some(index) => (index + len - 1) % len,
            None if forward => 0,
            None => len - 1,
        });
    }

    fn replace_nodes(&mut self, nodes: Vec<InspectorNode>) {
        if self.state.nodes == nodes {
            return;
        }
        let identity = self
            .state
            .selected
            .and_then(|index| self.state.nodes.get(index))
            .map(|node| (node.node_id, node.canvas_key));
        self.state.selected = identity.and_then(|identity| {
            nodes
                .iter()
                .position(|node| (node.node_id, node.canvas_key) == identity)
        });
        self.state.detail_offset = 0;
        self.state.nodes = nodes;
        self.dirty = true;
    }

    fn pick(&mut self, x: f32, y: f32) {
        self.state.selected = self
            .state
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.bounds.contains(x, y))
            .min_by(|(_, a), (_, b)| {
                (a.bounds.width * a.bounds.height).total_cmp(&(b.bounds.width * b.bounds.height))
            })
            .map(|(index, _)| index);
        self.state.picking = false;
        self.state.detail_offset = 0;
        self.keyboard = true;
        self.dirty = true;
    }
}

impl<R: Renderer> AppShell<R>
where
    R::Error: Debug,
{
    /// Installs or disables the developer inspector without adding application nodes.
    ///
    /// Hosts install the platform projection in debug builds. `None` removes all
    /// inspector drawing and input handling, including on secondary surfaces.
    pub fn set_inspector_projector(&mut self, projector: Option<InspectorProjector>) {
        self.app.inspector_projector = projector;
        for surface in &mut self.surfaces {
            surface.inspector = DeveloperInspector {
                dirty: true,
                ..Default::default()
            };
            if projector.is_none() {
                surface.renderer.set_inspector_overlay(None);
            }
            surface.is_dirty = true;
        }
    }

    /// The primary surface's inspector state, independent of application semantics.
    pub fn inspector_state(&self) -> &InspectorState {
        &self.surfaces[0].inspector.state
    }
}

impl<R: Renderer> SurfaceMut<'_, R>
where
    R::Error: Debug,
{
    /// This surface's developer UI and projected application elements.
    pub fn inspector_state(&self) -> &InspectorState {
        &self.surface().inspector.state
    }

    pub(crate) fn inspector_owns_keyboard(&self) -> bool {
        let inspector = &self.surface().inspector;
        inspector.state.open && inspector.keyboard
    }

    pub(crate) fn inspector_blocks_pointer(&self, x: f32, y: f32) -> bool {
        let inspector = &self.surface().inspector;
        self.shell_app_ref().inspector_projector.is_some()
            && (inspector.pointer_captured
                || inspector.state.picking
                || inspector
                    .state
                    .controls
                    .iter()
                    .any(|control| control.bounds.contains(x, y))
                || (inspector.state.open
                    && draw::panel_bounds(inspector.viewport.unwrap_or_default()).contains(x, y)))
    }

    pub(crate) fn inspector_scroll(&mut self, delta: f32) -> bool {
        let (x, y) = self.surface().cursor;
        if !self.inspector_blocks_pointer(x, y) {
            return false;
        }
        let inspector = &mut self.surface_mut().inspector;
        if inspector.state.open && delta != 0.0 {
            inspector.apply(if delta < 0.0 {
                InspectorAction::DetailsDown
            } else {
                InspectorAction::DetailsUp
            });
            self.mark_dirty();
        }
        true
    }

    pub(crate) fn inspector_press(&mut self, x: f32, y: f32) -> bool {
        if self.shell_app_ref().inspector_projector.is_none() {
            return false;
        }
        let inspector = &mut self.surface_mut().inspector;
        let action = inspector
            .state
            .controls
            .iter()
            .find(|control| control.bounds.contains(x, y))
            .map(|control| control.action);
        let consumed = if let Some(action) = action {
            inspector.apply(action);
            inspector.keyboard = inspector.state.open;
            true
        } else if inspector.state.open && inspector.state.picking {
            inspector.pick(x, y);
            true
        } else {
            inspector.keyboard = false;
            inspector.state.open
                && draw::panel_bounds(inspector.viewport.unwrap_or_default()).contains(x, y)
        };
        if consumed {
            inspector.keyboard = inspector.state.open;
            inspector.pointer_captured = true;
            self.mark_dirty();
        }
        consumed
    }

    pub(crate) fn inspector_key(&mut self, event: &KeyEvent) -> bool {
        if self.shell_app_ref().inspector_projector.is_none() {
            return false;
        }
        let inspector = &mut self.surface_mut().inspector;
        let toggle = event.key_code == KeyCode::I
            && (event.modifiers.ctrl || event.modifiers.meta)
            && event.modifiers.shift;
        let action = if toggle {
            Some(InspectorAction::Toggle)
        } else if inspector.state.open && inspector.keyboard {
            match event.key_code {
                KeyCode::Escape => Some(InspectorAction::Toggle),
                KeyCode::ArrowUp | KeyCode::ArrowLeft => Some(InspectorAction::Previous),
                KeyCode::ArrowDown | KeyCode::ArrowRight => Some(InspectorAction::Next),
                KeyCode::Digit1 => Some(InspectorAction::Normal),
                KeyCode::Digit2 => Some(InspectorAction::Overlay),
                KeyCode::Digit3 => Some(InspectorAction::Accessibility),
                KeyCode::P => Some(InspectorAction::Pick),
                KeyCode::PageUp => Some(InspectorAction::DetailsUp),
                KeyCode::PageDown => Some(InspectorAction::DetailsDown),
                _ => None,
            }
        } else {
            None
        };
        let Some(action) = action else {
            return inspector.state.open && inspector.keyboard;
        };
        if event.event_type == KeyEventType::KeyDown {
            inspector.apply(action);
            self.mark_dirty();
        }
        true
    }
}

pub(crate) fn refresh<R: Renderer>(
    app: &mut ShellApp,
    surface: &mut RootSurface<R>,
    revision: u64,
) -> bool {
    let Some(projector) = app.inspector_projector else {
        return false;
    };
    let viewport = surface.viewport_size();
    if surface.inspector.viewport != Some(viewport) {
        surface.inspector.viewport = Some(viewport);
        surface.inspector.dirty = true;
    }
    if surface.inspector.state.open && surface.inspector.revision != Some(revision) {
        surface.layout_tree_in_context(app);
        surface.semantics_tree_in_context(app);
        let nodes = match (&surface.layout_tree, &surface.semantics_tree) {
            (Some(layout), Some(semantics)) => projector(layout, semantics),
            _ => Vec::new(),
        };
        surface.inspector.replace_nodes(nodes);
        surface.inspector.revision = Some(revision);
    }
    if !surface.inspector.dirty && surface.inspector.installed {
        return false;
    }
    let graph = draw::build(&mut surface.inspector.state, viewport);
    surface.renderer.set_inspector_overlay(Some(graph));
    surface.inspector.installed = true;
    surface.inspector.dirty = false;
    true
}

#[cfg(test)]
#[path = "tests/inspector_tests.rs"]
mod tests;
