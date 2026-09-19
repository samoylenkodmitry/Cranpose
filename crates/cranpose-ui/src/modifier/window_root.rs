//! The window root modifier: a node whose subtree is the content of its own
//! window while staying inside the one composition.
//!
//! The node measures its content into the window's size and reports a zero
//! size to its parent, so the parent lays out as if the subtree were absent.
//! It registers itself with the app context's window root registry, which a
//! platform reads after each update to learn which windows exist. The scene
//! builder skips window roots when it builds a parent's scene and starts at
//! one when it builds that window's scene.

use std::{
    any::Any,
    cell::{Cell, RefCell},
    fmt,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{Applier, MemoryApplier, NodeId};
use cranpose_foundation::{
    Constraints, DelegatableNode, InvalidationKind, LayoutModifierNode, Measurable, ModifierNode,
    ModifierNodeContext, ModifierNodeElement, NodeCapabilities, NodeState, Size,
};
use cranpose_ui_layout::LayoutModifierMeasureResult;

use super::Modifier;
use crate::{
    render_state::{AppContextId, current_app_context, with_app_context_by_id},
    widgets::nodes::layout_node::LayoutNode,
};

/// What a window root needs from the platform's description of its window:
/// the logical size to lay the content out into, read on every measure so a
/// resize needs no new modifier, and the description itself for the platform
/// to take back.
pub trait WindowRootDescriptor: Any {
    /// The window's content size in logical pixels.
    fn layout_size(&self) -> Size;

    /// The descriptor as `Any`, so the platform that made it can downcast it.
    fn as_any(&self) -> &dyn Any;
}

/// A window root the registry knows about.
#[derive(Clone)]
pub struct WindowRootEntry {
    /// The layout node carrying the window root modifier.
    pub node: NodeId,
    /// The window's identity, stable across recompositions.
    pub id: u64,
    /// The platform's description of the window.
    pub descriptor: Rc<dyn WindowRootDescriptor>,
}

impl fmt::Debug for WindowRootEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRootEntry")
            .field("node", &self.node)
            .field("id", &self.id)
            .finish()
    }
}

/// The window roots attached in an app context, in attach order, with a
/// revision that changes whenever the set changes.
#[derive(Default)]
pub struct WindowRootRegistry {
    entries: RefCell<Vec<WindowRootEntry>>,
    revision: Cell<u64>,
}

impl WindowRootRegistry {
    fn register(&self, entry: WindowRootEntry) {
        let mut entries = self.entries.borrow_mut();
        if let Some(existing) = entries.iter_mut().find(|known| known.node == entry.node) {
            *existing = entry;
        } else {
            entries.push(entry);
        }
        self.bump();
    }

    fn unregister(&self, node: NodeId) {
        let mut entries = self.entries.borrow_mut();
        let before = entries.len();
        entries.retain(|entry| entry.node != node);
        if entries.len() != before {
            self.bump();
        }
    }

    fn bump(&self) {
        self.revision.set(self.revision.get().wrapping_add(1));
    }

    /// Every attached window root.
    pub fn entries(&self) -> Vec<WindowRootEntry> {
        self.entries.borrow().clone()
    }

    /// Changes whenever a window root attaches, detaches or is updated.
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }
}

/// The window roots attached in the current app context.
pub fn window_roots() -> Vec<WindowRootEntry> {
    current_app_context().map_or_else(Vec::new, |context| context.window_roots().entries())
}

/// The current app context's window root revision; see
/// [`WindowRootRegistry::revision`].
pub fn window_roots_revision() -> u64 {
    current_app_context().map_or(0, |context| context.window_roots().revision())
}

/// Whether `node` is a layout node whose modifier chain carries a window root.
pub fn is_window_root(applier: &mut MemoryApplier, node: NodeId) -> bool {
    applier
        .with_node::<LayoutNode, _>(node, |layout_node| layout_node.is_window_root())
        .unwrap_or(false)
}

/// The window root that owns `node`: the nearest node, `node` included, whose
/// modifier chain carries a window root. `None` when the node belongs to the
/// primary root.
pub fn nearest_window_root(applier: &mut MemoryApplier, node: NodeId) -> Option<NodeId> {
    let mut current = node;
    for _ in 0..100_000 {
        let is_root = applier
            .with_node::<LayoutNode, _>(current, |layout_node| layout_node.is_window_root())
            .unwrap_or(false);
        if is_root {
            return Some(current);
        }
        current = applier.get_mut(current).ok()?.parent()?;
    }
    None
}

/// Node that lays its content out into the window's size. That size is the
/// node's own, for the window's scene; the layout pass reports a zero size
/// to the node's parent.
pub struct WindowRootNode {
    id: u64,
    descriptor: Rc<dyn WindowRootDescriptor>,
    node_id: Cell<Option<NodeId>>,
    owner: Cell<Option<AppContextId>>,
    state: NodeState,
}

impl fmt::Debug for WindowRootNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRootNode")
            .field("id", &self.id)
            .field("node_id", &self.node_id.get())
            .finish()
    }
}

impl WindowRootNode {
    fn new(id: u64, descriptor: Rc<dyn WindowRootDescriptor>) -> Self {
        Self {
            id,
            descriptor,
            node_id: Cell::new(None),
            owner: Cell::new(None),
            state: NodeState::new(),
        }
    }

    fn entry(&self, node: NodeId) -> WindowRootEntry {
        WindowRootEntry {
            node,
            id: self.id,
            descriptor: Rc::clone(&self.descriptor),
        }
    }

    fn register(&self) {
        let Some(node) = self.node_id.get() else {
            return;
        };
        let Some(context) = current_app_context() else {
            log::debug!("window root {} attached outside an app context", self.id);
            return;
        };
        self.owner.set(Some(context.id()));
        context.window_roots().register(self.entry(node));
    }

    fn unregister(&self) {
        let (Some(node), Some(owner)) = (self.node_id.get(), self.owner.take()) else {
            return;
        };
        with_app_context_by_id(owner, |context| context.window_roots().unregister(node));
    }
}

impl DelegatableNode for WindowRootNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for WindowRootNode {
    fn on_attach(&mut self, context: &mut dyn ModifierNodeContext) {
        self.node_id.set(context.node_id());
        self.register();
        context.invalidate(InvalidationKind::Layout);
    }

    fn on_detach(&mut self) {
        self.unregister();
    }

    fn as_layout_node(&self) -> Option<&dyn LayoutModifierNode> {
        Some(self)
    }

    fn as_layout_node_mut(&mut self) -> Option<&mut dyn LayoutModifierNode> {
        Some(self)
    }
}

impl LayoutModifierNode for WindowRootNode {
    fn measure(
        &self,
        _context: &mut dyn ModifierNodeContext,
        measurable: &dyn Measurable,
        _constraints: Constraints,
    ) -> LayoutModifierMeasureResult {
        let size = self.descriptor.layout_size();
        let size = Size::new(size.width.max(0.0), size.height.max(0.0));
        let _content = measurable.measure(Constraints {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        });
        LayoutModifierMeasureResult::with_size(size)
    }

    fn min_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_width(&self, _measurable: &dyn Measurable, _height: f32) -> f32 {
        0.0
    }

    fn min_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        0.0
    }

    fn max_intrinsic_height(&self, _measurable: &dyn Measurable, _width: f32) -> f32 {
        0.0
    }
}

/// Element that creates and updates window root nodes.
#[derive(Clone)]
pub struct WindowRootElement {
    id: u64,
    descriptor: Rc<dyn WindowRootDescriptor>,
}

impl WindowRootElement {
    /// A window root with identity `id`, described by `descriptor`.
    pub fn new(id: u64, descriptor: Rc<dyn WindowRootDescriptor>) -> Self {
        Self { id, descriptor }
    }
}

impl fmt::Debug for WindowRootElement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRootElement")
            .field("id", &self.id)
            .finish()
    }
}

impl PartialEq for WindowRootElement {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && Rc::ptr_eq(&self.descriptor, &other.descriptor)
    }
}

impl Hash for WindowRootElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl ModifierNodeElement for WindowRootElement {
    type Node = WindowRootNode;

    fn create(&self) -> Self::Node {
        WindowRootNode::new(self.id, Rc::clone(&self.descriptor))
    }

    fn update(&self, node: &mut Self::Node) {
        node.id = self.id;
        node.descriptor = Rc::clone(&self.descriptor);
        node.register();
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT | NodeCapabilities::WINDOW_ROOT
    }

    fn inspector_name(&self) -> &'static str {
        "windowRoot"
    }
}

impl Modifier {
    /// Makes the node the root of its own window, identified by `id` and
    /// described by `descriptor`. The subtree is laid out into the
    /// descriptor's size and drawn into that window's scene; the parent sees
    /// a node of zero size and its scene skips the subtree. Platforms wrap
    /// this in a modifier that takes their own window configuration.
    pub fn window_root(self, id: u64, descriptor: Rc<dyn WindowRootDescriptor>) -> Self {
        self.then(Self::with_element(WindowRootElement::new(id, descriptor)))
    }
}
