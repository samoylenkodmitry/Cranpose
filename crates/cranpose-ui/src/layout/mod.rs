mod bottom_up;
mod coordinator_chain;
/// Re-exports of the alignment, arrangement and measurable types that layout policies use.
pub mod core;
pub(crate) mod flex_box;
mod frame_layout_arena;
pub(crate) mod grid;
mod layout_child_measurable;
mod layout_child_measure_state;
mod layout_node_data;
mod live_layout_node;
mod measure_telemetry;
mod measured_spare;
mod modifier_chain_measure;
mod placed_root_measurements;
/// Measure policies behind the built-in layout widgets such as `Box`, `Row` and `Column`.
pub mod policies;
mod reveal;
mod semantics_details;
mod semantics_labels;
mod semantics_tree;
mod semantics_update;
mod subcompose_measure;

use std::{cell::RefCell, mem::size_of, rc::Rc};

use cranpose_core::{
    Applier, ConcreteApplierHost, MemoryApplier, Node, NodeError, NodeId, RuntimeHandle,
};
use cranpose_foundation::{
    InvalidationKind, ModifierNodeContext, SemanticsConfiguration, SemanticsWidgetRole,
};
use cranpose_ui_graphics::ProjectiveTransform;
use cranpose_ui_layout::{AlignmentLines, Constraints, MeasurePolicy, PlaceTarget};
pub(crate) use frame_layout_arena::FrameLayoutArena;
use frame_layout_arena::VecPools;
use layout_child_measure_state::{
    ChildPass, LayoutChildMeasureState, LayoutChildren, inherited_alignment_lines, read_cell,
};
use layout_node_data::{role_from_modifier_slices, snapshot_node_data};
use live_layout_node::{LiveLayoutNode, read_live_layout_node};
use measured_spare::MeasuredSpare;
use placed_root_measurements::PlacedRootMeasurements;
pub use semantics_tree::{
    SemanticsAction, SemanticsCallback, SemanticsNode, SemanticsPlacement, SemanticsRole,
    SemanticsText, SemanticsTree,
};
use semantics_tree::{modal_takes_space, top_modal_path};
use web_time::Instant;

#[cfg(test)]
use self::core::{HorizontalAlignment, VerticalAlignment};
use self::{
    coordinator_chain::{CoordinatorChain, CoordinatorFrame},
    core::Measurable,
    measure_telemetry::{
        LayoutStages, layout_measure_telemetry_threshold_ms, log_layout_measure_telemetry,
        log_node_measure_telemetry,
    },
    semantics_update::semantics_placement,
};
pub use self::{
    layout_node_data::{LayoutNodeData, LayoutNodeKind},
    reveal::can_skip_scroll_reveal_from_applier,
    semantics_details::SemanticsDetails,
    semantics_update::{build_semantics_tree_from_applier, update_semantics_tree_from_applier},
};
use crate::{
    modifier::{
        DimensionConstraint, EdgeInsets, ModifierNodeSlicesDebugStats, Point, Rect as GeometryRect,
        Size,
    },
    render_state::AppContext,
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{ChildPlacement, IntrinsicKind, LayoutNode, LayoutParts, LayoutState},
};

#[derive(Default)]
pub(crate) struct LayoutNodeContext {
    invalidations: Vec<InvalidationKind>,
    update_requested: bool,
    density: f32,
}

impl LayoutNodeContext {
    pub(crate) fn new(density: f32) -> Self {
        Self {
            density,
            ..Self::default()
        }
    }

    pub(crate) fn take_invalidations(&mut self) -> Vec<InvalidationKind> {
        std::mem::take(&mut self.invalidations)
    }
}

impl ModifierNodeContext for LayoutNodeContext {
    fn invalidate(&mut self, kind: InvalidationKind) {
        if !self.invalidations.contains(&kind) {
            self.invalidations.push(kind);
        }
    }

    fn request_update(&mut self) {
        self.update_requested = true;
    }

    fn density(&self) -> f32 {
        self.density
    }
}

#[doc(hidden)]
pub fn invalidate_all_layout_caches() {
    crate::render_state::invalidate_layout_cache_epoch();
}

/// The host a layout pass lends its applier to. Between passes it holds an
/// empty applier and the app context keeps it, so a pass builds neither.
pub(crate) struct LayoutHost {
    host: Rc<ConcreteApplierHost<MemoryApplier>>,
}

impl Default for LayoutHost {
    fn default() -> Self {
        Self {
            host: Rc::new(ConcreteApplierHost::new(MemoryApplier::new())),
        }
    }
}

/// Lends `target` to a [`LayoutHost`] of `context` for one pass and gives it
/// back when dropped, also after a panic.
struct ApplierSlotGuard<'a> {
    target: &'a mut MemoryApplier,
    lent: LayoutHost,
    context: &'a AppContext,
}

impl<'a> ApplierSlotGuard<'a> {
    fn new(target: &'a mut MemoryApplier, context: &'a AppContext) -> Self {
        let lent = context.take_layout_host();
        std::mem::swap(target, &mut *lent.host.borrow_typed());
        Self {
            target,
            lent,
            context,
        }
    }

    fn host(&self) -> &Rc<ConcreteApplierHost<MemoryApplier>> {
        &self.lent.host
    }
}

impl Drop for ApplierSlotGuard<'_> {
    fn drop(&mut self) {
        let host = &self.lent.host;
        std::mem::swap(self.target, &mut *host.borrow_typed());
        // A host still shared past the pass stays with whoever holds it.
        if Rc::strong_count(host) == 1 {
            self.context.replace_layout_host(LayoutHost {
                host: Rc::clone(host),
            });
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct ModifierChainInputs {
    density: crate::density::Density,
    window_root: bool,
    offset: Point,
    uses_chain: bool,
}

struct ModifierChainMeasurement {
    size: Size,
    alignment_lines: AlignmentLines,
    content_offset: Point,
    offset: Point,
    window_root: bool,
    /// The incoming constraints this measurement holds for besides its own.
    hold: Option<cranpose_ui_layout::ConstraintsHold>,
    /// Whether no child's size entered it: see
    /// [`MeasurePolicy::ignores_child_sizes`].
    ignores_child_sizes: bool,
}

/// Counts and heap sizes of the layout and semantics data that one measure pass retains, for memory
/// diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutAllocationDebugStats {
    /// Number of `LayoutBox` nodes in the tree.
    pub layout_box_count: usize,
    /// Total number of child entries across all layout boxes.
    pub layout_box_child_count: usize,
    /// Total allocated capacity of the child vectors across all layout boxes.
    pub layout_box_child_capacity: usize,
    /// Heap bytes allocated for the child vectors of all layout boxes.
    pub layout_box_heap_bytes: usize,
    /// Number of modifier slice snapshots, one per layout box.
    pub modifier_slice_count: usize,
    /// Heap bytes held by all modifier slice snapshots.
    pub modifier_slice_heap_bytes: usize,
    /// Number of draw commands collected from modifier nodes.
    pub modifier_draw_command_count: usize,
    /// Allocated capacity of the draw command lists.
    pub modifier_draw_command_capacity: usize,
    /// Number of pointer input handlers collected from modifier nodes.
    pub modifier_pointer_input_count: usize,
    /// Allocated capacity of the pointer input handler lists.
    pub modifier_pointer_input_capacity: usize,
    /// Number of layout boxes that carry text content.
    pub modifier_text_content_count: usize,
    /// Number of layout boxes that carry a text style.
    pub modifier_text_style_count: usize,
    /// Number of layout boxes that carry text layout options.
    pub modifier_text_layout_options_count: usize,
    /// Number of layout boxes that carry a prepared text layout.
    pub modifier_prepared_text_layout_count: usize,
    /// Number of layout boxes that carry a graphics layer.
    pub modifier_graphics_layer_count: usize,
    /// Number of layout boxes that carry a graphics layer resolver.
    pub modifier_graphics_layer_resolver_count: usize,
    /// Number of nodes in the semantics tree.
    pub semantics_node_count: usize,
    /// Total number of actions across all semantics nodes.
    pub semantics_action_count: usize,
    /// Total allocated capacity of the action vectors across all semantics nodes.
    pub semantics_action_capacity: usize,
    /// Total number of child entries across all semantics nodes.
    pub semantics_child_count: usize,
    /// Total allocated capacity of the child vectors across all semantics nodes.
    pub semantics_child_capacity: usize,
    /// Number of semantics nodes that have a content description.
    pub semantics_description_count: usize,
    /// Allocated bytes of all content description strings.
    pub semantics_description_bytes: usize,
    /// Heap bytes held by the semantics tree: actions, children, details and descriptions.
    pub semantics_heap_bytes: usize,
}

impl LayoutAllocationDebugStats {
    fn add_modifier_slice(&mut self, stats: ModifierNodeSlicesDebugStats) {
        self.modifier_slice_count += 1;
        self.modifier_slice_heap_bytes += stats.heap_bytes;
        self.modifier_draw_command_count += stats.draw_command_count;
        self.modifier_draw_command_capacity += stats.draw_command_capacity;
        self.modifier_pointer_input_count += stats.pointer_input_count;
        self.modifier_pointer_input_capacity += stats.pointer_input_capacity;
        self.modifier_text_content_count += usize::from(stats.has_text_content);
        self.modifier_text_style_count += usize::from(stats.has_text_style);
        self.modifier_text_layout_options_count += usize::from(stats.has_text_layout_options);
        self.modifier_prepared_text_layout_count += usize::from(stats.has_prepared_text_layout);
        self.modifier_graphics_layer_count += usize::from(stats.has_graphics_layer);
        self.modifier_graphics_layer_resolver_count +=
            usize::from(stats.has_graphics_layer_resolver);
    }
}

/// Result of running layout for a Compose tree.
#[derive(Debug, Clone)]
pub struct LayoutTree {
    root: LayoutBox,
}

impl LayoutTree {
    /// Wraps `root` as a layout tree.
    pub fn new(root: LayoutBox) -> Self {
        Self { root }
    }

    /// Returns the root layout box.
    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    /// Returns the root layout box for in-place changes.
    pub fn root_mut(&mut self) -> &mut LayoutBox {
        &mut self.root
    }

    /// Consumes the tree and returns its root layout box.
    pub fn into_root(self) -> LayoutBox {
        self.root
    }

    /// Counts the allocations that the layout boxes of this tree retain.
    pub fn debug_allocation_stats(&self) -> LayoutAllocationDebugStats {
        let mut stats = LayoutAllocationDebugStats::default();
        record_layout_box_allocation_stats(&self.root, &mut stats);
        stats
    }
}

/// Layout information for a single node.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    /// Id of the runtime node this box describes.
    pub node_id: NodeId,
    /// Incarnation of the runtime node captured with these layout bounds.
    pub node_generation: u32,
    /// Position and size of the node in root coordinates, in logical pixels, before graphics-layer
    /// transforms.
    pub rect: GeometryRect,
    /// Content offset for scroll/inner transforms (applies to children, NOT this node's position)
    pub content_offset: Point,
    /// Modifier, semantics and kind data captured with the node.
    pub node_data: LayoutNodeData,
    /// Layout boxes of the node's placed children.
    pub children: Vec<Self>,
}

impl LayoutBox {
    /// Creates a layout box with node generation zero.
    pub fn new(
        node_id: NodeId,
        rect: GeometryRect,
        content_offset: Point,
        node_data: LayoutNodeData,
        children: Vec<Self>,
    ) -> Self {
        Self {
            node_id,
            node_generation: 0,
            rect,
            content_offset,
            node_data,
            children,
        }
    }
}

/// Extension trait that equips `MemoryApplier` with layout computation.
pub trait LayoutEngine {
    /// Measures the tree under `root` within `max_size` and returns its layout tree.
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError>;
}

impl LayoutEngine for MemoryApplier {
    fn compute_layout(&mut self, root: NodeId, max_size: Size) -> Result<LayoutTree, NodeError> {
        let measurements = measure_layout(self, root, max_size)?;
        measurements
            .into_layout_tree()
            .ok_or(NodeError::MissingContext {
                id: root,
                reason: "layout tree was not requested",
            })
    }
}

/// Result of running the measure pass for a Compose layout tree.
#[derive(Debug, Clone)]
pub struct LayoutMeasurements {
    root: Rc<MeasuredNode>,
    // Boxed: a frame that builds neither tree moves a few words, not the
    // hundreds of bytes the trees take inline.
    semantics: Option<Box<SemanticsTree>>,
    layout_tree: Option<Box<LayoutTree>>,
}

impl LayoutMeasurements {
    fn new(
        root: Rc<MeasuredNode>,
        semantics: Option<SemanticsTree>,
        layout_tree: Option<LayoutTree>,
    ) -> Self {
        Self {
            root,
            semantics: semantics.map(Box::new),
            layout_tree: layout_tree.map(Box::new),
        }
    }

    /// Returns the measured size of the root node.
    pub fn root_size(&self) -> Size {
        self.root.size
    }

    /// Returns the semantics tree, or `None` if the measure options left it out.
    pub fn semantics_tree(&self) -> Option<&SemanticsTree> {
        self.semantics.as_deref()
    }

    /// Counts the allocations that the layout tree and the semantics tree retain.
    pub fn debug_allocation_stats(&self) -> LayoutAllocationDebugStats {
        let mut stats = self
            .layout_tree
            .as_deref()
            .map(LayoutTree::debug_allocation_stats)
            .unwrap_or_default();
        if let Some(semantics) = &self.semantics {
            record_semantics_allocation_stats(semantics.root(), &mut stats);
        }
        stats
    }

    /// Consumes the measurements and returns the built [`LayoutTree`], if requested.
    pub fn into_layout_tree(self) -> Option<LayoutTree> {
        self.layout_tree.map(|tree| *tree)
    }

    /// Returns a cloned [`LayoutTree`] for rendering/debug consumers, if requested.
    pub fn layout_tree(&self) -> Option<LayoutTree> {
        self.layout_tree.as_deref().cloned()
    }
}

/// Builds a semantics tree from an existing [`LayoutTree`].
///
/// This is useful for consumers that need semantics on demand without forcing
/// every layout pass to eagerly allocate a full [`SemanticsTree`].
pub fn build_semantics_tree_from_layout_tree(layout_tree: &LayoutTree) -> SemanticsTree {
    SemanticsTree::new(build_semantics_node_from_layout_box(
        layout_tree.root(),
        Point::default(),
    ))
}

/// Builds a layout snapshot from retained layout state in the live applier tree.
///
/// Renderers use retained node state directly. This function exists for debug,
/// robot, and tests that need an owned [`LayoutTree`] without forcing every
/// layout pass to allocate one.
pub fn build_layout_tree_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<Option<LayoutTree>, NodeError> {
    let origin = layout_tree_origin(read_layout_node(applier, root, |state, _| state)?);
    let mut child_stack = Vec::new();
    place_layout_box(
        applier,
        root,
        origin,
        ProjectiveTransform::identity(),
        &mut child_stack,
    )
    .map(|root| root.map(LayoutTree::new))
}

/// Whether any box placed under `root` has area, as
/// [`build_layout_tree_from_applier`] would place it: the walk stops at the
/// first such box and reads no node's modifiers or semantics.
pub fn has_placed_content(applier: &mut MemoryApplier, root: NodeId) -> Result<bool, NodeError> {
    walk_placed_boxes(applier, root, |_| std::ops::ControlFlow::Break(()))
}

/// The far right and bottom edges of the boxes with area placed under `root`,
/// measured from the root's origin as [`build_layout_tree_from_applier`]
/// places them, reading no node's modifiers or semantics. `None` while
/// nothing under `root` has area.
pub fn placed_content_extent(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<Option<Size>, NodeError> {
    let mut extent: Option<Size> = None;
    walk_placed_boxes(applier, root, |rect| {
        let (right, bottom) = (rect.x + rect.width, rect.y + rect.height);
        extent = Some(extent.map_or_else(
            || Size::new(right, bottom),
            |extent| Size::new(extent.width.max(right), extent.height.max(bottom)),
        ));
        std::ops::ControlFlow::Continue(())
    })?;
    Ok(extent)
}

/// Hands `visit` every box with area placed under `root`, the root left out,
/// at the rect [`build_layout_tree_from_applier`] gives it, skipping a window
/// root's subtree and an unplaced node's, until `visit` breaks. Answers
/// whether it did.
fn walk_placed_boxes(
    applier: &mut MemoryApplier,
    root: NodeId,
    mut visit: impl FnMut(GeometryRect) -> std::ops::ControlFlow<()>,
) -> Result<bool, NodeError> {
    let mut pending: Vec<(NodeId, Point)> = Vec::new();
    let placed = read_layout_node(applier, root, |state, children| {
        if state.is_placed() {
            pending.extend(
                children
                    .iter()
                    .map(|&child| (child, state.content_offset())),
            );
        }
    })?;
    if placed.is_none() {
        return Ok(false);
    }
    while let Some((node_id, origin)) = pending.pop() {
        if crate::modifier::is_window_root(applier, node_id) {
            continue;
        }
        let rect = read_layout_node(applier, node_id, |state, children| {
            if !state.is_placed() {
                return None;
            }
            let (rect, content) = semantics_placement(&state, Some(origin));
            pending.extend(children.iter().map(|&child| (child, content)));
            Some(rect)
        })?
        .flatten();
        if let Some(rect) = rect
            && rect.width > 0.0
            && rect.height > 0.0
            && visit(rect).is_break()
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn layout_tree_origin(root: Option<LayoutState>) -> Point {
    let Some(state) = root else {
        return Point::default();
    };
    let position = state.position();
    Point {
        x: -position.x,
        y: -position.y,
    }
}

/// Places the root at the origin, the one placement no parent makes, and
/// names what the pass left unplaced.
fn finish_placement(applier: &mut MemoryApplier, root: NodeId, context: &AppContext) {
    if applier
        .with_node::<LayoutNode, _>(root, |node| {
            node.place(ChildPlacement::default(), None, context);
        })
        .is_err()
    {
        let _ = applier.with_node::<SubcomposeLayoutNode, _>(root, |node| {
            node.place(ChildPlacement::default(), None, context);
        });
    }
    if let Some(pass) = context.placement_pass().with_unplaced_nodes() {
        record_unplaced_parents(applier, root, pass, context);
    }
}

/// Names to the scene phase the parent of every node `pass` found placed and
/// left unplaced: no geometry setter reported those, yet their layers leave
/// the scene. Out of line: a pass rarely unplaces nodes.
#[inline(never)]
fn record_unplaced_parents(
    applier: &mut MemoryApplier,
    root: NodeId,
    pass: u64,
    context: &AppContext,
) {
    let mut parents = vec![root];
    let mut children = Vec::new();
    while let Some(parent) = parents.pop() {
        children.clear();
        if !matches!(
            read_layout_node(applier, parent, |_, ids| children.extend_from_slice(ids)),
            Ok(Some(()))
        ) {
            continue;
        }
        for &child in &children {
            match read_layout_node(applier, child, |state, _| state.unplaced_in(pass)) {
                Ok(Some(true)) => context.record_geometry_scene_node(parent),
                Ok(Some(false)) => parents.push(child),
                Ok(None) | Err(_) => {}
            }
        }
    }
}

fn read_layout_node<R>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    mut read: impl FnMut(LayoutState, &[NodeId]) -> R,
) -> Result<Option<R>, NodeError> {
    read_live_layout_node(applier, node_id, |node| {
        node.with_children(|children| read(node.state(), children))
    })
}

/// Places `node_id`'s box and its subtree. `child_stack` is shared by the
/// whole walk: each node pushes its children above its parent's, reads them
/// from there while its descendants push and pop above them, and pops them
/// when done, so no node's child list is copied out of the applier.
fn place_layout_box(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    parent_content_origin: Point,
    parent_transform: ProjectiveTransform,
    child_stack: &mut Vec<NodeId>,
) -> Result<Option<LayoutBox>, NodeError> {
    let first_child = child_stack.len();
    let Some(state) = read_layout_node(applier, node_id, |state, children| {
        if state.is_placed() {
            child_stack.extend_from_slice(children);
        }
        state
    })?
    else {
        return Ok(None);
    };
    if !state.is_placed() {
        return Ok(None);
    }

    let top_left = Point {
        x: parent_content_origin.x + state.position().x,
        y: parent_content_origin.y + state.position().y,
    };
    let rect = GeometryRect {
        x: top_left.x,
        y: top_left.y,
        width: state.size().width,
        height: state.size().height,
    };
    let (data, window_transform) =
        snapshot_node_data(applier, node_id, top_left, state.size(), parent_transform)?;
    let child_origin = Point {
        x: top_left.x + state.content_offset().x,
        y: top_left.y + state.content_offset().y,
    };
    let end = child_stack.len();
    let mut children = Vec::with_capacity(end - first_child);
    for index in first_child..end {
        let child_id = child_stack[index];
        if crate::modifier::is_window_root(applier, child_id) {
            continue;
        }
        if let Some(child) = place_layout_box(
            applier,
            child_id,
            child_origin,
            window_transform,
            child_stack,
        )? {
            children.push(child);
        }
    }
    child_stack.truncate(first_child);

    Ok(Some(LayoutBox {
        node_generation: applier.node_generation(node_id),
        ..LayoutBox::new(node_id, rect, state.content_offset(), data, children)
    }))
}

/// The modal the semantics tree of `root` would be rooted at, found without
/// building the tree: the topmost placed, visible modal that takes space,
/// or `None` when no modal is open. Unlike a tree update, it leaves the
/// nodes' semantics dirty flags alone.
pub fn top_modal_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<Option<NodeId>, NodeError> {
    // One depth-first walk on a shared stack, last child first, with a
    // modal's own step below its children's: the first modal step popped is
    // the modal drawn on top. It runs every frame, so no node's children are
    // copied and each node is looked up once.
    enum Step {
        Enter { node: NodeId, child: bool },
        Modal(NodeId),
    }

    fn push_placed(
        steps: &mut Vec<Step>,
        node: NodeId,
        reach: cranpose_foundation::SemanticsReach,
        size: Size,
        children: impl IntoIterator<Item = NodeId>,
    ) {
        if reach.hidden {
            return;
        }
        if reach.is_modal && modal_takes_space(size) {
            steps.push(Step::Modal(node));
        }
        steps.extend(
            children
                .into_iter()
                .map(|node| Step::Enter { node, child: true }),
        );
    }

    let mut steps = vec![Step::Enter {
        node: root,
        child: false,
    }];
    while let Some(step) = steps.pop() {
        let (node_id, child) = match step {
            Step::Modal(node) => return Ok(Some(node)),
            Step::Enter { node, child } => (node, child),
        };
        let node = match applier.get_mut(node_id) {
            Ok(node) => node.as_any_mut(),
            Err(NodeError::Missing { .. }) => continue,
            Err(error) => return Err(error),
        };
        if let Some(layout) = node.downcast_mut::<LayoutNode>() {
            let state = layout.layout_state();
            if state.is_placed() && !(child && layout.is_window_root()) {
                let children = layout.children.iter().copied();
                push_placed(
                    &mut steps,
                    node_id,
                    layout.semantics_reach(),
                    state.size(),
                    children,
                );
            }
        } else if let Some(subcompose) = node.downcast_mut::<SubcomposeLayoutNode>() {
            let state = subcompose.layout_state();
            if state.is_placed() {
                let reach = subcompose.semantics_reach();
                subcompose.with_active_children(|children| {
                    push_placed(
                        &mut steps,
                        node_id,
                        reach,
                        state.size(),
                        children.iter().copied(),
                    );
                });
            }
        }
    }
    Ok(None)
}

/// Selects which trees [`measure_layout_with_options`] builds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasureLayoutOptions {
    /// Build the semantics tree.
    pub collect_semantics: bool,
    /// Build the layout tree.
    pub build_layout_tree: bool,
}

impl Default for MeasureLayoutOptions {
    fn default() -> Self {
        Self {
            collect_semantics: true,
            build_layout_tree: true,
        }
    }
}

/// Check if a node or any of its descendants needs measure (selective measure optimization).
/// This can be used by the app shell to skip layout when the tree is clean.
///
/// O(1) check - just looks at root's dirty flag.
/// Works because all mutation paths bubble dirty flags to root via composer commands.
///
/// Returns Result to force caller to handle errors explicitly. No more unwrap_or(true) safety net.
pub fn tree_needs_layout(applier: &mut dyn Applier, root: NodeId) -> Result<bool, NodeError> {
    Ok(applier.get_mut(root)?.layout_dirty())
}

/// Check if the root semantics snapshot is dirty.
///
/// Semantics invalidations bubble to the root the same way layout invalidations do,
/// so a root check is sufficient to determine whether the next layout pass needs to
/// rebuild semantic data even when geometry is otherwise unchanged.
pub fn tree_needs_semantics(applier: &mut dyn Applier, root: NodeId) -> Result<bool, NodeError> {
    Ok(applier.get_mut(root)?.needs_semantics())
}

#[cfg(test)]
pub(crate) fn bubble_layout_dirty(applier: &mut MemoryApplier, node_id: NodeId) {
    cranpose_core::bubble_layout_dirty(applier as &mut dyn Applier, node_id);
}

/// Runs the measure phase for the subtree rooted at `root`.
pub fn measure_layout(
    applier: &mut MemoryApplier,
    root: NodeId,
    max_size: Size,
) -> Result<LayoutMeasurements, NodeError> {
    measure_layout_with_options(applier, root, max_size, MeasureLayoutOptions::default())
}

/// Runs the measure phase for the subtree rooted at `root` and builds only the trees that `options`
/// select.
pub fn measure_layout_with_options(
    applier: &mut MemoryApplier,
    root: NodeId,
    max_size: Size,
    options: MeasureLayoutOptions,
) -> Result<LayoutMeasurements, NodeError> {
    let mut stages = LayoutStages::start(layout_measure_telemetry_threshold_ms().is_some());
    let context = crate::render_state::require_current_app_context("a layout pass");
    context.begin_text_layout_pass();
    context.placement_pass().begin();
    process_pending_layout_repasses(&context, applier, root)?;
    stages.reached();

    let constraints = Constraints {
        min_width: 0.0,
        max_width: max_size.width,
        min_height: 0.0,
        max_height: max_size.height,
    };

    let (needs_remeasure, _needs_semantics, cached_epoch) = match applier
        .with_node::<LayoutNode, _>(root, |node| {
            (
                node.needs_measure() || Node::descendant_needs_measure(node),
                node.needs_semantics(),
                node.cache().epoch(),
            )
        }) {
        Ok(tuple) => tuple,
        Err(NodeError::TypeMismatch { .. }) => {
            let node = applier.get_mut(root)?;
            let measure_dirty = node.needs_measure() || node.descendant_needs_measure();
            let semantics_dirty = node.needs_semantics();
            (measure_dirty, semantics_dirty, 0)
        }
        Err(err) => return Err(err),
    };

    let epoch = if needs_remeasure {
        context.next_layout_cache_epoch()
    } else if cached_epoch != 0 {
        cached_epoch
    } else {
        context.current_layout_cache_epoch()
    };

    let guard = ApplierSlotGuard::new(applier, &context);
    let applier_host = guard.host();
    stages.reached();

    let builder = LayoutBuilder::new_with_epoch(Rc::clone(applier_host), epoch, &context);
    stages.reached();

    let measured = builder
        .state
        .measure_root(root, normalize_constraints(constraints))?;
    stages.reached();

    if let Ok(mut applier) = applier_host.try_borrow_typed() {
        finish_placement(&mut applier, root, &context);
    }
    stages.reached();

    let (layout_tree, semantics) = {
        let mut applier_ref = applier_host.borrow_typed();
        let layout_tree = if options.build_layout_tree {
            Some(build_layout_tree(&mut applier_ref, &measured)?)
        } else {
            None
        };
        let semantics = if options.collect_semantics {
            let semantics_tree = if let Some(layout_tree) = layout_tree.as_ref() {
                clear_semantics_dirty_flags(&mut applier_ref, &measured)?;
                build_semantics_tree_from_layout_tree(layout_tree)
            } else {
                build_semantics_tree_from_live_nodes(&mut applier_ref, &measured)?
            };
            Some(semantics_tree)
        } else {
            None
        };
        (layout_tree, semantics)
    };
    stages.reached();

    drop(builder);
    stages.reached();

    drop(guard);
    stages.reached();

    log_layout_measure_telemetry(root, &stages);

    Ok(LayoutMeasurements::new(measured, semantics, layout_tree))
}

fn process_pending_layout_repasses(
    context: &AppContext,
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<(), NodeError> {
    for node_id in context.take_modifier_slice_repass_nodes() {
        if let Ok(node) = applier.get_mut(node_id) {
            let any = node.as_any_mut();
            if let Some(layout) = any.downcast_mut::<LayoutNode>() {
                layout.mark_modifier_slices_dirty();
            } else if let Some(subcompose) = any.downcast_mut::<SubcomposeLayoutNode>() {
                subcompose.mark_modifier_slices_dirty();
            }
        }
    }
    let measure_repass_nodes = context.take_measure_repass_nodes();
    let repass_nodes = context.take_layout_repass_nodes();
    if measure_repass_nodes.is_empty() && repass_nodes.is_empty() {
        return Ok(());
    }
    for node_id in measure_repass_nodes {
        cranpose_core::bubble_measure_dirty(applier as &mut dyn Applier, node_id);
    }
    for node_id in repass_nodes {
        cranpose_core::bubble_layout_dirty(applier as &mut dyn Applier, node_id);
    }
    applier.get_mut(root)?.mark_descendant_needs_layout(false);
    Ok(())
}

/// A layout pass's builder, with the frame arena it took from `context` and
/// gives back when dropped.
struct LayoutBuilder<'a> {
    state: Rc<LayoutBuilderState>,
    context: &'a AppContext,
}

impl<'a> LayoutBuilder<'a> {
    fn new_with_epoch(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        epoch: u64,
        context: &'a Rc<AppContext>,
    ) -> Self {
        Self {
            state: Rc::new(LayoutBuilderState::new_with_epoch(
                applier,
                epoch,
                Rc::clone(context),
            )),
            context,
        }
    }
}

impl Drop for LayoutBuilder<'_> {
    fn drop(&mut self) {
        if Rc::strong_count(&self.state) != 1 {
            return;
        }
        let Ok(mut frame_arena) = self.state.frame_arena.try_borrow_mut() else {
            return;
        };
        self.context
            .replace_layout_frame_arena(std::mem::take(&mut *frame_arena));
    }
}

struct LayoutBuilderState {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: RefCell<Option<RuntimeHandle>>,
    cache_epoch: u64,
    cache_floor: u64,
    frame_arena: RefCell<FrameLayoutArena>,
    /// The context the pass lays out in, which takes the nodes whose
    /// geometry it changed.
    context: Rc<AppContext>,
    /// The threshold of the measure telemetry, read once a pass: `None`
    /// while the telemetry is off.
    measure_telemetry_ms: Option<f64>,
}

struct LayoutRuntimeFrameBindingCleanup<'a> {
    state: &'a RefCell<LayoutRuntimeState>,
}

impl Drop for LayoutRuntimeFrameBindingCleanup<'_> {
    fn drop(&mut self) {
        self.state.borrow().unbind_children();
    }
}

enum LayoutNodeVisit<'a> {
    Cached(Rc<MeasuredNode>),
    /// Only nodes below this one changed: its measurement holds unless a
    /// changed child measures differently.
    Keep(KeptNode),
    Measure(LayoutNodeMeasure<'a>),
}

/// A node whose measurement may hold, with the parts of it the keep reads
/// and writes, so the keep needs no lookup of the node.
struct KeptNode {
    measured: Rc<MeasuredNode>,
    parts: Rc<LayoutParts>,
    /// Whether a child that measures to another size leaves the node as it
    /// is.
    ignores_child_sizes: bool,
}

/// A kept measurement after the walk over its changed children: `resized`
/// when one of them measured to another size that the node ignores.
struct KeptChildren {
    measured: Rc<MeasuredNode>,
    resized: bool,
}

/// What measuring one changed child of a kept node again needs.
#[derive(Clone, Copy)]
struct ChangedChild {
    constraints: Constraints,
    placed: bool,
}

/// What the walk over a kept measurement finds of one child.
enum ChildChange {
    Unchanged,
    Changed(ChangedChild),
    /// The node has to run its policy.
    Refuse,
}

/// A changed child of a kept node as a layout pass finds it, from a layout
/// node or a subcompose node.
struct ChildView {
    self_dirty: bool,
    /// The child's parent data, read only when the child changed itself:
    /// only its own modifiers give it.
    parent_data: Option<crate::modifier::SharedParentData>,
    /// The constraints the child's own cache holds.
    cached_constraints: Option<Constraints>,
    read_intrinsics: bool,
    placed: bool,
}

impl ChildView {
    /// The child, when it or a node below it changed.
    fn of(node: &LiveLayoutNode<'_>) -> Option<Self> {
        let cache = node.cache();
        let dirt = cache.dirt();
        let self_dirty = dirt.measure || dirt.layout;
        if !self_dirty && !dirt.descendant.layout {
            return None;
        }
        Some(Self {
            self_dirty,
            parent_data: self_dirty.then(|| node.parent_data()),
            cached_constraints: cache.measured_constraints(),
            read_intrinsics: cache.has_intrinsics(),
            placed: node.is_placed(),
        })
    }
}

struct LayoutNodeMeasure<'a> {
    parts: Rc<LayoutParts>,
    chain: ModifierChainInputs,
    pools: VecPools<'a>,
    spare: MeasuredSpare,
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the bottom-up pass, the chain measure and the subcompose measure in their files"
)]
impl LayoutBuilderState {
    fn new_with_epoch(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        epoch: u64,
        context: Rc<AppContext>,
    ) -> Self {
        let runtime_handle = applier.borrow_typed().runtime_handle();

        Self {
            applier,
            runtime_handle: RefCell::new(runtime_handle),
            cache_epoch: epoch,
            cache_floor: context.layout_cache_floor(),
            frame_arena: RefCell::new(context.take_layout_frame_arena()),
            context,
            measure_telemetry_ms: layout_measure_telemetry_threshold_ms(),
        }
    }

    fn with_applier_result<R>(
        &self,
        f: impl FnOnce(&mut MemoryApplier) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return Err(NodeError::MissingContext {
                id: NodeId::default(),
                reason: "applier already borrowed",
            });
        };
        f(&mut applier)
    }

    fn measure_subcompose_node(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<(&'static str, Rc<MeasuredNode>), NodeError> {
        self.clear_subcompose_placed(node_id);
        if let Some(cached) = self.subcompose_measurement_to_keep(node_id, constraints)
            && let Some(kept) = self.keep_subcompose_measurement(node_id, constraints, &cached)?
        {
            return Ok(("subcompose kept", kept));
        }
        Ok(match self.try_measure_subcompose(node_id, constraints)? {
            Some(measured) => {
                self.store_subcompose_measurement(node_id, constraints, &measured);
                ("subcompose", measured)
            }
            None => (
                "fallback",
                Rc::new(MeasuredNode::new(
                    node_id,
                    Size::default(),
                    Point { x: 0.0, y: 0.0 },
                    Point::default(),
                    Vec::new(),
                )),
            ),
        })
    }

    /// The measurement of a subcompose node whose own inputs did not change
    /// since it measured at `constraints`, as for a layout node: its policy
    /// does not run, so its content does not compose again.
    fn subcompose_measurement_to_keep(
        &self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Option<Rc<MeasuredNode>> {
        let mut applier = self.applier.try_borrow_typed().ok()?;
        applier
            .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                if node.needs_measure() || Node::needs_layout(node) {
                    return None;
                }
                let cache = node.cache();
                if cache.epoch() < self.cache_floor {
                    return None;
                }
                cache.get_measurement(constraints)
            })
            .ok()
            .flatten()
    }

    /// Keeps a subcompose node's measurement as [`Self::keep_measurement`]
    /// keeps a layout node's. A subcompose node holds no record of what its
    /// policy gave each child, so a changed child must have only changed
    /// nodes below it, and measures again at the constraints its own cache
    /// holds.
    fn keep_subcompose_measurement(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
        cached: &Rc<MeasuredNode>,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let kept = self.remeasure_changed_children(
            cached,
            false,
            |child_id| self.subcompose_child_change(child_id),
            |child_id| self.mark_measured_without_policy(child_id),
        )?;
        let Some(KeptChildren { measured: kept, .. }) = kept else {
            return Ok(None);
        };
        self.with_applier_result(|applier| {
            applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.cache()
                    .store_measurement(constraints, Rc::clone(&kept));
                node.clear_needs_layout();
            })
        })?;
        Ok(Some(kept))
    }

    /// What the walk over a kept subcompose node finds of a child. A changed
    /// child refuses the keep when it changed itself, a parent read its
    /// intrinsic sizes, or its cache holds no constraints.
    fn subcompose_child_change(&self, child_id: NodeId) -> Result<ChildChange, NodeError> {
        self.child_change(child_id, |view| {
            match view.cached_constraints.filter(|_| !view.self_dirty) {
                Some(constraints) if !view.read_intrinsics => ChildChange::Changed(ChangedChild {
                    constraints,
                    placed: view.placed,
                }),
                _ => ChildChange::Refuse,
            }
        })
    }

    /// What the walk finds of a child: unchanged when neither it nor a node
    /// below it changed, otherwise what `decide` makes of it.
    fn child_change(
        &self,
        child_id: NodeId,
        decide: impl FnOnce(ChildView) -> ChildChange,
    ) -> Result<ChildChange, NodeError> {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return Ok(ChildChange::Refuse);
        };
        Ok(
            match read_live_layout_node(&mut applier, child_id, |node| ChildView::of(&node))? {
                None => ChildChange::Refuse,
                Some(None) => ChildChange::Unchanged,
                Some(Some(view)) => decide(view),
            },
        )
    }

    /// Marks a child a subcompose node measured again without its policy, so
    /// the policy checks it when it next runs: it may keep measurements of
    /// its own, as a lazy list keeps its items'.
    fn mark_measured_without_policy(&self, child_id: NodeId) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        if let Ok(node) = applier.get_mut(child_id) {
            node.mark_descendant_needs_layout(true);
        }
    }

    fn clear_subcompose_placed(&self, node_id: NodeId) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            node.clear_placed(&self.context);
        });
    }

    fn measure_node(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let Some(threshold_ms) = self.measure_telemetry_ms else {
            return self
                .measure_node_untimed(node_id, constraints)
                .map(|(_, measured)| measured);
        };
        self.measure_node_timed(node_id, constraints, threshold_ms)
    }

    // Out of line, so a measure without telemetry keeps no room for its
    // timing and log on the stack at each level of the tree.
    #[inline(never)]
    fn measure_node_timed(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
        threshold_ms: f64,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let start = Instant::now();
        let (kind, measured) = self.measure_node_untimed(node_id, constraints)?;
        log_node_measure_telemetry(kind, node_id, constraints, &measured, (threshold_ms, start));
        Ok(measured)
    }

    /// The measurement of `node_id` and the kind of node it is.
    fn measure_node_untimed(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<(&'static str, Rc<MeasuredNode>), NodeError> {
        if let Some(measured) = self.measure_layout_node(node_id, constraints)? {
            return Ok(("layout", measured));
        }
        self.measure_subcompose_node(node_id, constraints)
    }

    fn try_measure_subcompose(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let (node_handle, resolved_modifiers, spare) = {
            let Ok(mut applier) = self.applier.try_borrow_typed() else {
                return Ok(None);
            };
            let node = match applier.get_mut(node_id) {
                Ok(node) => node,
                Err(NodeError::Missing { .. }) => return Ok(None),
                Err(err) => return Err(err),
            };
            let any = node.as_any_mut();
            if let Some(subcompose) = any.downcast_mut::<SubcomposeLayoutNode>() {
                let handle = subcompose.handle();
                let resolved_modifiers = handle
                    .resolved_modifiers()
                    .on_device_grid(subcompose.density().density());
                let spare = MeasuredSpare::take(subcompose.cache());
                (handle, resolved_modifiers, spare)
            } else {
                return Ok(None);
            }
        };

        let props = resolved_modifiers.layout_properties();
        let padding = resolved_modifiers.padding();
        let offset = resolved_modifiers.offset();
        let mut inner_constraints = normalize_constraints(subtract_padding(constraints, padding));

        if let DimensionConstraint::Points(width) = props.width() {
            let constrained_width = width - padding.horizontal_sum();
            inner_constraints.max_width = inner_constraints.max_width.min(constrained_width);
            inner_constraints.min_width = inner_constraints.min_width.min(constrained_width);
        }
        if let DimensionConstraint::Points(height) = props.height() {
            let constrained_height = height - padding.vertical_sum();
            inner_constraints.max_height = inner_constraints.max_height.min(constrained_height);
            inner_constraints.min_height = inner_constraints.min_height.min(constrained_height);
        }

        let measure_error = RefCell::new(None);
        let measured_children = node_handle.measured_children_scratch();
        let measure_result = self.subcompose_policy_result(
            &node_handle,
            node_id,
            inner_constraints,
            &measured_children,
            &measure_error,
        )?;

        if let Some(err) = measure_error.borrow_mut().take() {
            return Err(err);
        }

        let cranpose_ui_layout::MeasureResult {
            size: measured_size,
            alignment_lines: explicit_lines,
            placements,
        } = measure_result;

        let mut width = measured_size.width + padding.horizontal_sum();
        let mut height = measured_size.height + padding.vertical_sum();

        width = resolve_dimension(
            width,
            props.width(),
            props.min_width(),
            props.max_width(),
            constraints.min_width,
            constraints.max_width,
        );
        height = resolve_dimension(
            height,
            props.height(),
            props.min_height(),
            props.max_height(),
            constraints.min_height,
            constraints.max_height,
        );

        let MeasuredSpare {
            node: spare,
            mut children,
        } = spare;
        children.reserve_exact(placements.len());
        let mut alignment_lines = AlignmentLines::default();
        let mut measured_children_by_id = measured_children.borrow_mut();

        // One borrow of the applier places every child; measuring a child
        // the pass did not measure borrows it on its own.
        let mut applier = self.applier.try_borrow_typed().ok();
        if let Some(applier) = applier.as_mut() {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |parent_node| {
                parent_node.set_measured_size(Size { width, height }, &self.context);
                parent_node
                    .layout_parts()
                    .note_child_placements(&placements);
                parent_node.clear_needs_measure();
                parent_node.clear_needs_layout();
            });
        }

        // A lazy policy placed its items' roots with their measurements; the
        // other placements are looked up.
        let placed_roots = node_handle.take_placed_roots();
        let mut carried = PlacedRootMeasurements::new(&placed_roots);
        for placement in &placements {
            let looked_up;
            let child: &MeasuredNode = if let Some(measured) = carried.next_measurement() {
                measured
            } else {
                looked_up =
                    if let Some(measured) = measured_children_by_id.remove(&placement.node_id) {
                        measured
                    } else {
                        drop(applier.take());
                        let measured = self.measure_node(placement.node_id, inner_constraints)?;
                        applier = self.applier.try_borrow_typed().ok();
                        measured
                    };
                &looked_up
            };
            let policy_position = Point {
                x: padding.left + placement.x,
                y: padding.top + placement.y,
            };
            let retained_position = Point {
                x: policy_position.x + child.offset.x,
                y: policy_position.y + child.offset.y,
            };
            let retained = ChildPlacement::new(retained_position, placement.z_index);
            alignment_lines.merge(
                child
                    .alignment_lines_for_parent()
                    .translated(policy_position.y),
            );

            if let Some(applier) = applier.as_mut() {
                let _ = read_live_layout_node(applier, placement.node_id, |node| {
                    node.place(retained, node_id, &self.context);
                });
            }

            children.push(MeasuredChild::new(child, policy_position));
        }
        drop(applier);

        node_handle.recycle_placed_roots(placed_roots);
        node_handle.recycle_placement_scratch(placements);

        Ok(Some(
            MeasuredNode::new(
                node_id,
                Size { width, height },
                offset,
                Point::default(),
                children,
            )
            .with_alignment_lines(
                alignment_lines
                    .with_overrides(explicit_lines.translated(padding.top))
                    .translated(offset.y),
            )
            .into_rc(spare),
        ))
    }

    fn measure_layout_node(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let mut keep = true;
        let (mut applier, measure) = loop {
            let Ok(mut applier) = self.applier.try_borrow_typed() else {
                return Ok(None);
            };
            match applier.with_node::<LayoutNode, _>(node_id, |node| {
                self.visit_layout_node(node, constraints, keep)
            }) {
                Ok(LayoutNodeVisit::Measure(measure)) => break (applier, measure),
                Ok(LayoutNodeVisit::Cached(measured)) => return Ok(Some(measured)),
                Ok(LayoutNodeVisit::Keep(node)) => {
                    drop(applier);
                    if let Some(kept) = self.keep_measurement(constraints, node)? {
                        return Ok(Some(kept));
                    }
                    keep = false;
                }
                Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => return Ok(None),
                Err(err) => return Err(err),
            }
        };
        let LayoutNodeMeasure {
            parts,
            chain,
            mut pools,
            spare,
        } = measure;

        let _frame_binding_cleanup = LayoutRuntimeFrameBindingCleanup {
            state: parts.runtime_state(),
        };
        self.bind_layout_children(
            &mut applier,
            (node_id, &parts),
            &pools.child_ids,
            ChildPass::Measure,
        )?;
        drop(applier);

        let runtime_state = parts.runtime_state().borrow();
        let measurement = self.measure_through_modifier_chain(
            node_id,
            &runtime_state,
            chain,
            constraints,
            &mut pools.placements,
            &mut pools.placement_indices,
        );

        if let Some(err) = runtime_state.take_child_error() {
            for child_state in runtime_state.child_states() {
                child_state.measured.take();
            }
            if runtime_state.take_moved_coordinators() {
                let mut layout_state = parts.layout_state().borrow_mut();
                layout_state.note_geometry_change(node_id, &self.context);
            }
            return Err(err);
        }

        let MeasuredSpare {
            node: spare,
            mut children,
        } = spare;
        runtime_state.measured_children(
            &pools.placements,
            &pools.placement_indices,
            measurement.content_offset,
            &mut children,
        );
        let measured = MeasuredNode::new(
            node_id,
            measurement.size,
            measurement.offset,
            measurement.content_offset,
            children,
        )
        .with_alignment_lines(measurement.alignment_lines)
        .with_window_root(measurement.window_root)
        .into_rc(spare);

        // The node's parts hold all its measure writes: no lookup of it.
        let cache = parts.cache();
        cache.store_held_measurement(
            constraints,
            Rc::clone(&measured),
            measurement.hold,
            measurement.ignores_child_sizes,
        );
        let mut layout_state = parts.layout_state().borrow_mut();
        if runtime_state.take_moved_coordinators() {
            layout_state.note_geometry_change(node_id, &self.context);
        }
        cache.clear_needs_measure();
        cache.clear_layout_dirt();
        layout_state.set_size(measurement.size, &self.context);
        layout_state.set_content_offset(measurement.content_offset);

        Ok(Some(measured))
    }

    /// The intrinsic size `kind` names of a layout node, through its modifier
    /// chain and measure policy, as Compose computes it: no measure, no
    /// placement, and the node's measurement cache stays as it was. `None`
    /// when the node is not a layout node.
    fn layout_node_intrinsic(
        self: &Rc<Self>,
        node_id: NodeId,
        kind: IntrinsicKind,
    ) -> Result<Option<f32>, NodeError> {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return Ok(None);
        };
        let mut pools = VecPools::acquire(&self.frame_arena);
        let bound = applier.with_node::<LayoutNode, _>(node_id, |node| {
            let parts = Rc::clone(node.layout_parts());
            let chain = parts.runtime_state().borrow_mut().bind_node(node);
            pools.child_ids.extend_from_slice(&node.children);
            (parts, chain)
        });
        let (parts, chain) = match bound {
            Ok(bound) => bound,
            Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let _frame_binding_cleanup = LayoutRuntimeFrameBindingCleanup {
            state: parts.runtime_state(),
        };
        self.bind_layout_children(
            &mut applier,
            (node_id, &parts),
            &pools.child_ids,
            ChildPass::Intrinsics,
        )?;
        drop(applier);

        let runtime_state = parts.runtime_state().borrow();
        let value = if chain.uses_chain {
            let scope = crate::density::DensityMeasureScope::new(chain.density);
            let frame = CoordinatorFrame::new(
                &runtime_state.measure_policy,
                &scope,
                runtime_state.child_measurables(),
                runtime_state.child_states(),
                &mut pools.placements,
                &mut pools.placement_indices,
            );
            runtime_state
                .coordinator_chain
                .intrinsic_from(0, &frame, kind)
        } else {
            policy_intrinsic(
                runtime_state.measure_policy.as_ref(),
                runtime_state.child_measurables(),
                kind,
            )
        };
        match runtime_state.take_child_error() {
            Some(err) => Err(err),
            None => Ok(Some(value)),
        }
    }

    /// The measurement of a node whose own inputs did not change since it
    /// measured at `constraints`: only nodes below it are dirty.
    fn measurement_to_keep(
        &self,
        node: &LayoutNode,
        constraints: Constraints,
    ) -> Option<Rc<MeasuredNode>> {
        if node.needs_measure() || node.needs_layout() || node.is_virtual() {
            return None;
        }
        let cache = node.cache();
        if cache.epoch() < self.cache_floor {
            return None;
        }
        cache.get_measurement(constraints)
    }

    // Measures again the children of `node_id` that changed, at their last
    // constraints, and keeps the node's measurement when each measures the
    // same for its parent; `None` when the node has to measure again. Out of
    // line: few frames keep a measurement, and the measure stays small.
    #[inline(never)]
    fn keep_measurement(
        self: &Rc<Self>,
        constraints: Constraints,
        node: KeptNode,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let KeptNode {
            measured,
            parts,
            ignores_child_sizes,
        } = node;
        let mut next_state = 0;
        let kept = self.remeasure_changed_children(
            &measured,
            ignores_child_sizes,
            |child_id| self.layout_child_change(parts.runtime_state(), &mut next_state, child_id),
            |_| {},
        )?;
        let Some(KeptChildren {
            measured: kept,
            resized,
        }) = kept
        else {
            return Ok(None);
        };
        let cache = parts.cache();
        if resized {
            cache.store_resized_measurement(constraints, Rc::clone(&kept));
        } else {
            cache.store_measurement(constraints, Rc::clone(&kept));
        }
        cache.clear_layout_dirt();
        Ok(Some(kept))
    }

    /// Walks the children of a kept measurement, measures again each one
    /// `change` finds changed, at the constraints it last had, and keeps the
    /// measurement when each of them reads the same for it, or, when the
    /// measurement `ignores_child_sizes`, the same but for its size. `None`
    /// as soon as `change` refuses a child or one measures differently for
    /// its parent.
    fn remeasure_changed_children(
        self: &Rc<Self>,
        cached: &Rc<MeasuredNode>,
        ignores_child_sizes: bool,
        mut change: impl FnMut(NodeId) -> Result<ChildChange, NodeError>,
        mut remeasured: impl FnMut(NodeId),
    ) -> Result<Option<KeptChildren>, NodeError> {
        let mut resized = false;
        for previous in &cached.children {
            let child = match change(previous.node_id)? {
                ChildChange::Unchanged => continue,
                ChildChange::Refuse => return Ok(None),
                ChildChange::Changed(child) => child,
            };
            let measured = self.measure_node(previous.node_id, child.constraints)?;
            remeasured(previous.node_id);
            let now = ForParent::of(&measured);
            if !previous.for_parent.same_as(&now) {
                if !(ignores_child_sizes && previous.for_parent.same_but_size(&now)) {
                    return Ok(None);
                }
                resized = true;
            }
            if child.placed {
                self.place_where_it_was(cached.node_id, previous.node_id);
            }
        }
        Ok(Some(KeptChildren {
            measured: Rc::clone(cached),
            resized,
        }))
    }

    /// What the walk over a kept layout node finds of a child, from the
    /// record of the node's last measure, which `next_state` walks in order.
    /// A changed child refuses the keep when its parent data changed, the
    /// node read its intrinsic sizes, or the node did not measure it.
    fn layout_child_change(
        &self,
        runtime_state: &RefCell<LayoutRuntimeState>,
        next_state: &mut usize,
        child_id: NodeId,
    ) -> Result<ChildChange, NodeError> {
        let runtime_state = runtime_state.borrow();
        let Some((offset, state)) = runtime_state
            .child_states()
            .get(*next_state..)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .find(|(_, state)| state.node_id == child_id)
        else {
            return Ok(ChildChange::Refuse);
        };
        *next_state += offset + 1;
        // A clean child's flags sit in the cache its record shares with it.
        if read_cell(&state.parts, |parts| {
            parts
                .as_ref()
                .is_some_and(|parts| !parts.cache().dirt().any())
        }) {
            return Ok(ChildChange::Unchanged);
        }
        self.child_change(child_id, |view| {
            let parent_data_changed = view.parent_data.as_ref().is_some_and(|data| {
                state.reads_parent_data_change(&*runtime_state.measure_policy, data)
            });
            match state.measured_constraints.get() {
                Some(constraints) if !state.read_intrinsics.get() && !parent_data_changed => {
                    ChildChange::Changed(ChangedChild {
                        constraints,
                        placed: view.placed,
                    })
                }
                _ => ChildChange::Refuse,
            }
        })
    }

    /// Stores a subcompose node's new measurement in its cache, as a layout
    /// node's measure stores its own.
    fn store_subcompose_measurement(
        &self,
        node_id: NodeId,
        constraints: Constraints,
        measured: &Rc<MeasuredNode>,
    ) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            node.cache()
                .store_measurement(constraints, Rc::clone(measured));
        });
    }

    /// Places a child of the kept node `parent` where it was: the node's
    /// policy would place it there again.
    fn place_where_it_was(&self, parent: NodeId, child_id: NodeId) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        let _ = read_live_layout_node(&mut applier, child_id, |child| {
            child.stay_placed(parent, &self.context);
        });
    }

    fn visit_layout_node(
        &self,
        node: &mut LayoutNode,
        constraints: Constraints,
        keep: bool,
    ) -> LayoutNodeVisit<'_> {
        node.clear_placed(&self.context);
        if keep && let Some(measured) = self.measurement_to_keep(node, constraints) {
            return LayoutNodeVisit::Keep(KeptNode {
                ignores_child_sizes: node.cache().ignores_child_sizes_of(&measured),
                measured,
                parts: Rc::clone(node.layout_parts()),
            });
        }
        let cache = node.cache();
        cache.activate(self.cache_epoch);
        if !Node::layout_dirty(node)
            && let Some(cached) = cache.get_measurement(constraints)
        {
            node.clear_needs_measure();
            node.clear_needs_layout();
            return LayoutNodeVisit::Cached(cached);
        }

        let spare = MeasuredSpare::take(cache);
        let parts = Rc::clone(node.layout_parts());
        let chain = parts.runtime_state().borrow_mut().bind_node(node);
        let mut pools = VecPools::acquire(&self.frame_arena);
        pools.child_ids.extend_from_slice(&node.children);
        LayoutNodeVisit::Measure(LayoutNodeMeasure {
            parts,
            chain,
            pools,
            spare,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MeasuredNode {
    node_id: NodeId,
    size: Size,
    offset: Point,
    content_offset: Point,
    alignment_lines: AlignmentLines,
    children: Vec<MeasuredChild>,
    window_root: bool,
}

impl MeasuredNode {
    fn new(
        node_id: NodeId,
        size: Size,
        offset: Point,
        content_offset: Point,
        children: Vec<MeasuredChild>,
    ) -> Self {
        Self {
            node_id,
            size,
            offset,
            content_offset,
            alignment_lines: AlignmentLines::default(),
            children,
            window_root: false,
        }
    }

    fn with_window_root(mut self, window_root: bool) -> Self {
        self.window_root = window_root;
        self
    }

    fn with_alignment_lines(mut self, alignment_lines: AlignmentLines) -> Self {
        self.alignment_lines = alignment_lines;
        self
    }

    pub(crate) fn alignment_lines_for_parent(&self) -> AlignmentLines {
        if self.window_root {
            AlignmentLines::default()
        } else {
            self.alignment_lines
        }
    }

    pub(crate) fn size_for_parent(&self) -> Size {
        if self.window_root {
            Size::new(0.0, 0.0)
        } else {
            self.size
        }
    }

    pub(crate) fn size(&self) -> Size {
        self.size
    }

    /// The measurement in `spare`'s memory, or in a new allocation when
    /// there is no spare.
    fn into_rc(self, spare: Option<Rc<Self>>) -> Rc<Self> {
        let Some(mut spare) = spare else {
            return Rc::new(self);
        };
        match Rc::get_mut(&mut spare) {
            Some(slot) => *slot = self,
            None => return Rc::new(self),
        }
        spare
    }
}

/// What a parent's measurement keeps of a child: which child, where the
/// policy placed it, and what of the child's measurement the policy read.
/// The measurement itself stays with the child's cache, its one holder, so
/// the child's next measure can fill it in place.
#[derive(Debug, Clone, Copy)]
struct MeasuredChild {
    node_id: NodeId,
    offset: Point,
    for_parent: ForParent,
}

impl MeasuredChild {
    fn new(measured: &MeasuredNode, offset: Point) -> Self {
        Self {
            node_id: measured.node_id,
            offset,
            for_parent: ForParent::of(measured),
        }
    }
}

/// What a parent's policy reads of a child's measurement: the size and the
/// baselines, and the offset it places the child by.
#[derive(Debug, Clone, Copy)]
struct ForParent {
    size: Size,
    offset: Point,
    alignment_lines: AlignmentLines,
}

impl ForParent {
    fn of(measured: &MeasuredNode) -> Self {
        Self {
            size: measured.size_for_parent(),
            offset: measured.offset,
            alignment_lines: measured.alignment_lines_for_parent(),
        }
    }

    /// Whether a parent's policy, reading `other` in place of this, would
    /// lay its children out the same.
    fn same_as(&self, other: &Self) -> bool {
        self.size == other.size && self.same_but_size(other)
    }

    /// Whether `other` reads the same as this but for its size.
    fn same_but_size(&self, other: &Self) -> bool {
        self.offset == other.offset
            && self.alignment_lines.first_baseline() == other.alignment_lines.first_baseline()
            && self.alignment_lines.last_baseline() == other.alignment_lines.last_baseline()
    }
}

/// The intrinsic size `kind` names, of a measure policy over `measurables`.
fn policy_intrinsic(
    policy: &dyn MeasurePolicy,
    measurables: &[Box<dyn Measurable>],
    kind: IntrinsicKind,
) -> f32 {
    match kind {
        IntrinsicKind::MinWidth(height) => policy.min_intrinsic_width(measurables, height),
        IntrinsicKind::MaxWidth(height) => policy.max_intrinsic_width(measurables, height),
        IntrinsicKind::MinHeight(width) => policy.min_intrinsic_height(measurables, width),
        IntrinsicKind::MaxHeight(width) => policy.max_intrinsic_height(measurables, width),
    }
}

pub(crate) struct LayoutRuntimeState {
    /// `None` until the node first binds a child: most nodes are leaves.
    children: Option<Box<LayoutChildren>>,
    coordinator_chain: CoordinatorChain,
    measure_policy: Rc<dyn MeasurePolicy>,
}

impl LayoutRuntimeState {
    pub(crate) fn new(measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        Self {
            children: None,
            coordinator_chain: CoordinatorChain::default(),
            measure_policy,
        }
    }

    fn bind_node(&mut self, node: &LayoutNode) -> ModifierChainInputs {
        if !Rc::ptr_eq(&self.measure_policy, &node.measure_policy) {
            self.measure_policy = Rc::clone(&node.measure_policy);
        }
        self.coordinator_chain.sync(node)
    }

    // Notes that a measure placed the node's coordinators, and says whether
    // one of them moved since the scene last heard of the node. A node
    // without layout modifiers moves only with its size, which the node
    // reports itself.
    fn take_moved_coordinators(&self) -> bool {
        self.coordinator_chain.take_moved()
    }

    pub(crate) fn placed_coordinator_rect(
        &self,
        ordinal: usize,
        node_size: Size,
    ) -> Option<GeometryRect> {
        self.coordinator_chain.placed_rect(ordinal, node_size)
    }

    #[cfg(test)]
    pub(crate) fn debug_stats(&self) -> LayoutRuntimeDebugStats {
        LayoutRuntimeDebugStats {
            child_ids: self
                .children
                .as_ref()
                .map(|children| children.ids.clone())
                .unwrap_or_default(),
            child_state_ptrs: self
                .child_states()
                .iter()
                .map(|state| Rc::as_ptr(state) as *const () as usize)
                .collect(),
            child_measurable_ptrs: self
                .child_measurables()
                .iter()
                .map(|measurable| {
                    measurable.as_ref() as *const dyn Measurable as *const () as usize
                })
                .collect(),
            child_measurable_count: self.child_measurables().len(),
            coordinator_node_ptrs: self.coordinator_chain.debug_ptrs(),
            coordinator_node_count: self.coordinator_chain.debug_ptrs().len(),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LayoutRuntimeDebugStats {
    pub(crate) child_ids: Vec<NodeId>,
    pub(crate) child_state_ptrs: Vec<usize>,
    pub(crate) child_measurable_ptrs: Vec<usize>,
    pub(crate) child_measurable_count: usize,
    pub(crate) coordinator_node_ptrs: Vec<usize>,
    pub(crate) coordinator_node_count: usize,
}

impl PlaceTarget for LayoutChildMeasureState {
    fn place(&self, x: f32, y: f32) {
        let internal_offset = read_cell(&self.measured, |measured| {
            measured
                .as_ref()
                .map(|measured| measured.offset)
                .unwrap_or_default()
        });
        self.place_retained(Point {
            x: x + internal_offset.x,
            y: y + internal_offset.y,
        });
    }
}

/// Runs `visit` on the measurement `child` holds in its cache, or on a leaf
/// of no size for a child that holds none, as a node of another kind
/// measures.
fn visit_child<R>(
    applier: &mut MemoryApplier,
    child: &MeasuredChild,
    visit: impl FnOnce(&mut MemoryApplier, &MeasuredNode) -> R,
) -> R {
    let held = applier.get_mut(child.node_id).ok().and_then(|node| {
        let any = node.as_any_mut();
        if let Some(layout) = any.downcast_ref::<LayoutNode>() {
            return layout.cache().held_measurement();
        }
        any.downcast_ref::<SubcomposeLayoutNode>()
            .and_then(|subcompose| subcompose.cache().held_measurement())
    });
    match held {
        Some(held) => visit(applier, &held),
        None => visit(
            applier,
            &MeasuredNode::new(
                child.node_id,
                Size::default(),
                Point::default(),
                Point::default(),
                Vec::new(),
            ),
        ),
    }
}

fn clear_semantics_dirty_flags(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<(), NodeError> {
    match applier.with_node::<LayoutNode, _>(node.node_id, |layout| {
        layout.clear_needs_semantics();
    }) {
        Ok(()) => {}
        Err(NodeError::Missing { .. }) => {}
        Err(NodeError::TypeMismatch { .. }) => {
            match applier.with_node::<SubcomposeLayoutNode, _>(node.node_id, |subcompose| {
                subcompose.clear_needs_semantics();
            }) {
                Ok(()) | Err(NodeError::Missing { .. } | NodeError::TypeMismatch { .. }) => {}
                Err(err) => return Err(err),
            }
        }
        Err(err) => return Err(err),
    }

    for child in &node.children {
        visit_child(applier, child, clear_semantics_dirty_flags)?;
    }

    Ok(())
}

fn build_semantics_tree_from_live_nodes(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<SemanticsTree, NodeError> {
    Ok(SemanticsTree::new(build_semantics_node_from_live_nodes(
        applier, node, None,
    )?))
}

fn semantics_node_from_parts(
    node_id: NodeId,
    node_generation: u32,
    mut role: SemanticsRole,
    config: Option<SemanticsConfiguration>,
    children: Vec<SemanticsNode>,
    held_details: Option<Box<SemanticsDetails>>,
    bounds: GeometryRect,
) -> SemanticsNode {
    let focusable = crate::focus_dispatch::has_focus_target(node_id);
    let mut node = SemanticsNode {
        node_id,
        bounds,
        node_generation,
        children,
        focusable,
        focused: focusable && crate::focus_dispatch::active_focus_target() == Some(node_id),
        details: held_details,
        ..SemanticsNode::default()
    };
    let details = match config {
        Some(mut config) => {
            if config.role == Some(SemanticsWidgetRole::Button) {
                role = SemanticsRole::Button;
            }
            if config.is_activatable() {
                node.actions.push(SemanticsAction::Click {
                    handler: SemanticsCallback::new(node_id),
                });
            }
            let on_click_label = config.on_click_label.take().or_else(|| {
                config
                    .on_click
                    .as_ref()
                    .and_then(|action| (!action.label.is_empty()).then(|| action.label.clone()))
            });
            node.widget_role = config.role;
            node.description = config.content_description.take();
            node.on_click = config.on_click.take();
            node.selected = config.selected;
            node.toggled = config.toggled;
            node.enabled = config.enabled;
            node.hidden = config.hidden;
            node.merge_descendants = config.merge_descendants;
            node.traversal_index = config.traversal_index;
            node.text = config.text.take();
            let is_modal = config.is_modal
                && modal_takes_space(Size {
                    width: bounds.width,
                    height: bounds.height,
                });
            SemanticsDetails::from_configuration(config, on_click_label, is_modal)
        }
        None => SemanticsDetails::NONE,
    };
    node.set_details(details);
    node.role = role;
    node
}

#[inline(never)]
fn build_semantics_node_from_live_nodes(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
    origin: Option<Point>,
) -> Result<SemanticsNode, NodeError> {
    let (role, config, (bounds, content), placement) =
        match applier.with_node::<LayoutNode, _>(node.node_id, |layout| {
            let role = role_from_modifier_slices(&layout.modifier_slices_snapshot());
            let config = layout.semantics_configuration();
            layout.clear_needs_semantics();
            let state = layout.layout_state();
            (
                role,
                config,
                semantics_placement(&state, origin),
                SemanticsPlacement::of(&state),
            )
        }) {
            Ok(data) => data,
            Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => {
                match applier.with_node::<SubcomposeLayoutNode, _>(node.node_id, |subcompose| {
                    subcompose.clear_needs_semantics();
                    let state = subcompose.layout_state();
                    (
                        SemanticsRole::Subcompose,
                        subcompose.semantics_configuration(),
                        semantics_placement(&state, origin),
                        SemanticsPlacement::of(&state),
                    )
                }) {
                    Ok(data) => data,
                    Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => {
                        let top_left = origin.unwrap_or_default();
                        (
                            SemanticsRole::Unknown,
                            None,
                            (
                                GeometryRect::from_origin_size(top_left, node.size),
                                top_left,
                            ),
                            SemanticsPlacement::default(),
                        )
                    }
                    Err(err) => return Err(err),
                }
            }
            Err(err) => return Err(err),
        };

    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        children.push(visit_child(applier, child, |applier, child| {
            build_semantics_node_from_live_nodes(applier, child, Some(content))
        })?);
    }

    Ok(SemanticsNode {
        placement,
        ..semantics_node_from_parts(
            node.node_id,
            applier.node_generation(node.node_id),
            role,
            config,
            children,
            None,
            bounds,
        )
    })
}

fn record_semantics_allocation_stats(node: &SemanticsNode, stats: &mut LayoutAllocationDebugStats) {
    stats.semantics_node_count += 1;
    stats.semantics_action_count += node.actions.len();
    stats.semantics_action_capacity += node.actions.capacity();
    stats.semantics_child_count += node.children.len();
    stats.semantics_child_capacity += node.children.capacity();
    stats.semantics_heap_bytes += node.actions.capacity() * size_of::<SemanticsAction>();
    stats.semantics_heap_bytes += node.children.capacity() * size_of::<SemanticsNode>();
    if node.details.is_some() {
        stats.semantics_heap_bytes += size_of::<SemanticsDetails>();
    }

    if let Some(description) = &node.description {
        stats.semantics_description_count += 1;
        stats.semantics_description_bytes += description.capacity();
        stats.semantics_heap_bytes += description.capacity();
    }

    for child in &node.children {
        record_semantics_allocation_stats(child, stats);
    }
}

fn record_layout_box_allocation_stats(
    layout_box: &LayoutBox,
    stats: &mut LayoutAllocationDebugStats,
) {
    stats.layout_box_count += 1;
    stats.layout_box_child_count += layout_box.children.len();
    stats.layout_box_child_capacity += layout_box.children.capacity();
    stats.layout_box_heap_bytes += layout_box.children.capacity() * size_of::<LayoutBox>();
    stats.add_modifier_slice(layout_box.node_data.modifier_slices().debug_stats());

    for child in &layout_box.children {
        record_layout_box_allocation_stats(child, stats);
    }
}

fn build_layout_tree(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<LayoutTree, NodeError> {
    fn place(
        applier: &mut MemoryApplier,
        node: &MeasuredNode,
        origin: Point,
        parent_transform: ProjectiveTransform,
    ) -> Result<LayoutBox, NodeError> {
        let top_left = Point {
            x: origin.x + node.offset.x,
            y: origin.y + node.offset.y,
        };
        let rect = GeometryRect {
            x: top_left.x,
            y: top_left.y,
            width: node.size.width,
            height: node.size.height,
        };
        let (data, window_transform) =
            snapshot_node_data(applier, node.node_id, top_left, node.size, parent_transform)?;
        let mut children = Vec::with_capacity(node.children.len());
        for child in &node.children {
            if crate::modifier::is_window_root(applier, child.node_id) {
                continue;
            }
            let child_origin = Point {
                x: top_left.x + child.offset.x,
                y: top_left.y + child.offset.y,
            };
            children.push(visit_child(applier, child, |applier, child| {
                place(applier, child, child_origin, window_transform)
            })?);
        }
        Ok(LayoutBox {
            node_generation: applier.node_generation(node.node_id),
            ..LayoutBox::new(node.node_id, rect, node.content_offset, data, children)
        })
    }

    Ok(LayoutTree::new(place(
        applier,
        node,
        Point { x: 0.0, y: 0.0 },
        ProjectiveTransform::identity(),
    )?))
}

fn semantics_role_from_layout_box(layout_box: &LayoutBox) -> SemanticsRole {
    match &layout_box.node_data.kind {
        LayoutNodeKind::Subcompose => SemanticsRole::Subcompose,
        LayoutNodeKind::Spacer => SemanticsRole::Spacer,
        LayoutNodeKind::Unknown => SemanticsRole::Unknown,
        LayoutNodeKind::Button { .. } => SemanticsRole::Button,
        LayoutNodeKind::Layout => role_from_modifier_slices(layout_box.node_data.modifier_slices()),
    }
}

#[inline(never)]
fn build_semantics_node_from_layout_box(layout_box: &LayoutBox, origin: Point) -> SemanticsNode {
    let rect = layout_box.rect;
    let content = Point {
        x: rect.x + layout_box.content_offset.x,
        y: rect.y + layout_box.content_offset.y,
    };
    let children = layout_box
        .children
        .iter()
        .map(|child| build_semantics_node_from_layout_box(child, content))
        .collect();

    SemanticsNode {
        placement: SemanticsPlacement {
            position: Point {
                x: rect.x - origin.x,
                y: rect.y - origin.y,
            },
            content_offset: layout_box.content_offset,
        },
        ..semantics_node_from_parts(
            layout_box.node_id,
            layout_box.node_generation,
            semantics_role_from_layout_box(layout_box),
            layout_box.node_data.semantics().cloned(),
            children,
            None,
            rect,
        )
    }
}

fn subtract_padding(constraints: Constraints, padding: EdgeInsets) -> Constraints {
    let horizontal = padding.horizontal_sum();
    let vertical = padding.vertical_sum();
    let min_width = (constraints.min_width - horizontal).max(0.0);
    let mut max_width = constraints.max_width;
    if max_width.is_finite() {
        max_width = (max_width - horizontal).max(0.0);
    }
    let min_height = (constraints.min_height - vertical).max(0.0);
    let mut max_height = constraints.max_height;
    if max_height.is_finite() {
        max_height = (max_height - vertical).max(0.0);
    }
    normalize_constraints(Constraints {
        min_width,
        max_width,
        min_height,
        max_height,
    })
}

fn resolve_dimension(
    base: f32,
    explicit: DimensionConstraint,
    min_override: Option<f32>,
    max_override: Option<f32>,
    min_limit: f32,
    max_limit: f32,
) -> f32 {
    let mut min_bound = min_limit;
    if let Some(min_value) = min_override {
        min_bound = min_bound.max(min_value);
    }

    let mut max_bound = if max_limit.is_finite() {
        max_limit
    } else {
        max_override.unwrap_or(max_limit)
    };
    if let Some(max_value) = max_override {
        if max_bound.is_finite() {
            max_bound = max_bound.min(max_value);
        } else {
            max_bound = max_value;
        }
    }
    if max_bound < min_bound {
        max_bound = min_bound;
    }

    let mut size = match explicit {
        DimensionConstraint::Points(points) => points,
        DimensionConstraint::Fraction(fraction) => {
            if max_limit.is_finite() {
                max_limit * fraction.clamp(0.0, 1.0)
            } else {
                base
            }
        }
        DimensionConstraint::Unspecified => base,
        DimensionConstraint::Intrinsic(_) => base,
    };

    size = clamp_dimension(size, min_bound, max_bound);
    size = clamp_dimension(size, min_limit, max_limit);
    size.max(0.0)
}

fn clamp_dimension(value: f32, min: f32, max: f32) -> f32 {
    let mut result = value.max(min);
    if max.is_finite() {
        result = result.min(max);
    }
    result
}

fn normalize_constraints(mut constraints: Constraints) -> Constraints {
    if constraints.max_width < constraints.min_width {
        constraints.max_width = constraints.min_width;
    }
    if constraints.max_height < constraints.min_height {
        constraints.max_height = constraints.min_height;
    }
    constraints
}

#[cfg(test)]
#[path = "tests/layout_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/semantics_update_tests.rs"]
mod semantics_update_tests;

#[cfg(test)]
#[path = "tests/coordinator_geometry_tests.rs"]
mod coordinator_geometry_tests;
