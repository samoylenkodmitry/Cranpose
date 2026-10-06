pub mod core;
pub mod policies;
mod reveal;
mod semantics_details;
mod semantics_labels;
mod semantics_update;

use std::{
    cell::{Cell, RefCell},
    fmt,
    mem::size_of,
    rc::Rc,
    sync::OnceLock,
};

use cranpose_core::{
    Applier, ApplierHost, Composer, ConcreteApplierHost, MemoryApplier, Node, NodeError, NodeId,
    Phase, RuntimeHandle, SlotTable, SlotsHost,
};
use cranpose_foundation::{
    InvalidationKind, ModifierNodeContext, NodeCapabilities, SemanticsConfiguration,
    SemanticsCustomAction, SemanticsWidgetRole,
};
use cranpose_ui_graphics::{ProjectiveTransform, layer_transform::layer_transform_to_window};
use cranpose_ui_layout::{AlignmentLines, Constraints, MeasurePolicy, PlaceTarget, Placement};
use web_time::Instant;

#[cfg(test)]
use self::core::{HorizontalAlignment, VerticalAlignment};
use self::{
    core::{Measurable, Placeable},
    semantics_update::semantics_placement,
};
pub use self::{
    reveal::can_skip_scroll_reveal_from_applier,
    semantics_details::SemanticsDetails,
    semantics_update::{build_semantics_tree_from_applier, update_semantics_tree_from_applier},
};
use crate::{
    modifier::{
        DimensionConstraint, EdgeInsets, Modifier, ModifierNodeSlices,
        ModifierNodeSlicesDebugStats, Point, Rect as GeometryRect, ResolvedModifiers, Size,
    },
    subcompose_layout::{CachedBatchMeasureInputs, SubcomposeLayoutNode},
    widgets::nodes::{
        IntrinsicKind, LayoutNode, LayoutNodeCacheHandles, LayoutState, begin_placement_pass,
        placement_pass_with_unplaced_nodes,
    },
};

#[derive(Default)]
pub(crate) struct LayoutNodeContext {
    invalidations: Vec<InvalidationKind>,
    update_requested: bool,
    active_capabilities: Vec<NodeCapabilities>,
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

    fn push_active_capabilities(&mut self, capabilities: NodeCapabilities) {
        self.active_capabilities.push(capabilities);
    }

    fn pop_active_capabilities(&mut self) {
        self.active_capabilities.pop();
    }

    fn density(&self) -> f32 {
        self.density
    }
}

#[doc(hidden)]
pub fn invalidate_all_layout_caches() {
    crate::render_state::invalidate_layout_cache_epoch();
}

fn layout_measure_telemetry_threshold_ms() -> Option<f64> {
    static THRESHOLD_MS: OnceLock<Option<f64>> = OnceLock::new();
    *THRESHOLD_MS.get_or_init(|| {
        std::env::var("CRANPOSE_LAYOUT_MEASURE_TELEMETRY_MS")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .or_else(|| {
                std::env::var_os("CRANPOSE_LAYOUT_MEASURE_TELEMETRY")
                    .is_some()
                    .then_some(4.0)
            })
    })
}

struct LayoutMeasureTelemetry {
    root: NodeId,
    start: Instant,
    after_repasses: Instant,
    after_guard: Instant,
    after_builder: Instant,
    after_measure: Instant,
    after_root_place: Instant,
    after_aux: Instant,
    after_builder_drop: Instant,
    after_guard_drop: Instant,
}

fn log_layout_measure_telemetry(times: LayoutMeasureTelemetry) {
    let Some(threshold_ms) = layout_measure_telemetry_threshold_ms() else {
        return;
    };

    let total_ms = times
        .after_guard_drop
        .duration_since(times.start)
        .as_secs_f64()
        * 1000.0;
    if total_ms < threshold_ms {
        return;
    }

    let repass_ms = times
        .after_repasses
        .duration_since(times.start)
        .as_secs_f64()
        * 1000.0;
    let guard_ms = times
        .after_guard
        .duration_since(times.after_repasses)
        .as_secs_f64()
        * 1000.0;
    let builder_ms = times
        .after_builder
        .duration_since(times.after_guard)
        .as_secs_f64()
        * 1000.0;
    let measure_ms = times
        .after_measure
        .duration_since(times.after_builder)
        .as_secs_f64()
        * 1000.0;
    let root_place_ms = times
        .after_root_place
        .duration_since(times.after_measure)
        .as_secs_f64()
        * 1000.0;
    let aux_ms = times
        .after_aux
        .duration_since(times.after_root_place)
        .as_secs_f64()
        * 1000.0;
    let builder_drop_ms = times
        .after_builder_drop
        .duration_since(times.after_aux)
        .as_secs_f64()
        * 1000.0;
    let guard_drop_ms = times
        .after_guard_drop
        .duration_since(times.after_builder_drop)
        .as_secs_f64()
        * 1000.0;
    log::warn!(
        "[layout-measure-telemetry] root={} total_ms={total_ms:.2} repass_ms={repass_ms:.2} guard_ms={guard_ms:.2} builder_ms={builder_ms:.2} measure_ms={measure_ms:.2} root_place_ms={root_place_ms:.2} aux_ms={aux_ms:.2} builder_drop_ms={builder_drop_ms:.2} guard_drop_ms={guard_drop_ms:.2}",
        times.root
    );
}

fn log_node_measure_telemetry(
    kind: &'static str,
    node_id: NodeId,
    constraints: Constraints,
    measured: &MeasuredNode,
    (threshold_ms, start): (f64, Instant),
) {
    let size = measured.size;
    let children = measured.children.len();
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    if total_ms < threshold_ms {
        return;
    }

    log::warn!(
        "[layout-node-telemetry] kind={kind} node={} total_ms={total_ms:.2} constraints=({:.1},{:.1},{:.1},{:.1}) size=({:.1},{:.1}) children={children}",
        node_id,
        constraints.min_width,
        constraints.max_width,
        constraints.min_height,
        constraints.max_height,
        size.width,
        size.height,
    );
}

struct ApplierSlotGuard<'a> {
    target: &'a mut MemoryApplier,
    host: Rc<ConcreteApplierHost<MemoryApplier>>,
    slots: Rc<RefCell<SlotTable>>,
}

impl<'a> ApplierSlotGuard<'a> {
    fn new(target: &'a mut MemoryApplier) -> Self {
        let original_applier = std::mem::replace(target, MemoryApplier::new());
        let host = Rc::new(ConcreteApplierHost::new(original_applier));

        let slots = {
            let mut applier_ref = host.borrow_typed();
            std::mem::take(applier_ref.slots())
        };
        let slots = Rc::new(RefCell::new(slots));

        Self {
            target,
            host,
            slots,
        }
    }

    fn host(&self) -> Rc<ConcreteApplierHost<MemoryApplier>> {
        Rc::clone(&self.host)
    }

    fn slots_handle(&self) -> Rc<RefCell<SlotTable>> {
        Rc::clone(&self.slots)
    }
}

impl Drop for ApplierSlotGuard<'_> {
    fn drop(&mut self) {
        {
            let mut applier_ref = self.host.borrow_typed();
            *applier_ref.slots() = std::mem::take(&mut *self.slots.borrow_mut());
        }

        {
            let mut applier_ref = self.host.borrow_typed();
            let original_applier = std::mem::take(&mut *applier_ref);
            let _ = std::mem::replace(self.target, original_applier);
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
    uses_chain: bool,
}

struct ScratchVecPool<T> {
    available: Vec<Vec<T>>,
}

impl<T> ScratchVecPool<T> {
    fn acquire(&mut self) -> Vec<T> {
        self.available.pop().unwrap_or_default()
    }

    fn release(&mut self, mut values: Vec<T>) {
        values.clear();
        self.available.push(values);
    }

    #[cfg(test)]
    fn available_count(&self) -> usize {
        self.available.len()
    }
}

impl<T> Default for ScratchVecPool<T> {
    fn default() -> Self {
        Self {
            available: Vec::new(),
        }
    }
}

#[derive(Default)]
pub(crate) struct FrameLayoutArena {
    tmp_child_ids: ScratchVecPool<NodeId>,
    tmp_placements: ScratchVecPool<Placement>,
}

#[cfg(test)]
impl FrameLayoutArena {
    pub(crate) fn available_placement_scratch_count(&self) -> usize {
        self.tmp_placements.available_count()
    }

    pub(crate) fn seed_placement_scratch_for_test(&mut self) {
        self.tmp_placements.release(Vec::with_capacity(1));
    }
}

/// Discrete event callback reference produced during semantics extraction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticsCallback {
    node_id: NodeId,
}

impl SemanticsCallback {
    pub fn new(node_id: NodeId) -> Self {
        Self { node_id }
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }
}

/// Semantics action exposed to the input system.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsAction {
    Click { handler: SemanticsCallback },
}

/// A text node's text in the semantics tree, shared with the node's modifier
/// slices rather than copied out of them.
#[derive(Clone, Debug)]
pub struct SemanticsText(Rc<crate::text::AnnotatedString>);

impl SemanticsText {
    /// The text the node shows.
    pub fn as_str(&self) -> &str {
        &self.0.text
    }
}

impl PartialEq for SemanticsText {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SemanticsText {}

impl From<&str> for SemanticsText {
    fn from(text: &str) -> Self {
        Self(Rc::new(crate::text::AnnotatedString::from(text)))
    }
}

/// Semantic role describing how a node should participate in accessibility and hit testing.
/// Roles are now derived from SemanticsConfiguration rather than widget types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsRole {
    /// Generic container or layout node
    Layout,
    /// Subcomposition boundary
    Subcompose,
    /// Text content derived from the text node semantics payload.
    Text { value: SemanticsText },
    /// Spacer (non-interactive)
    Spacer,
    /// Button (derived from the semantics `role`)
    Button,
    /// Unknown or unspecified role
    Unknown,
}

/// A single node within the semantics tree.
///
/// Not `Eq`: a node now carries drawn-control bounds, which are floats. The
/// tree is compared for "did anything a screen reader can see change", and
/// that comparison is `PartialEq` everywhere else on the accessibility path
/// (`AccessibilityRect`, `AccessibilityElement`) for the same reason.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsNode {
    pub node_id: NodeId,
    /// Where the node lies in the root's coordinates: its layout rect,
    /// before graphics-layer transforms, as [`LayoutBox::rect`] holds it.
    pub bounds: GeometryRect,
    /// Where layout put the node inside its parent's content, from which a
    /// tree update moves a node that did not change along with its parent.
    pub placement: SemanticsPlacement,
    /// Incarnation of the runtime node, incremented when its storage is recycled.
    pub node_generation: u32,
    /// Where this node sits in the tree (layout, text, subcomposition …).
    pub role: SemanticsRole,
    /// What kind of control this node is, as a screen reader announces it —
    /// Compose's `Role`. Separate from [`SemanticsNode::role`] because a
    /// clickable `Row` is structurally a layout node and semantically a button.
    pub widget_role: Option<SemanticsWidgetRole>,
    pub actions: Vec<SemanticsAction>,
    pub children: Vec<SemanticsNode>,
    pub description: Option<String>,
    /// Direct activation callback for keyboard and assistive technology.
    pub on_click: Option<SemanticsCustomAction>,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
    pub enabled: bool,
    /// Whether a screen reader skips this node and everything under it.
    pub hidden: bool,
    /// Whether a screen reader takes this node and the text under it as one
    /// stop.
    pub merge_descendants: bool,
    /// Where a screen reader visits this node among the ones beside it.
    pub traversal_index: f32,
    /// The text an editable field holds.
    pub text: Option<String>,
    /// Whether this node registered a focus target, so focus can land on it.
    /// Compose's `SemanticsProperties.Focused` companion.
    pub focusable: bool,
    /// Whether focus sits on this node right now.
    pub focused: bool,
    /// The properties few nodes set, held only when the node sets one: read
    /// them through [`SemanticsNode::details`] and write them through
    /// [`SemanticsNode::set_details`].
    pub details: Option<Box<SemanticsDetails>>,
}

impl Default for SemanticsNode {
    fn default() -> Self {
        Self {
            node_id: 0,
            bounds: GeometryRect::EMPTY,
            placement: SemanticsPlacement::default(),
            node_generation: 0,
            role: SemanticsRole::Unknown,
            widget_role: None,
            actions: Vec::new(),
            children: Vec::new(),
            description: None,
            on_click: None,
            selected: None,
            toggled: None,
            enabled: true,
            hidden: false,
            merge_descendants: false,
            traversal_index: 0.0,
            text: None,
            focusable: false,
            focused: false,
            details: None,
        }
    }
}

/// Where layout put a semantics node: its offset inside its parent's
/// content and where its own content starts inside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SemanticsPlacement {
    position: Point,
    content_offset: Point,
}

impl SemanticsPlacement {
    fn of(state: &LayoutState) -> Self {
        Self {
            position: state.position(),
            content_offset: state.content_offset(),
        }
    }

    fn place(self, origin: Point) -> (Point, Point) {
        let top_left = Point {
            x: origin.x + self.position.x,
            y: origin.y + self.position.y,
        };
        let content = Point {
            x: top_left.x + self.content_offset.x,
            y: top_left.y + self.content_offset.y,
        };
        (top_left, content)
    }
}

/// Rooted semantics tree extracted after layout.
///
/// Two trees are equal when they hold the same nodes; what a tree keeps to
/// bring itself up to date is not compared.
#[derive(Clone, Debug)]
pub struct SemanticsTree {
    full: SemanticsNode,
    modal: Option<Vec<usize>>,
    tracking: semantics_update::SemanticsTracking,
}

impl PartialEq for SemanticsTree {
    fn eq(&self, other: &Self) -> bool {
        self.full == other.full && self.modal == other.modal
    }
}

impl SemanticsTree {
    fn new(full: SemanticsNode) -> Self {
        let modal = top_modal_path(&full);
        Self {
            full,
            modal,
            tracking: semantics_update::SemanticsTracking::default(),
        }
    }

    /// Returns the top visible modal subtree, or the full root when no modal is open.
    pub fn root(&self) -> &SemanticsNode {
        self.modal.as_deref().map_or(&self.full, |path| {
            path.iter()
                .try_fold(&self.full, |node, &index| node.children.get(index))
                .unwrap_or(&self.full)
        })
    }
}

/// Whether a modal of `size` takes space; a zero-sized modal hides nothing.
fn modal_takes_space(size: Size) -> bool {
    size.width > 0.0 && size.height > 0.0 && size.width.is_finite() && size.height.is_finite()
}

/// The path to the modal drawn on top: the last visible modal in document
/// order, a modal inside another preferred to it.
fn top_modal_path(root: &SemanticsNode) -> Option<Vec<usize>> {
    fn search(node: &SemanticsNode, path: &mut Vec<usize>) -> bool {
        if node.hidden {
            return false;
        }
        for (index, child) in node.children.iter().enumerate().rev() {
            path.push(index);
            if search(child, path) {
                return true;
            }
            path.pop();
        }
        node.details().is_modal
    }
    let mut path = Vec::new();
    search(root, &mut path).then_some(path)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutAllocationDebugStats {
    pub layout_box_count: usize,
    pub layout_box_child_count: usize,
    pub layout_box_child_capacity: usize,
    pub layout_box_heap_bytes: usize,
    pub modifier_slice_count: usize,
    pub modifier_slice_heap_bytes: usize,
    pub modifier_draw_command_count: usize,
    pub modifier_draw_command_capacity: usize,
    pub modifier_pointer_input_count: usize,
    pub modifier_pointer_input_capacity: usize,
    pub modifier_text_content_count: usize,
    pub modifier_text_style_count: usize,
    pub modifier_text_layout_options_count: usize,
    pub modifier_prepared_text_layout_count: usize,
    pub modifier_graphics_layer_count: usize,
    pub modifier_graphics_layer_resolver_count: usize,
    pub semantics_node_count: usize,
    pub semantics_action_count: usize,
    pub semantics_action_capacity: usize,
    pub semantics_child_count: usize,
    pub semantics_child_capacity: usize,
    pub semantics_description_count: usize,
    pub semantics_description_bytes: usize,
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
    pub fn new(root: LayoutBox) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &LayoutBox {
        &self.root
    }

    pub fn root_mut(&mut self) -> &mut LayoutBox {
        &mut self.root
    }

    pub fn into_root(self) -> LayoutBox {
        self.root
    }

    pub fn debug_allocation_stats(&self) -> LayoutAllocationDebugStats {
        let mut stats = LayoutAllocationDebugStats::default();
        record_layout_box_allocation_stats(&self.root, &mut stats);
        stats
    }
}

/// Layout information for a single node.
#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub node_id: NodeId,
    /// Incarnation of the runtime node captured with these layout bounds.
    pub node_generation: u32,
    pub rect: GeometryRect,
    /// Content offset for scroll/inner transforms (applies to children, NOT this node's position)
    pub content_offset: Point,
    pub node_data: LayoutNodeData,
    pub children: Vec<LayoutBox>,
}

impl LayoutBox {
    pub fn new(
        node_id: NodeId,
        rect: GeometryRect,
        content_offset: Point,
        node_data: LayoutNodeData,
        children: Vec<LayoutBox>,
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

/// Snapshot of the data required to render a layout node.
#[derive(Debug, Clone)]
pub struct LayoutNodeData {
    /// Composable origins retained by an inspection-enabled layout node.
    #[cfg(feature = "inspection")]
    pub source_trace: Rc<[cranpose_core::source_trace::SourceLocation]>,
    pub modifier: Modifier,
    pub resolved_modifiers: ResolvedModifiers,
    pub modifier_slices: Rc<ModifierNodeSlices>,
    pub semantics: Option<Rc<SemanticsConfiguration>>,
    pub kind: LayoutNodeKind,
}

impl LayoutNodeData {
    pub fn new(
        modifier: Modifier,
        resolved_modifiers: ResolvedModifiers,
        modifier_slices: Rc<ModifierNodeSlices>,
        semantics: Option<Rc<SemanticsConfiguration>>,
        kind: LayoutNodeKind,
    ) -> Self {
        Self {
            #[cfg(feature = "inspection")]
            source_trace: Rc::default(),
            modifier,
            resolved_modifiers,
            modifier_slices,
            semantics,
            kind,
        }
    }

    /// Semantics the node's live modifier chain reported when the snapshot
    /// was taken.
    pub fn semantics(&self) -> Option<&SemanticsConfiguration> {
        self.semantics.as_deref()
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.resolved_modifiers
    }

    pub fn modifier_slices(&self) -> &ModifierNodeSlices {
        &self.modifier_slices
    }
}

/// Classification of the node captured inside a [`LayoutBox`].
///
/// Note: Text content is no longer represented as a distinct LayoutNodeKind.
/// Text nodes now use `LayoutNodeKind::Layout` with their content stored in
/// `modifier_slices.text_content()` via TextModifierNode, following Jetpack
/// Compose's pattern where text is a modifier node capability.
#[derive(Clone)]
pub enum LayoutNodeKind {
    Layout,
    Subcompose,
    Spacer,
    Button { on_click: Rc<RefCell<dyn FnMut()>> },
    Unknown,
}

impl fmt::Debug for LayoutNodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayoutNodeKind::Layout => f.write_str("Layout"),
            LayoutNodeKind::Subcompose => f.write_str("Subcompose"),
            LayoutNodeKind::Spacer => f.write_str("Spacer"),
            LayoutNodeKind::Button { .. } => f.write_str("Button"),
            LayoutNodeKind::Unknown => f.write_str("Unknown"),
        }
    }
}

/// Extension trait that equips `MemoryApplier` with layout computation.
pub trait LayoutEngine {
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
    semantics: Option<SemanticsTree>,
    layout_tree: Option<LayoutTree>,
}

impl LayoutMeasurements {
    fn new(
        root: Rc<MeasuredNode>,
        semantics: Option<SemanticsTree>,
        layout_tree: Option<LayoutTree>,
    ) -> Self {
        Self {
            root,
            semantics,
            layout_tree,
        }
    }

    /// Returns the measured size of the root node.
    pub fn root_size(&self) -> Size {
        self.root.size
    }

    pub fn semantics_tree(&self) -> Option<&SemanticsTree> {
        self.semantics.as_ref()
    }

    pub fn debug_allocation_stats(&self) -> LayoutAllocationDebugStats {
        let mut stats = self
            .layout_tree
            .as_ref()
            .map(LayoutTree::debug_allocation_stats)
            .unwrap_or_default();
        if let Some(semantics) = &self.semantics {
            record_semantics_allocation_stats(semantics.root(), &mut stats);
        }
        stats
    }

    /// Consumes the measurements and returns the built [`LayoutTree`], if requested.
    pub fn into_layout_tree(self) -> Option<LayoutTree> {
        self.layout_tree
    }

    /// Returns a cloned [`LayoutTree`] for rendering/debug consumers, if requested.
    pub fn layout_tree(&self) -> Option<LayoutTree> {
        self.layout_tree.clone()
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
fn finish_placement(applier: &mut MemoryApplier, root: NodeId) {
    if applier
        .with_node::<LayoutNode, _>(root, |node| node.set_position(Point::default()))
        .is_err()
    {
        let _ = applier.with_node::<SubcomposeLayoutNode, _>(root, |node| {
            node.set_position(Point::default());
        });
    }
    if let Some(pass) = placement_pass_with_unplaced_nodes() {
        record_unplaced_parents(applier, root, pass);
    }
}

/// Names to the scene phase the parent of every node `pass` found placed and
/// left unplaced: nothing about the node itself changed, so the geometry
/// setters had nothing to report, yet its layer has to leave the scene. A
/// pass unplaces nodes rarely, so it pays for this walk only when it did.
fn record_unplaced_parents(applier: &mut MemoryApplier, root: NodeId, pass: u64) {
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
                Ok(Some(true)) => crate::render_state::record_geometry_scene_node(parent),
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

enum LiveLayoutNode<'a> {
    Layout(&'a LayoutNode),
    Subcompose(&'a SubcomposeLayoutNode),
}

impl LiveLayoutNode<'_> {
    fn state(&self) -> LayoutState {
        match self {
            Self::Layout(node) => node.layout_state(),
            Self::Subcompose(node) => node.layout_state(),
        }
    }

    fn parent(&self) -> Option<NodeId> {
        match self {
            Self::Layout(node) => node.parent(),
            Self::Subcompose(node) => node.parent(),
        }
    }

    fn is_window_root(&self) -> bool {
        matches!(self, Self::Layout(node) if node.is_window_root())
    }

    fn with_children<R>(&self, read: impl FnOnce(&[NodeId]) -> R) -> R {
        match self {
            Self::Layout(node) => read(&node.children),
            Self::Subcompose(node) => node.with_active_children(read),
        }
    }

    fn semantics(&self) -> Option<SemanticsConfiguration> {
        match self {
            Self::Layout(node) => node.semantics_configuration(),
            Self::Subcompose(node) => node.semantics_configuration(),
        }
    }
}

fn read_live_layout_node<R>(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    mut read: impl FnMut(LiveLayoutNode<'_>) -> R,
) -> Result<Option<R>, NodeError> {
    match applier.with_node::<LayoutNode, _>(node_id, |node| read(LiveLayoutNode::Layout(node))) {
        Ok(value) => return Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => {}
        Err(err) => return Err(err),
    }
    match applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
        read(LiveLayoutNode::Subcompose(node))
    }) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

fn snapshot_node_data(
    applier: &mut MemoryApplier,
    node_id: NodeId,
    top_left: Point,
    size: Size,
    parent_transform: ProjectiveTransform,
) -> Result<(LayoutNodeData, ProjectiveTransform), NodeError> {
    let info = runtime_metadata_for(applier, node_id)?;
    let kind = layout_kind_from_metadata(node_id, &info);
    let RuntimeNodeMetadata {
        modifier,
        resolved_modifiers,
        modifier_slices,
        semantics,
        ..
    } = info;

    let window_transform = modifier_slices
        .graphics_layer()
        .map_or(parent_transform, |layer| {
            layer_transform_to_window(
                parent_transform,
                top_left,
                modifier_slices.layer_bounds(size),
                &layer,
            )
        });
    modifier_slices.publish_window_geometry(top_left, window_transform, size);

    let data = LayoutNodeData::new(
        modifier,
        resolved_modifiers,
        modifier_slices,
        semantics,
        kind,
    );
    #[cfg(feature = "inspection")]
    let data = {
        let mut data = data;
        data.source_trace = applier
            .with_node::<LayoutNode, _>(node_id, |node| node.source_trace.clone())
            .unwrap_or_default();
        data
    };
    Ok((data, window_transform))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasureLayoutOptions {
    pub collect_semantics: bool,
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

pub fn measure_layout_with_options(
    applier: &mut MemoryApplier,
    root: NodeId,
    max_size: Size,
    options: MeasureLayoutOptions,
) -> Result<LayoutMeasurements, NodeError> {
    let telemetry_start = Instant::now();
    crate::render_state::begin_text_layout_pass();
    begin_placement_pass();
    process_pending_layout_repasses(applier, root)?;
    let after_repasses = Instant::now();

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
                node.cache_handles().epoch(),
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
        crate::render_state::next_layout_cache_epoch()
    } else if cached_epoch != 0 {
        cached_epoch
    } else {
        crate::render_state::current_layout_cache_epoch()
    };

    let guard = ApplierSlotGuard::new(applier);
    let applier_host = guard.host();
    let slots_handle = guard.slots_handle();
    let after_guard = Instant::now();

    let frame_arena = crate::render_state::take_layout_frame_arena();
    let builder = LayoutBuilder::new_with_epoch(
        Rc::clone(&applier_host),
        epoch,
        Rc::clone(&slots_handle),
        frame_arena,
    );
    let after_builder = Instant::now();

    let measured = builder
        .state
        .measure_node(root, normalize_constraints(constraints))?;
    let after_measure = Instant::now();

    if let Ok(mut applier) = applier_host.try_borrow_typed() {
        finish_placement(&mut applier, root);
    }
    let after_root_place = Instant::now();

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
    let after_aux = Instant::now();

    drop(builder);
    let after_builder_drop = Instant::now();

    drop(guard);
    let after_guard_drop = Instant::now();

    log_layout_measure_telemetry(LayoutMeasureTelemetry {
        root,
        start: telemetry_start,
        after_repasses,
        after_guard,
        after_builder,
        after_measure,
        after_root_place,
        after_aux,
        after_builder_drop,
        after_guard_drop,
    });

    Ok(LayoutMeasurements::new(measured, semantics, layout_tree))
}

fn process_pending_layout_repasses(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<(), NodeError> {
    for node_id in crate::render_state::take_modifier_slice_repass_nodes() {
        if let Ok(node) = applier.get_mut(node_id) {
            let any = node.as_any_mut();
            if let Some(layout) = any.downcast_mut::<crate::widgets::nodes::LayoutNode>() {
                layout.mark_modifier_slices_dirty();
            } else if let Some(subcompose) =
                any.downcast_mut::<crate::subcompose_layout::SubcomposeLayoutNode>()
            {
                subcompose.mark_modifier_slices_dirty();
            }
        }
    }
    let measure_repass_nodes = crate::take_measure_repass_nodes();
    let repass_nodes = crate::take_layout_repass_nodes();
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

struct LayoutBuilder {
    state: Rc<LayoutBuilderState>,
}

impl LayoutBuilder {
    fn new_with_epoch(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        epoch: u64,
        slots: Rc<RefCell<SlotTable>>,
        frame_arena: FrameLayoutArena,
    ) -> Self {
        Self {
            state: Rc::new(LayoutBuilderState::new_with_epoch(
                applier,
                epoch,
                slots,
                frame_arena,
            )),
        }
    }
}

impl Drop for LayoutBuilder {
    fn drop(&mut self) {
        if Rc::strong_count(&self.state) != 1 {
            return;
        }
        let Ok(mut frame_arena) = self.state.frame_arena.try_borrow_mut() else {
            return;
        };
        crate::render_state::replace_layout_frame_arena(std::mem::take(&mut *frame_arena));
    }
}

struct LayoutBuilderState {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: RefCell<Option<RuntimeHandle>>,
    slots: Rc<RefCell<SlotTable>>,
    cache_epoch: u64,
    cache_floor: u64,
    frame_arena: RefCell<FrameLayoutArena>,
}

struct LayoutRuntimeFrameBindingCleanup<'a> {
    state: &'a RefCell<LayoutRuntimeState>,
}

impl Drop for LayoutRuntimeFrameBindingCleanup<'_> {
    fn drop(&mut self) {
        self.state.borrow().frame.unbind();
    }
}

enum LayoutNodeVisit<'a> {
    Cached(Rc<MeasuredNode>),
    /// Only nodes below this one changed: its measurement holds unless a
    /// changed child measures differently.
    Keep(Rc<MeasuredNode>),
    Measure(LayoutNodeMeasure<'a>),
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
    parent_data: Option<cranpose_ui_layout::ParentData>,
    /// The constraints the child's own cache holds.
    cached_constraints: Option<Constraints>,
    read_intrinsics: bool,
    placed: bool,
}

impl ChildView {
    /// The child, when it or a node below it changed.
    fn of(
        node: &dyn Node,
        cache: &LayoutNodeCacheHandles,
        props: &dyn Fn() -> crate::modifier::LayoutProperties,
        placed: bool,
    ) -> Option<Self> {
        let self_dirty = node.needs_measure() || node.needs_layout();
        if !self_dirty && !node.descendant_needs_layout() {
            return None;
        }
        Some(Self {
            self_dirty,
            parent_data: self_dirty.then(|| parent_data_of(props())),
            cached_constraints: cache.measured_constraints(),
            read_intrinsics: cache.has_intrinsics(),
            placed,
        })
    }
}

struct LayoutNodeMeasure<'a> {
    runtime_state: Rc<RefCell<LayoutRuntimeState>>,
    chain: ModifierChainInputs,
    pools: VecPools<'a>,
}

impl LayoutBuilderState {
    fn new_with_epoch(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        epoch: u64,
        slots: Rc<RefCell<SlotTable>>,
        frame_arena: FrameLayoutArena,
    ) -> Self {
        let runtime_handle = applier.borrow_typed().runtime_handle();

        Self {
            applier,
            runtime_handle: RefCell::new(runtime_handle),
            slots,
            cache_epoch: epoch,
            cache_floor: crate::render_state::layout_cache_floor(),
            frame_arena: RefCell::new(frame_arena),
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
                let cache = node.cache_handles();
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
            |child_id| self.subcompose_child_change(child_id),
            |child_id| self.mark_measured_without_policy(child_id),
        )?;
        let Some(kept) = kept else {
            return Ok(None);
        };
        self.with_applier_result(|applier| {
            applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.cache_handles()
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
            match read_layout_child(&mut applier, child_id, ChildView::of)? {
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
            node.clear_placed();
        });
    }

    fn measure_node(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let telemetry = layout_measure_telemetry_threshold_ms().map(|ms| (ms, Instant::now()));
        let (kind, measured) =
            if let Some(measured) = self.measure_layout_node(node_id, constraints)? {
                ("layout", measured)
            } else {
                self.measure_subcompose_node(node_id, constraints)?
            };
        if let Some(telemetry) = telemetry {
            log_node_measure_telemetry(kind, node_id, constraints, &measured, telemetry);
        }
        Ok(measured)
    }

    /// The measurement `node_id` cached for `constraints` in layout cache
    /// epoch `current_epoch`, the one the app context holds.
    fn cached_measure_node_with_applier(
        applier: &mut MemoryApplier,
        node_id: NodeId,
        constraints: Constraints,
        current_epoch: u64,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let served = |cache: &LayoutNodeCacheHandles, dirty: bool| {
            let epoch = cache.epoch();
            if dirty || epoch == 0 || epoch != current_epoch {
                return None;
            }
            cache.get_measurement(constraints)
        };

        match applier.with_node::<LayoutNode, _>(node_id, |node| {
            let measured = served(node.cache_handles(), Node::layout_dirty(node))?;
            node.set_measured_size(measured.size);
            Some(measured)
        }) {
            Ok(measured) => Ok(measured),
            Err(NodeError::TypeMismatch { .. }) => {
                match applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                    let measured = served(node.cache_handles(), Node::layout_dirty(node))?;
                    node.set_measured_size(measured.size);
                    Some(measured)
                }) {
                    Ok(measured) => Ok(measured),
                    Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(None),
                    Err(err) => Err(err),
                }
            }
            Err(NodeError::Missing { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn try_measure_subcompose(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let (node_handle, resolved_modifiers) = {
            let Ok(mut applier) = self.applier.try_borrow_typed() else {
                return Ok(None);
            };
            let node = match applier.get_mut(node_id) {
                Ok(node) => node,
                Err(NodeError::Missing { .. }) => return Ok(None),
                Err(err) => return Err(err),
            };
            let any = node.as_any_mut();
            if let Some(subcompose) =
                any.downcast_mut::<crate::subcompose_layout::SubcomposeLayoutNode>()
            {
                let handle = subcompose.handle();
                let resolved_modifiers = handle
                    .resolved_modifiers()
                    .on_device_grid(subcompose.density().density());
                (handle, resolved_modifiers)
            } else {
                return Ok(None);
            }
        };

        let runtime_handle = {
            let mut runtime_handle = self.runtime_handle.borrow_mut();
            if runtime_handle.is_none()
                && let Ok(applier) = self.applier.try_borrow_typed()
            {
                *runtime_handle = applier.runtime_handle();
            }
            runtime_handle.clone().ok_or(NodeError::MissingContext {
                id: node_id,
                reason: "runtime handle required for subcomposition",
            })?
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

        let mut slots_guard = SlotsGuard::take(&self.slots);
        let slots_host = slots_guard.host();
        let applier_host_dyn: Rc<dyn ApplierHost> = Rc::clone(&self.applier) as Rc<dyn ApplierHost>;
        let composer = Composer::new(
            Rc::clone(&slots_host),
            applier_host_dyn,
            runtime_handle,
            Some(node_id),
        );
        composer.enter_phase(Phase::Measure);

        let measure_error = RefCell::new(None);
        let measured_children = node_handle.measured_children_scratch();

        let measure_result = node_handle.measure_with_cached_batch(
            &composer,
            node_id,
            inner_constraints,
            CachedBatchMeasureInputs {
                measurer: Box::new(|child_id: NodeId, child_constraints: Constraints| {
                    match self.measure_node(child_id, child_constraints) {
                        Ok(measured) => {
                            measured_children
                                .borrow_mut()
                                .insert(child_id, Rc::clone(&measured));
                            let size = measured.size_for_parent();
                            Placeable::value(size.width, size.height, child_id)
                                .with_alignment_lines(measured.alignment_lines_for_parent())
                        }
                        Err(err) => {
                            let mut slot = measure_error.borrow_mut();
                            if slot.is_none() {
                                *slot = Some(err);
                            }
                            Placeable::value(0.0, 0.0, child_id)
                        }
                    }
                }),
                cached_measure_batch_registrar: Box::new(
                    |child_ids: &[NodeId],
                     child_constraints: Constraints,
                     out: &mut Vec<Option<Size>>| {
                        out.clear();
                        out.resize(child_ids.len(), None);

                        let Ok(mut applier) = self.applier.try_borrow_typed() else {
                            return;
                        };

                        let mut measured_children = measured_children.borrow_mut();
                        let current_epoch = crate::render_state::current_layout_cache_epoch();
                        for (index, &child_id) in child_ids.iter().enumerate() {
                            match Self::cached_measure_node_with_applier(
                                &mut applier,
                                child_id,
                                child_constraints,
                                current_epoch,
                            ) {
                                Ok(Some(measured)) => {
                                    out[index] = Some(measured.size);
                                    measured_children.insert(child_id, Rc::clone(&measured));
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    let mut slot = measure_error.borrow_mut();
                                    if slot.is_none() {
                                        *slot = Some(err);
                                    }
                                    break;
                                }
                            }
                        }
                    },
                ),
                retained_measure_lookup: Box::new(|child_id| {
                    measured_children.borrow().get(&child_id).cloned()
                }),
                retained_measure_registrar: Box::new(|measurements| {
                    let mut measured_children = measured_children.borrow_mut();
                    for measured in measurements {
                        measured_children.insert(measured.node_id(), Rc::clone(measured));
                    }
                }),
                error: &measure_error,
            },
        )?;
        drop(composer);
        slots_guard.restore(slots_host.into_table()?);

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

        let mut children = Vec::with_capacity(placements.len());
        let mut alignment_lines = AlignmentLines::default();
        let mut measured_children_by_id = measured_children.borrow_mut();

        if let Ok(mut applier) = self.applier.try_borrow_typed() {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |parent_node| {
                parent_node.set_measured_size(Size { width, height });
                parent_node.clear_needs_measure();
                parent_node.clear_needs_layout();
            });
        }

        for placement in &placements {
            let child = if let Some(measured) = measured_children_by_id.remove(&placement.node_id) {
                measured
            } else {
                self.measure_node(placement.node_id, inner_constraints)?
            };
            let policy_position = Point {
                x: padding.left + placement.x,
                y: padding.top + placement.y,
            };
            let retained_position = Point {
                x: policy_position.x + child.offset.x,
                y: policy_position.y + child.offset.y,
            };
            alignment_lines.merge(
                child
                    .alignment_lines_for_parent()
                    .translated(policy_position.y),
            );

            if let Ok(mut applier) = self.applier.try_borrow_typed()
                && applier
                    .with_node::<LayoutNode, _>(placement.node_id, |node| {
                        node.set_position(retained_position);
                    })
                    .is_err()
            {
                let _ = applier.with_node::<SubcomposeLayoutNode, _>(placement.node_id, |node| {
                    node.set_position(retained_position);
                });
            }

            children.push(MeasuredChild {
                node: RefCell::new(child),
                offset: policy_position,
            });
        }

        node_handle.set_active_children(children.iter().map(|c| c.node.borrow().node_id));
        node_handle.recycle_placement_scratch(placements);

        Ok(Some(Rc::new(
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
            ),
        )))
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
                Ok(LayoutNodeVisit::Keep(cached)) => {
                    drop(applier);
                    if let Some(kept) = self.keep_measurement(node_id, constraints, &cached)? {
                        return Ok(Some(kept));
                    }
                    keep = false;
                }
                Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => return Ok(None),
                Err(err) => return Err(err),
            }
        };
        let LayoutNodeMeasure {
            runtime_state,
            chain,
            mut pools,
        } = measure;

        let _frame_binding_cleanup = LayoutRuntimeFrameBindingCleanup {
            state: &runtime_state,
        };
        self.bind_layout_children(
            &mut applier,
            &runtime_state,
            &pools.child_ids,
            ChildPass::Measure,
        )?;
        drop(applier);
        pools.child_ids.clear();

        let runtime_state = runtime_state.borrow();
        let measurement = self.measure_through_modifier_chain(
            node_id,
            &runtime_state,
            chain,
            constraints,
            &mut pools.placements,
            &mut pools.child_ids,
        );

        if let Some(err) = runtime_state.frame.error.borrow_mut().take() {
            for child_state in &runtime_state.child_states {
                child_state.measured.borrow_mut().take();
            }
            self.with_applier_result(|applier| {
                applier.with_node::<LayoutNode, _>(node_id, |node| {
                    runtime_state.write_node_geometry(node_id, node, &measurement);
                })
            })
            .ok();
            return Err(err);
        }

        let measured = Rc::new(
            MeasuredNode::new(
                node_id,
                measurement.size,
                measurement.offset,
                measurement.content_offset,
                runtime_state.measured_children(
                    &pools.placements,
                    &pools.child_ids,
                    measurement.content_offset,
                ),
            )
            .with_alignment_lines(measurement.alignment_lines)
            .with_window_root(measurement.window_root),
        );

        self.with_applier_result(|applier| {
            applier.with_node::<LayoutNode, _>(node_id, |node| {
                node.cache_handles()
                    .store_measurement(constraints, Rc::clone(&measured));
                runtime_state.write_node_geometry(node_id, node, &measurement);
                node.clear_needs_measure();
                node.clear_needs_layout();
                node.set_measured_size(measurement.size);
                node.set_content_offset(measurement.content_offset);
            })
        })
        .ok();

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
            let runtime_state = node.layout_runtime_state_handle();
            let chain = runtime_state.borrow_mut().bind_node(node);
            pools.child_ids.extend_from_slice(&node.children);
            (runtime_state, chain)
        });
        let (runtime_state, chain) = match bound {
            Ok(bound) => bound,
            Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => return Ok(None),
            Err(err) => return Err(err),
        };
        let _frame_binding_cleanup = LayoutRuntimeFrameBindingCleanup {
            state: &runtime_state,
        };
        self.bind_layout_children(
            &mut applier,
            &runtime_state,
            &pools.child_ids,
            ChildPass::Intrinsics,
        )?;
        drop(applier);
        pools.child_ids.clear();

        let runtime_state = runtime_state.borrow();
        let value = if chain.uses_chain {
            let scope = crate::density::DensityMeasureScope::new(chain.density);
            let frame = CoordinatorFrame::new(
                &runtime_state.measure_policy,
                &scope,
                runtime_state.child_measurables.as_slice(),
                &runtime_state.child_states,
                &mut pools.placements,
                &mut pools.child_ids,
            );
            runtime_state
                .coordinator_chain
                .intrinsic_from(0, &frame, kind)
        } else {
            policy_intrinsic(
                runtime_state.measure_policy.as_ref(),
                runtime_state.child_measurables.as_slice(),
                kind,
            )
        };
        match runtime_state.frame.error.borrow_mut().take() {
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
        let cache = node.cache_handles();
        if cache.epoch() < self.cache_floor {
            return None;
        }
        cache.get_measurement(constraints)
    }

    /// Measures again the children of `node_id` that changed, at the
    /// constraints they last had, and keeps the node's measurement when each
    /// of them measures the same for its parent: the node's policy would
    /// place them as before. `None` when one of them changed, and the node
    /// has to measure again.
    fn keep_measurement(
        self: &Rc<Self>,
        node_id: NodeId,
        constraints: Constraints,
        cached: &Rc<MeasuredNode>,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let runtime_state = self.with_applier_result(|applier| {
            applier.with_node::<LayoutNode, _>(node_id, |node| node.layout_runtime_state_handle())
        })?;
        let mut next_state = 0;
        let kept = self.remeasure_changed_children(
            cached,
            |child_id| self.layout_child_change(&runtime_state, &mut next_state, child_id),
            |_| {},
        )?;
        let Some(kept) = kept else {
            return Ok(None);
        };
        self.with_applier_result(|applier| {
            applier.with_node::<LayoutNode, _>(node_id, |node| {
                node.cache_handles()
                    .store_measurement(constraints, Rc::clone(&kept));
                node.clear_needs_layout();
            })
        })?;
        Ok(Some(kept))
    }

    /// Walks the children of a kept measurement, measures again each one
    /// `change` finds changed, at the constraints it last had, and keeps the
    /// measurement with their new measurements swapped in. `None` as soon as
    /// `change` refuses a child or one measures differently for its parent;
    /// the measurement is then left as it was.
    fn remeasure_changed_children(
        self: &Rc<Self>,
        cached: &Rc<MeasuredNode>,
        mut change: impl FnMut(NodeId) -> Result<ChildChange, NodeError>,
        mut remeasured: impl FnMut(NodeId),
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let mut replaced = smallvec::SmallVec::<[(usize, Rc<MeasuredNode>); 4]>::new();
        for (index, previous) in cached.children.iter().enumerate() {
            let previous = Rc::clone(&previous.node.borrow());
            let child_id = previous.node_id();
            let child = match change(child_id)? {
                ChildChange::Unchanged => continue,
                ChildChange::Refuse => return Ok(None),
                ChildChange::Changed(child) => child,
            };
            let measured = self.measure_node(child_id, child.constraints)?;
            remeasured(child_id);
            if !measures_the_same_for_parent(&previous, &measured) {
                return Ok(None);
            }
            if child.placed {
                self.place_where_it_was(child_id);
            }
            if !Rc::ptr_eq(&previous, &measured) {
                replaced.push((index, measured));
            }
        }
        for (index, measured) in replaced {
            if let Some(child) = cached.children.get(index) {
                *child.node.borrow_mut() = measured;
            }
        }
        Ok(Some(Rc::clone(cached)))
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
            .child_states
            .get(*next_state..)
            .unwrap_or_default()
            .iter()
            .enumerate()
            .find(|(_, state)| state.node_id == child_id)
        else {
            return Ok(ChildChange::Refuse);
        };
        *next_state += offset + 1;
        self.child_change(child_id, |view| {
            let parent_data_changed = view
                .parent_data
                .is_some_and(|data| Some(data) != state.parent_data.get());
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
            node.cache_handles()
                .store_measurement(constraints, Rc::clone(measured));
        });
    }

    /// Places a child of a kept node where it was: the node's policy would
    /// place it there again.
    fn place_where_it_was(&self, child_id: NodeId) {
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return;
        };
        if applier
            .with_node::<LayoutNode, _>(child_id, |child| child.set_position(child.position()))
            .is_err()
        {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(child_id, |child| {
                child.set_position(child.layout_state().position());
            });
        }
    }

    fn visit_layout_node(
        &self,
        node: &mut LayoutNode,
        constraints: Constraints,
        keep: bool,
    ) -> LayoutNodeVisit<'_> {
        node.clear_placed();
        if keep && let Some(cached) = self.measurement_to_keep(node, constraints) {
            return LayoutNodeVisit::Keep(cached);
        }
        let cache = node.cache_handles();
        cache.activate(self.cache_epoch);
        if !Node::layout_dirty(node)
            && let Some(cached) = cache.get_measurement(constraints)
        {
            node.clear_needs_measure();
            node.clear_needs_layout();
            return LayoutNodeVisit::Cached(cached);
        }

        let runtime_state = node.layout_runtime_state_handle();
        let chain = runtime_state.borrow_mut().bind_node(node);
        let mut pools = VecPools::acquire(&self.frame_arena);
        pools.child_ids.extend_from_slice(&node.children);
        LayoutNodeVisit::Measure(LayoutNodeMeasure {
            runtime_state,
            chain,
            pools,
        })
    }

    fn bind_layout_children(
        self: &Rc<Self>,
        applier: &mut MemoryApplier,
        runtime_state: &RefCell<LayoutRuntimeState>,
        child_ids: &[NodeId],
        pass: ChildPass,
    ) -> Result<(), NodeError> {
        let mut runtime_state = runtime_state.borrow_mut();
        runtime_state.frame.bind(self, pass);
        let mut bound = 0;
        for &child_id in child_ids {
            if self.bind_layout_child(applier, &mut runtime_state, bound, child_id)? {
                bound += 1;
            }
        }
        runtime_state.truncate_children(bound);
        Ok(())
    }

    fn bind_layout_child(
        &self,
        applier: &mut MemoryApplier,
        runtime_state: &mut LayoutRuntimeState,
        position: usize,
        child_id: NodeId,
    ) -> Result<bool, NodeError> {
        let bound = applier.with_node::<LayoutNode, _>(child_id, |child| {
            runtime_state.child_state_at(position, child_id).bind(
                LayoutChildBinding {
                    cache: child.cache_handles(),
                    layout_state: Some(child.layout_state_handle()),
                    parent_data: Some(parent_data_of(
                        child.resolved_modifiers().layout_properties(),
                    )),
                    dirty: child.needs_layout() || child.needs_measure(),
                    descendant_dirty: Node::descendant_needs_layout(child),
                },
                self,
            );
        });
        match bound {
            Ok(()) => Ok(true),
            Err(NodeError::TypeMismatch { .. }) => {
                match applier.with_node::<SubcomposeLayoutNode, _>(child_id, |child| {
                    runtime_state.child_state_at(position, child_id).bind(
                        LayoutChildBinding {
                            cache: child.cache_handles(),
                            layout_state: None,
                            parent_data: Some(parent_data_of(
                                child.resolved_modifiers().layout_properties(),
                            )),
                            dirty: child.needs_layout() || child.needs_measure(),
                            descendant_dirty: Node::descendant_needs_layout(child),
                        },
                        self,
                    );
                }) {
                    Ok(()) => Ok(true),
                    Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(false),
                    Err(err) => Err(err),
                }
            }
            Err(NodeError::Missing { .. }) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn measure_through_modifier_chain(
        &self,
        node_id: NodeId,
        runtime_state: &LayoutRuntimeState,
        chain: ModifierChainInputs,
        constraints: Constraints,
        placements: &mut Vec<Placement>,
        placement_indices: &mut Vec<usize>,
    ) -> ModifierChainMeasurement {
        let scope = crate::density::DensityMeasureScope::new(chain.density);

        if !chain.uses_chain {
            let measurement = runtime_state.measure_policy.measure_into(
                &scope,
                runtime_state.child_measurables.as_slice(),
                constraints,
                placements,
            );
            return ModifierChainMeasurement {
                size: measurement.size,
                alignment_lines: inherited_alignment_lines(
                    &runtime_state.child_states,
                    placements,
                    placement_indices,
                )
                .with_overrides(measurement.alignment_lines),
                content_offset: Point::default(),
                offset: chain.offset,
                window_root: chain.window_root,
                uses_chain: false,
            };
        }

        let frame = CoordinatorFrame::new(
            &runtime_state.measure_policy,
            &scope,
            runtime_state.child_measurables.as_slice(),
            &runtime_state.child_states,
            placements,
            placement_indices,
        );
        let placeable = runtime_state
            .coordinator_chain
            .measure_from(0, &frame, constraints);
        let (content_x, content_y) = placeable.content_offset();

        let invalidations = frame.take_invalidations();
        if !invalidations.is_empty() {
            self.with_applier_result(|applier| {
                applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                    for kind in invalidations {
                        match kind {
                            InvalidationKind::Layout => layout_node.mark_needs_measure(),
                            InvalidationKind::Draw => layout_node.mark_needs_redraw(),
                            InvalidationKind::Semantics => layout_node.mark_needs_semantics(),
                            InvalidationKind::PointerInput => layout_node.mark_needs_pointer_pass(),
                            InvalidationKind::Focus => layout_node.mark_needs_focus_sync(),
                        }
                    }
                })
            })
            .ok();
        }

        ModifierChainMeasurement {
            alignment_lines: placeable.alignment_lines(),
            size: Size {
                width: placeable.width(),
                height: placeable.height(),
            },
            content_offset: Point {
                x: content_x - chain.offset.x,
                y: content_y - chain.offset.y,
            },
            offset: chain.offset,
            window_root: chain.window_root,
            uses_chain: true,
        }
    }
}

struct VecPools<'a> {
    arena: &'a RefCell<FrameLayoutArena>,
    child_ids: Vec<NodeId>,
    placements: Vec<Placement>,
}

impl<'a> VecPools<'a> {
    fn acquire(arena: &'a RefCell<FrameLayoutArena>) -> Self {
        let mut pools = arena.borrow_mut();
        let child_ids = pools.tmp_child_ids.acquire();
        let placements = pools.tmp_placements.acquire();
        Self {
            arena,
            child_ids,
            placements,
        }
    }
}

impl Drop for VecPools<'_> {
    fn drop(&mut self) {
        let mut pools = self.arena.borrow_mut();
        pools
            .tmp_child_ids
            .release(std::mem::take(&mut self.child_ids));
        pools
            .tmp_placements
            .release(std::mem::take(&mut self.placements));
    }
}

struct SlotsGuard<'a> {
    table: &'a RefCell<SlotTable>,
    slots: Option<SlotTable>,
}

impl<'a> SlotsGuard<'a> {
    fn take(table: &'a RefCell<SlotTable>) -> Self {
        let slots = std::mem::take(&mut *table.borrow_mut());
        Self {
            table,
            slots: Some(slots),
        }
    }

    fn host(&mut self) -> Rc<SlotsHost> {
        let slots = self.slots.take().unwrap_or_default();
        Rc::new(SlotsHost::new(slots))
    }

    fn restore(&mut self, slots: SlotTable) {
        debug_assert!(self.slots.is_none());
        self.slots = Some(slots);
    }
}

impl Drop for SlotsGuard<'_> {
    fn drop(&mut self) {
        if let Some(slots) = self.slots.take() {
            *self.table.borrow_mut() = slots;
        }
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

    #[cfg(test)]
    pub(crate) fn leaf(node_id: NodeId, size: Size) -> Self {
        Self::new(
            node_id,
            size,
            Point::default(),
            Point::default(),
            Vec::new(),
        )
    }

    pub(crate) fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub(crate) fn size(&self) -> Size {
        self.size
    }
}

#[derive(Debug, Clone)]
struct MeasuredChild {
    /// The child's measurement. A kept parent swaps in the new measurement
    /// of a child that measures the same for it.
    node: RefCell<Rc<MeasuredNode>>,
    offset: Point,
}

/// The intrinsic size `kind` names, of a measurable.
fn measurable_intrinsic(measurable: &dyn Measurable, kind: IntrinsicKind) -> f32 {
    match kind {
        IntrinsicKind::MinWidth(height) => measurable.min_intrinsic_width(height),
        IntrinsicKind::MaxWidth(height) => measurable.max_intrinsic_width(height),
        IntrinsicKind::MinHeight(width) => measurable.min_intrinsic_height(width),
        IntrinsicKind::MaxHeight(width) => measurable.max_intrinsic_height(width),
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

/// The intrinsic size `kind` names, of a layout modifier around `wrapped`.
fn layout_modifier_intrinsic(
    node: &dyn cranpose_foundation::LayoutModifierNode,
    wrapped: &dyn Measurable,
    kind: IntrinsicKind,
    density: f32,
) -> f32 {
    match kind {
        IntrinsicKind::MinWidth(height) => node.min_intrinsic_width(wrapped, height, density),
        IntrinsicKind::MaxWidth(height) => node.max_intrinsic_width(wrapped, height, density),
        IntrinsicKind::MinHeight(width) => node.min_intrinsic_height(wrapped, width, density),
        IntrinsicKind::MaxHeight(width) => node.max_intrinsic_height(wrapped, width, density),
    }
}

struct CoordinatorFrame<'a> {
    measure_policy: &'a Rc<dyn MeasurePolicy>,
    scope: &'a dyn cranpose_ui_layout::MeasureScope,
    measurables: &'a [Box<dyn Measurable>],
    child_states: &'a [Rc<LayoutChildMeasureState>],
    placements: RefCell<&'a mut Vec<Placement>>,
    placement_indices: RefCell<&'a mut Vec<usize>>,
    context: RefCell<LayoutNodeContext>,
}

impl<'a> CoordinatorFrame<'a> {
    fn new(
        measure_policy: &'a Rc<dyn MeasurePolicy>,
        scope: &'a dyn cranpose_ui_layout::MeasureScope,
        measurables: &'a [Box<dyn Measurable>],
        child_states: &'a [Rc<LayoutChildMeasureState>],
        placements: &'a mut Vec<Placement>,
        placement_indices: &'a mut Vec<usize>,
    ) -> Self {
        Self {
            measure_policy,
            scope,
            measurables,
            child_states,
            placements: RefCell::new(placements),
            placement_indices: RefCell::new(placement_indices),
            context: RefCell::new(LayoutNodeContext::new(scope.density())),
        }
    }

    fn take_invalidations(&self) -> Vec<InvalidationKind> {
        self.context.borrow_mut().take_invalidations()
    }
}

struct CoordinatorLink<'chain, 'frame_ref, 'frame_data> {
    chain: &'chain CoordinatorChain,
    frame: &'frame_ref CoordinatorFrame<'frame_data>,
    index: usize,
    alignment_lines: Cell<AlignmentLines>,
}

impl<'chain, 'frame_ref, 'frame_data> CoordinatorLink<'chain, 'frame_ref, 'frame_data> {
    fn new(
        chain: &'chain CoordinatorChain,
        frame: &'frame_ref CoordinatorFrame<'frame_data>,
        index: usize,
    ) -> Self {
        Self {
            chain,
            frame,
            index,
            alignment_lines: Cell::default(),
        }
    }
}

impl Measurable for CoordinatorLink<'_, '_, '_> {
    fn measure(&self, constraints: Constraints) -> Placeable {
        let placeable = self.chain.measure_from(self.index, self.frame, constraints);
        self.alignment_lines.set(placeable.alignment_lines());
        placeable
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MinWidth(height))
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MaxWidth(height))
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MinHeight(width))
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.chain
            .intrinsic_from(self.index, self.frame, IntrinsicKind::MaxHeight(width))
    }
}

struct CoordinatorNode {
    modifier_index: usize,
    node: Rc<RefCell<dyn cranpose_foundation::ModifierNode>>,
    measured_size: Cell<Size>,
    accumulated_offset: Cell<Point>,
}

impl CoordinatorNode {
    fn new(
        modifier_index: usize,
        node: Rc<RefCell<dyn cranpose_foundation::ModifierNode>>,
    ) -> Self {
        Self {
            modifier_index,
            node,
            measured_size: Cell::new(Size::default()),
            accumulated_offset: Cell::new(Point::default()),
        }
    }

    fn matches(
        &self,
        modifier_index: usize,
        node: &Rc<RefCell<dyn cranpose_foundation::ModifierNode>>,
    ) -> bool {
        self.modifier_index == modifier_index && Rc::ptr_eq(&self.node, node)
    }

    #[cfg(test)]
    fn ptr(&self) -> usize {
        Rc::as_ptr(&self.node) as *const () as usize
    }
}

#[derive(Default)]
struct CoordinatorChain {
    nodes: Vec<CoordinatorNode>,
    /// The size the node's own measure policy measured its content at.
    inner_size: Cell<Size>,
    /// The chain revision the nodes were last synced at and the inputs it
    /// gave: a node whose modifiers did not change since measures again
    /// without walking its chain.
    synced: Option<(u64, ModifierChainInputs)>,
}

impl CoordinatorChain {
    fn sync(&mut self, node: &LayoutNode) -> ModifierChainInputs {
        let density = node.density();
        let revision = node.modifier_chain().revision();
        if let Some((synced, inputs)) = self.synced
            && synced == revision
            && inputs.density == density
        {
            return inputs;
        }
        let inputs = self.walk(node, density);
        self.synced = Some((revision, inputs));
        inputs
    }

    /// Syncs the nodes with the layout nodes of `node`'s chain and sums
    /// its offsets.
    fn walk(&mut self, node: &LayoutNode, density: crate::density::Density) -> ModifierChainInputs {
        let mut inputs = ModifierChainInputs {
            density,
            window_root: node.is_window_root(),
            offset: Point::default(),
            uses_chain: false,
        };
        let chain_handle = node.modifier_chain();
        if !chain_handle.has_layout_nodes() {
            return inputs;
        }

        let chain = chain_handle.chain();
        let mut len = 0;
        let mut matches = true;
        chain.for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
            let Some(index) = node_ref.entry_index() else {
                return;
            };
            if let Some(node) = chain.get_node_rc(index) {
                matches = matches
                    && self
                        .nodes
                        .get(len)
                        .is_some_and(|candidate| candidate.matches(index, node));
                len += 1;
            }
            node_ref.with_node(|node| {
                if let Some(offset_node) = node
                    .as_any()
                    .downcast_ref::<crate::modifier_nodes::OffsetNode>()
                {
                    let delta = offset_node.device_offset(density.density());
                    inputs.offset.x += delta.x;
                    inputs.offset.y += delta.y;
                }
            });
        });

        inputs.uses_chain = len > 0;
        if inputs.uses_chain && !(matches && len == self.nodes.len()) {
            self.rebuild(chain, len);
        }
        inputs
    }

    fn rebuild(&mut self, chain: &cranpose_foundation::ModifierNodeChain, len: usize) {
        let mut previous_nodes = std::mem::take(&mut self.nodes);
        self.nodes.reserve(len);
        chain.for_each_forward_matching(NodeCapabilities::LAYOUT, |node_ref| {
            let Some((index, node)) = node_ref
                .entry_index()
                .and_then(|index| chain.get_node_rc(index).map(|node| (index, node)))
            else {
                return;
            };
            match previous_nodes
                .iter()
                .position(|candidate| candidate.matches(index, node))
            {
                Some(position) => self.nodes.push(previous_nodes.swap_remove(position)),
                None => self
                    .nodes
                    .push(CoordinatorNode::new(index, Rc::clone(node))),
            }
        });
    }

    fn measure_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        constraints: Constraints,
    ) -> Placeable {
        let Some(node) = self.nodes.get(index) else {
            let mut placements = frame.placements.borrow_mut();
            let measurement = frame.measure_policy.measure_into(
                frame.scope,
                frame.measurables,
                constraints,
                &mut placements,
            );
            self.inner_size.set(measurement.size);
            return Placeable::value(
                measurement.size.width,
                measurement.size.height,
                NodeId::default(),
            )
            .with_alignment_lines(
                inherited_alignment_lines(
                    frame.child_states,
                    &placements,
                    &mut frame.placement_indices.borrow_mut(),
                )
                .with_overrides(measurement.alignment_lines),
            );
        };

        let wrapped = CoordinatorLink::new(self, frame, index + 1);
        let node_borrow = node.node.borrow();

        let Some(layout_node) = node_borrow.as_layout_node() else {
            let placeable = wrapped.measure(constraints);
            node.measured_size.set(Size {
                width: placeable.width(),
                height: placeable.height(),
            });
            let child_accumulated = self.total_content_offset_from(index + 1);
            node.accumulated_offset.set(child_accumulated);
            return placeable;
        };

        let result = match frame.context.try_borrow_mut() {
            Ok(mut context) => layout_node.measure(&mut *context, &wrapped, constraints),
            Err(_) => {
                let mut temp = LayoutNodeContext::new(frame.scope.density());
                let result = layout_node.measure(&mut temp, &wrapped, constraints);
                if let Ok(mut context) = frame.context.try_borrow_mut() {
                    for kind in temp.take_invalidations() {
                        context.invalidate(kind);
                    }
                }
                result
            }
        };

        node.measured_size.set(result.size);
        let local_offset = Point {
            x: result.placement_offset_x,
            y: result.placement_offset_y,
        };
        let child_accumulated = self.total_content_offset_from(index + 1);
        let accumulated = Point {
            x: local_offset.x + child_accumulated.x,
            y: local_offset.y + child_accumulated.y,
        };
        node.accumulated_offset.set(accumulated);

        Placeable::value_with_offset(
            result.size.width,
            result.size.height,
            NodeId::default(),
            (accumulated.x, accumulated.y),
        )
        .with_alignment_lines(
            wrapped
                .alignment_lines
                .get()
                .translated(local_offset.y)
                .with_overrides(result.alignment_lines),
        )
    }

    fn intrinsic_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        kind: IntrinsicKind,
    ) -> f32 {
        let Some(node) = self.nodes.get(index) else {
            return policy_intrinsic(frame.measure_policy.as_ref(), frame.measurables, kind);
        };
        let wrapped = CoordinatorLink::new(self, frame, index + 1);
        let node_borrow = node.node.borrow();
        node_borrow.as_layout_node().map_or_else(
            || measurable_intrinsic(&wrapped, kind),
            |layout_node| {
                layout_modifier_intrinsic(layout_node, &wrapped, kind, frame.scope.density())
            },
        )
    }

    /// Writes where each coordinator and the node's content ended up in the
    /// last measure, relative to the node drawn at its `node_offset`, and
    /// says whether any of them moved.
    fn write_geometry(
        &self,
        geometry: &crate::modifier::CoordinatorGeometry,
        node_offset: Point,
    ) -> bool {
        let content = self.total_content_offset_from(0);
        let placed = |inner_offset: Point, size: Size| GeometryRect {
            x: content.x - inner_offset.x - node_offset.x,
            y: content.y - inner_offset.y - node_offset.y,
            width: size.width,
            height: size.height,
        };
        geometry.replace(
            self.nodes
                .iter()
                .map(|node| placed(node.accumulated_offset.get(), node.measured_size.get()))
                .chain(std::iter::once(placed(
                    Point::default(),
                    self.inner_size.get(),
                ))),
        )
    }

    fn total_content_offset_from(&self, index: usize) -> Point {
        self.nodes
            .get(index)
            .map(|node| node.accumulated_offset.get())
            .unwrap_or_default()
    }

    #[cfg(test)]
    fn debug_ptrs(&self) -> Vec<usize> {
        self.nodes.iter().map(CoordinatorNode::ptr).collect()
    }
}

fn inherited_alignment_lines(
    child_states: &[Rc<LayoutChildMeasureState>],
    placements: &[Placement],
    placement_indices: &mut Vec<usize>,
) -> AlignmentLines {
    placement_indices.clear();
    let in_child_order = placements.len() == child_states.len()
        && placements
            .iter()
            .zip(child_states)
            .all(|(placement, child)| placement.node_id == child.node_id);
    if !in_child_order {
        placement_indices.extend(0..placements.len());
        placement_indices.sort_unstable_by_key(|&index| (placements[index].node_id, index));
    }
    let mut lines = AlignmentLines::default();
    for (index, child) in child_states.iter().enumerate() {
        let placement = placement_for_child(placements, placement_indices, index, child.node_id);
        if let Some(measured) = child.measured.borrow().as_ref() {
            lines.merge(
                measured
                    .alignment_lines_for_parent()
                    .translated(child.placement_position(placement).y),
            );
        }
    }
    lines
}

fn placement_for_child<'a>(
    placements: &'a [Placement],
    indices: &[usize],
    child_index: usize,
    node_id: NodeId,
) -> Option<&'a Placement> {
    let index = if indices.is_empty() {
        child_index
    } else {
        let first = indices.partition_point(|&index| placements[index].node_id < node_id);
        *indices.get(first)?
    };
    placements
        .get(index)
        .filter(|placement| placement.node_id == node_id)
}

pub(crate) struct LayoutRuntimeState {
    child_ids: Vec<NodeId>,
    child_states: Vec<Rc<LayoutChildMeasureState>>,
    child_measurables: Vec<Box<dyn Measurable>>,
    coordinator_chain: CoordinatorChain,
    measure_policy: Rc<dyn MeasurePolicy>,
    frame: Rc<LayoutChildFrame>,
}

impl LayoutRuntimeState {
    pub(crate) fn new(measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        Self {
            child_ids: Vec::new(),
            child_states: Vec::new(),
            child_measurables: Vec::new(),
            coordinator_chain: CoordinatorChain::default(),
            measure_policy,
            frame: Rc::default(),
        }
    }

    fn bind_node(&mut self, node: &LayoutNode) -> ModifierChainInputs {
        if !Rc::ptr_eq(&self.measure_policy, &node.measure_policy) {
            self.measure_policy = Rc::clone(&node.measure_policy);
        }
        self.coordinator_chain.sync(node)
    }

    fn child_state_at(&mut self, position: usize, child_id: NodeId) -> &LayoutChildMeasureState {
        if self.child_ids.get(position) != Some(&child_id) {
            let from = match self.child_ids[position..]
                .iter()
                .position(|&id| id == child_id)
            {
                Some(offset) => position + offset,
                None => {
                    let state = LayoutChildMeasureState::new(child_id, Rc::clone(&self.frame));
                    self.child_ids.push(child_id);
                    self.child_states.push(Rc::clone(&state));
                    self.child_measurables
                        .push(Box::new(LayoutChildMeasurable::new(state)));
                    self.child_ids.len() - 1
                }
            };
            self.child_ids.swap(position, from);
            self.child_states.swap(position, from);
            self.child_measurables.swap(position, from);
        }
        &self.child_states[position]
    }

    fn truncate_children(&mut self, len: usize) {
        self.child_ids.truncate(len);
        self.child_states.truncate(len);
        self.child_measurables.truncate(len);
    }

    fn measured_children(
        &self,
        placements: &[Placement],
        placement_indices: &[usize],
        content_offset: Point,
    ) -> Vec<MeasuredChild> {
        let mut measured_children = Vec::with_capacity(self.child_states.len());
        for (index, child_state) in self.child_states.iter().enumerate() {
            let Some(measured) = child_state.measured.borrow_mut().take() else {
                continue;
            };
            let placement =
                placement_for_child(placements, placement_indices, index, child_state.node_id);
            let base_position = child_state.placement_position(placement);
            if placement.is_some() {
                child_state.place_retained(Point {
                    x: base_position.x + measured.offset.x,
                    y: base_position.y + measured.offset.y,
                });
            }
            measured_children.push(MeasuredChild {
                node: RefCell::new(measured),
                offset: Point {
                    x: content_offset.x + base_position.x,
                    y: content_offset.y + base_position.y,
                },
            });
        }
        measured_children
    }

    fn write_node_geometry(
        &self,
        node_id: NodeId,
        node: &LayoutNode,
        measurement: &ModifierChainMeasurement,
    ) {
        let geometry = node.coordinator_geometry();
        let moved = if measurement.uses_chain {
            self.coordinator_chain
                .write_geometry(geometry, measurement.offset)
        } else {
            geometry.replace([GeometryRect {
                x: 0.0,
                y: 0.0,
                width: measurement.size.width,
                height: measurement.size.height,
            }])
        };
        if moved {
            crate::render_state::record_geometry_scene_node(node_id);
        }
    }

    #[cfg(test)]
    pub(crate) fn debug_stats(&self) -> LayoutRuntimeDebugStats {
        LayoutRuntimeDebugStats {
            child_ids: self.child_ids.clone(),
            child_state_ptrs: self
                .child_states
                .iter()
                .map(|state| Rc::as_ptr(state) as *const () as usize)
                .collect(),
            child_measurable_ptrs: self
                .child_measurables
                .iter()
                .map(|measurable| {
                    measurable.as_ref() as *const dyn Measurable as *const () as usize
                })
                .collect(),
            child_measurable_count: self.child_measurables.len(),
            coordinator_node_ptrs: self.coordinator_chain.debug_ptrs(),
            coordinator_node_count: self.coordinator_chain.nodes.len(),
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

#[derive(Default)]
struct LayoutChildFrame {
    builder: RefCell<Option<Rc<LayoutBuilderState>>>,
    error: RefCell<Option<NodeError>>,
    pass: Cell<ChildPass>,
}

/// What a node binds its children for.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ChildPass {
    /// A measure of the node, which records how it read each child, for a
    /// later keep of its measurement.
    #[default]
    Measure,
    /// An intrinsic size of the node, which its parent asks: the record of
    /// the node's last measure stays as it was.
    Intrinsics,
}

impl LayoutChildFrame {
    fn bind(&self, builder: &Rc<LayoutBuilderState>, pass: ChildPass) {
        self.error.borrow_mut().take();
        *self.builder.borrow_mut() = Some(Rc::clone(builder));
        self.pass.set(pass);
    }

    fn measures(&self) -> bool {
        self.pass.get() == ChildPass::Measure
    }

    fn unbind(&self) {
        self.builder.borrow_mut().take();
    }

    fn record_error(&self, err: NodeError) {
        if self.builder.borrow().is_none() {
            return;
        }
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }
}

struct LayoutChildBinding<'a> {
    cache: &'a LayoutNodeCacheHandles,
    layout_state: Option<&'a Rc<RefCell<LayoutState>>>,
    parent_data: Option<cranpose_ui_layout::ParentData>,
    /// The child's own layout or measure is dirty.
    dirty: bool,
    /// Only nodes below the child are dirty.
    descendant_dirty: bool,
}

/// Reads a child that is a layout node or a subcompose node: its flags,
/// cache, layout properties and placement. `None` for another node type.
fn read_layout_child<R>(
    applier: &mut MemoryApplier,
    child_id: NodeId,
    read: impl Fn(
        &dyn Node,
        &LayoutNodeCacheHandles,
        &dyn Fn() -> crate::modifier::LayoutProperties,
        bool,
    ) -> R,
) -> Result<Option<R>, NodeError> {
    match applier.with_node::<LayoutNode, _>(child_id, |child| {
        read(
            &*child,
            child.cache_handles(),
            &|| child.resolved_modifiers().layout_properties(),
            child.is_placed(),
        )
    }) {
        Ok(value) => Ok(Some(value)),
        Err(NodeError::TypeMismatch { .. }) => {
            match applier.with_node::<SubcomposeLayoutNode, _>(child_id, |child| {
                read(
                    &*child,
                    child.cache_handles(),
                    &|| child.resolved_modifiers().layout_properties(),
                    child.layout_state().is_placed(),
                )
            }) {
                Ok(value) => Ok(Some(value)),
                Err(NodeError::TypeMismatch { .. } | NodeError::Missing { .. }) => Ok(None),
                Err(err) => Err(err),
            }
        }
        Err(NodeError::Missing { .. }) => Ok(None),
        Err(err) => Err(err),
    }
}

/// Whether a parent's policy, given `measured` in place of `previous`, would
/// lay its children out the same: the size and baselines it reads and the
/// offset it places the child by are equal.
fn measures_the_same_for_parent(previous: &MeasuredNode, measured: &MeasuredNode) -> bool {
    let (before, after) = (
        previous.alignment_lines_for_parent(),
        measured.alignment_lines_for_parent(),
    );
    previous.size_for_parent() == measured.size_for_parent()
        && previous.offset == measured.offset
        && before.first_baseline() == after.first_baseline()
        && before.last_baseline() == after.last_baseline()
}

fn parent_data_of(props: crate::modifier::LayoutProperties) -> cranpose_ui_layout::ParentData {
    let weight = props.weight().unwrap_or_default();
    cranpose_ui_layout::ParentData {
        weight: weight.weight,
        fill: weight.fill,
        box_alignment: props.box_alignment(),
        row_alignment: props.row_alignment(),
        row_baseline: props.row_baseline(),
        column_alignment: props.column_alignment(),
    }
}

struct LayoutChildMeasureState {
    node_id: NodeId,
    frame: Rc<LayoutChildFrame>,
    cache: RefCell<LayoutNodeCacheHandles>,
    cache_epoch: Cell<u64>,
    force_remeasure: Cell<bool>,
    parent_data: Cell<Option<cranpose_ui_layout::ParentData>>,
    /// The constraints the parent's last measure gave the child.
    measured_constraints: Cell<Option<Constraints>>,
    /// Whether the parent's last measure read the child's intrinsic sizes.
    read_intrinsics: Cell<bool>,
    measured: RefCell<Option<Rc<MeasuredNode>>>,
    last_position: Cell<Option<Point>>,
    layout_state: RefCell<Option<Rc<RefCell<LayoutState>>>>,
}

impl LayoutChildMeasureState {
    fn placement_position(&self, placement: Option<&Placement>) -> Point {
        placement
            .map(|placement| Point {
                x: placement.x,
                y: placement.y,
            })
            .or_else(|| self.last_position.get())
            .unwrap_or_default()
    }

    fn new(node_id: NodeId, frame: Rc<LayoutChildFrame>) -> Rc<Self> {
        Rc::new(Self {
            node_id,
            frame,
            cache: RefCell::new(LayoutNodeCacheHandles::default()),
            cache_epoch: Cell::new(0),
            force_remeasure: Cell::new(true),
            parent_data: Cell::new(None),
            measured_constraints: Cell::new(None),
            read_intrinsics: Cell::new(false),
            measured: RefCell::new(None),
            last_position: Cell::new(None),
            layout_state: RefCell::new(None),
        })
    }

    /// Binds the child for a measure of its parent. A child with only dirty
    /// nodes below it keeps its cache: it measures again, but may keep its
    /// measurement.
    fn bind(&self, binding: LayoutChildBinding<'_>, pass: &LayoutBuilderState) {
        let child_epoch = binding.cache.epoch();
        let stale = binding.dirty || child_epoch < pass.cache_floor;
        let cache_epoch = if stale { pass.cache_epoch } else { child_epoch };
        binding.cache.activate(cache_epoch);
        self.cache.borrow_mut().clone_from(binding.cache);
        self.cache_epoch.set(cache_epoch);
        self.force_remeasure.set(stale || binding.descendant_dirty);
        self.parent_data.set(binding.parent_data);
        if self.frame.measures() {
            self.measured.borrow_mut().take();
            self.last_position.set(None);
            self.measured_constraints.set(None);
            self.read_intrinsics.set(false);
        }
        let mut layout_state = self.layout_state.borrow_mut();
        let shared = layout_state
            .as_ref()
            .zip(binding.layout_state)
            .is_some_and(|(current, bound)| Rc::ptr_eq(current, bound));
        if !shared {
            *layout_state = binding.layout_state.cloned();
        }
    }

    fn place_retained(&self, position: Point) {
        self.last_position.set(Some(position));
        let builder = self.frame.builder.borrow();
        let Some(builder) = builder.as_ref() else {
            return;
        };
        if let Some(layout_state) = self.layout_state.borrow().as_ref() {
            layout_state.borrow_mut().place(position);
            return;
        }
        let Ok(mut applier) = builder.applier.try_borrow_typed() else {
            return;
        };
        let _ = applier.with_node::<SubcomposeLayoutNode, _>(self.node_id, |node| {
            node.set_position(position);
        });
    }

    /// The intrinsic size of a child that is a layout node, from its
    /// modifier chain and measure policy, without measuring it. `None` for
    /// another node type, which measures for it.
    fn layout_intrinsic(&self, kind: IntrinsicKind) -> Option<f32> {
        let builder = self.frame.builder.borrow();
        let builder = builder.as_ref()?;
        match builder.layout_node_intrinsic(self.node_id, kind) {
            Ok(value) => value,
            Err(err) => {
                self.frame.record_error(err);
                Some(0.0)
            }
        }
    }

    fn perform_measure(&self, constraints: Constraints) -> Result<Rc<MeasuredNode>, NodeError> {
        let builder = self.frame.builder.borrow();
        let builder = builder.as_ref().ok_or(NodeError::MissingContext {
            id: self.node_id,
            reason: "layout child applier not configured",
        })?;
        builder.measure_node(self.node_id, constraints)
    }

    fn measure_cached(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        if self.frame.measures() {
            self.measured_constraints.set(Some(constraints));
        }
        let cache = self.cache.borrow();
        cache.activate(self.cache_epoch.get());
        if !self.force_remeasure.get()
            && let Some(cached) = cache.get_measurement(constraints)
        {
            return Some(cached);
        }

        match self.perform_measure(constraints) {
            Ok(measured) => {
                self.force_remeasure.set(false);
                cache.store_measurement(constraints, Rc::clone(&measured));
                Some(measured)
            }
            Err(err) => {
                self.frame.record_error(err);
                None
            }
        }
    }
}

struct LayoutChildMeasurable {
    state: Rc<LayoutChildMeasureState>,
}

impl LayoutChildMeasurable {
    fn new(state: Rc<LayoutChildMeasureState>) -> Self {
        Self { state }
    }

    fn resolved_parent_data(&self) -> Option<cranpose_ui_layout::ParentData> {
        if self.state.frame.builder.borrow().is_none() {
            return None;
        }
        self.state.parent_data.get()
    }

    fn intrinsic(
        &self,
        kind: IntrinsicKind,
        constraints: Constraints,
        extent: fn(Size) -> f32,
    ) -> f32 {
        let state = &self.state;
        if state.frame.measures() {
            state.read_intrinsics.set(true);
        }
        let cache = state.cache.borrow();
        cache.activate(state.cache_epoch.get());
        if !state.force_remeasure.get()
            && let Some(value) = cache.get_intrinsic(&kind)
        {
            return value;
        }
        let value = match state.layout_intrinsic(kind) {
            Some(value) => value,
            None => state
                .measure_cached(constraints)
                .map_or(0.0, |node| extent(node.size_for_parent())),
        };
        cache.store_intrinsic(kind, value);
        value
    }
}

impl PlaceTarget for LayoutChildMeasureState {
    fn place(&self, x: f32, y: f32) {
        let internal_offset = self
            .measured
            .borrow()
            .as_ref()
            .map(|measured| measured.offset)
            .unwrap_or_default();
        self.place_retained(Point {
            x: x + internal_offset.x,
            y: y + internal_offset.y,
        });
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measure(&self, constraints: Constraints) -> Placeable {
        let state = &self.state;
        let measured = state.measure_cached(constraints);
        let (measured_size, size_for_parent) = measured.as_ref().map_or(
            (
                Size {
                    width: 0.0,
                    height: 0.0,
                },
                Size {
                    width: 0.0,
                    height: 0.0,
                },
            ),
            |measured| (measured.size, measured.size_for_parent()),
        );
        if let Some(layout_state) = state.layout_state.borrow().as_ref() {
            layout_state.borrow_mut().set_size(measured_size);
        }
        let alignment_lines = measured
            .as_ref()
            .map_or_else(AlignmentLines::default, |node| {
                node.alignment_lines_for_parent()
            });
        *state.measured.borrow_mut() = measured;

        Placeable::with_place_target(
            size_for_parent.width,
            size_for_parent.height,
            state.node_id,
            Rc::clone(&self.state) as Rc<dyn PlaceTarget>,
        )
        .with_alignment_lines(alignment_lines)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MinWidth(height),
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: height,
                max_height: height,
            },
            |size| size.width,
        )
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MaxWidth(height),
            Constraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: 0.0,
                max_height: height,
            },
            |size| size.width,
        )
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MinHeight(width),
            Constraints {
                min_width: width,
                max_width: width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            |size| size.height,
        )
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.intrinsic(
            IntrinsicKind::MaxHeight(width),
            Constraints {
                min_width: 0.0,
                max_width: width,
                min_height: 0.0,
                max_height: f32::INFINITY,
            },
            |size| size.height,
        )
    }

    fn flex_parent_data(&self) -> Option<cranpose_ui_layout::FlexParentData> {
        let parent_data = self.resolved_parent_data()?;
        if !parent_data.has_weight() {
            return None;
        }
        Some(cranpose_ui_layout::FlexParentData::new(
            parent_data.weight,
            parent_data.fill,
        ))
    }

    fn parent_data(&self) -> cranpose_ui_layout::ParentData {
        self.resolved_parent_data().unwrap_or_default()
    }
}

#[derive(Clone)]
struct RuntimeNodeMetadata {
    modifier: Modifier,
    resolved_modifiers: ResolvedModifiers,
    modifier_slices: Rc<ModifierNodeSlices>,
    semantics: Option<Rc<SemanticsConfiguration>>,
    role: SemanticsRole,
    button_handler: Option<Rc<RefCell<dyn FnMut()>>>,
}

impl Default for RuntimeNodeMetadata {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            resolved_modifiers: ResolvedModifiers::default(),
            modifier_slices: Rc::default(),
            semantics: None,
            role: SemanticsRole::Unknown,
            button_handler: None,
        }
    }
}

fn role_from_modifier_slices(modifier_slices: &ModifierNodeSlices) -> SemanticsRole {
    modifier_slices
        .annotated_text()
        .map_or(SemanticsRole::Layout, |text| SemanticsRole::Text {
            value: SemanticsText(Rc::clone(text)),
        })
}

fn runtime_metadata_for(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<RuntimeNodeMetadata, NodeError> {
    if let Ok(meta) = applier.with_node::<LayoutNode, _>(node_id, |layout| {
        let modifier = layout.modifier.clone();
        let resolved_modifiers = layout.resolved_modifiers();
        let modifier_slices = layout.modifier_slices_snapshot();
        let role = role_from_modifier_slices(&modifier_slices);

        RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            semantics: layout.semantics_configuration().map(Rc::new),
            role,
            button_handler: None,
        }
    }) {
        return Ok(meta);
    }

    if let Ok((modifier, resolved_modifiers, modifier_slices, semantics)) =
        applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            (
                node.modifier(),
                node.resolved_modifiers(),
                node.modifier_slices_snapshot(),
                node.semantics_configuration().map(Rc::new),
            )
        })
    {
        return Ok(RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            semantics,
            role: SemanticsRole::Subcompose,
            button_handler: None,
        });
    }
    Ok(RuntimeNodeMetadata::default())
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
        clear_semantics_dirty_flags(applier, &child.node.borrow())?;
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
        children.push(build_semantics_node_from_live_nodes(
            applier,
            &child.node.borrow(),
            Some(content),
        )?);
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
            let child_node = child.node.borrow();
            if crate::modifier::is_window_root(applier, child_node.node_id) {
                continue;
            }
            let child_origin = Point {
                x: top_left.x + child.offset.x,
                y: top_left.y + child.offset.y,
            };
            children.push(place(applier, &child_node, child_origin, window_transform)?);
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

fn layout_kind_from_metadata(_node_id: NodeId, info: &RuntimeNodeMetadata) -> LayoutNodeKind {
    match &info.role {
        SemanticsRole::Layout => LayoutNodeKind::Layout,
        SemanticsRole::Subcompose => LayoutNodeKind::Subcompose,
        SemanticsRole::Text { .. } => LayoutNodeKind::Layout,
        SemanticsRole::Spacer => LayoutNodeKind::Spacer,
        SemanticsRole::Button => {
            let handler = info
                .button_handler
                .as_ref()
                .cloned()
                .unwrap_or_else(|| Rc::new(RefCell::new(|| {})));
            LayoutNodeKind::Button { on_click: handler }
        }
        SemanticsRole::Unknown => LayoutNodeKind::Unknown,
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
