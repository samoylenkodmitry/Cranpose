pub mod core;
pub mod policies;

use std::{
    cell::{Cell, RefCell},
    fmt,
    mem::size_of,
    rc::Rc,
    sync::OnceLock,
};

use cranpose_core::{
    Applier, ApplierHost, Composer, ConcreteApplierHost, MemoryApplier, Node, NodeError, NodeId,
    Phase, RuntimeHandle, SlotTable, SlotsHost, SnapshotStateObserver,
};

use self::core::Measurable;
use self::core::Placeable;
#[cfg(test)]
use self::core::{HorizontalAlignment, VerticalAlignment};
use crate::modifier::{
    collect_semantics_from_modifier, DimensionConstraint, EdgeInsets, Modifier, ModifierNodeSlices,
    ModifierNodeSlicesDebugStats, Point, Rect as GeometryRect, ResolvedModifiers, Size,
};

use crate::subcompose_layout::{CachedBatchMeasureInputs, SubcomposeLayoutNode};
use crate::widgets::nodes::{IntrinsicKind, LayoutNode, LayoutNodeCacheHandles, LayoutState};
use cranpose_foundation::{
    text::TextRange, CanvasSemanticsNode, InvalidationKind, ModifierNodeContext, NodeCapabilities,
    SemanticsConfiguration, SemanticsCustomAction, SemanticsWidgetRole,
};
use cranpose_ui_layout::{Constraints, MeasurePolicy, Placement};
use web_time::Instant;

/// Runtime context for modifier nodes during measurement.
///
/// Unlike `BasicModifierNodeContext`, this context accumulates invalidations
/// that can be processed after measurement to set dirty flags on the LayoutNode.
#[derive(Default)]
pub(crate) struct LayoutNodeContext {
    invalidations: Vec<InvalidationKind>,
    update_requested: bool,
    active_capabilities: Vec<NodeCapabilities>,
}

impl LayoutNodeContext {
    pub(crate) fn new() -> Self {
        Self::default()
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
}

/// Forces all layout caches to be invalidated on the next measure by incrementing the epoch.
///
/// # ⚠️ Internal Use Only - NOT Public API
///
/// **This function is hidden from public documentation and MUST NOT be called by external code.**
///
/// Only `cranpose-app-shell` may call this for rare global events:
/// - Window/viewport resize
/// - Global font scale or density changes
/// - Debug toggles that affect all layout
///
/// **This is O(entire app size) - extremely expensive!**
///
/// # For Local Changes
///
/// **Do NOT use this for scroll, single-node mutations, or any local layout change.**
/// Instead, use the scoped repass mechanism:
/// ```text
/// cranpose_ui::schedule_layout_repass(node_id);
/// ```
///
/// The scoped path bubbles dirty flags without invalidating all caches, giving you O(subtree) instead of O(app).
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
    size: Size,
    children: usize,
    start: Instant,
) {
    let Some(threshold_ms) = layout_measure_telemetry_threshold_ms() else {
        return;
    };

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

/// RAII guard that:
/// - moves the current MemoryApplier into a ConcreteApplierHost
/// - holds a shared handle to the `SlotTable` used by `LayoutBuilder`
/// - on Drop, always:
///   * restores slots into the host from the shared handle
///   * moves the original MemoryApplier back into the Composition
///
/// This makes `measure_layout` panic/Err-safe wrt both the applier and slots.
/// The key invariant: guard and builder share the same `Rc<RefCell<SlotTable>>`,
/// so the guard never loses access to the authoritative slots even on panic.
struct ApplierSlotGuard<'a> {
    /// The `MemoryApplier` inside the Composition::applier that we must restore into.
    target: &'a mut MemoryApplier,
    /// Host that owns the original MemoryApplier while layout is running.
    host: Rc<ConcreteApplierHost<MemoryApplier>>,
    /// Shared handle to the slot table. Both the guard and the builder hold a clone.
    /// On Drop, we write whatever is in this handle back into the applier.
    slots: Rc<RefCell<SlotTable>>,
}

impl<'a> ApplierSlotGuard<'a> {
    /// Creates a new guard:
    /// - moves the current MemoryApplier out of `target` into a host
    /// - takes the current slots out of the host and wraps them in a shared handle
    fn new(target: &'a mut MemoryApplier) -> Self {
        // Move the original applier into a host; leave `target` with a fresh one
        let original_applier = std::mem::replace(target, MemoryApplier::new());
        let host = Rc::new(ConcreteApplierHost::new(original_applier));

        // Take slots from the host into a shared handle
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

    /// Rc to pass into LayoutBuilder::new_with_epoch
    fn host(&self) -> Rc<ConcreteApplierHost<MemoryApplier>> {
        Rc::clone(&self.host)
    }

    /// Returns the shared handle to slots for the builder to use.
    /// The builder clones this Rc, so both guard and builder share the same slots.
    fn slots_handle(&self) -> Rc<RefCell<SlotTable>> {
        Rc::clone(&self.slots)
    }
}

impl Drop for ApplierSlotGuard<'_> {
    fn drop(&mut self) {
        // 1) Restore slots into the host's MemoryApplier from the shared handle.
        // This works correctly whether we're on the success path or panic/error path,
        // because we always have the shared handle.
        {
            let mut applier_ref = self.host.borrow_typed();
            *applier_ref.slots() = std::mem::take(&mut *self.slots.borrow_mut());
        }

        // 2) Move the original MemoryApplier (with restored/updated slots) back into `target`
        {
            let mut applier_ref = self.host.borrow_typed();
            let original_applier = std::mem::take(&mut *applier_ref);
            let _ = std::mem::replace(self.target, original_applier);
        }
        // No Rc::try_unwrap in Drop → no "panic during panic" risk.
    }
}

/// Result of measuring through the modifier node chain.
struct ModifierChainMeasurement {
    size: Size,
    /// Content offset for scroll/inner transforms - NOT padding semantics
    content_offset: Point,
    /// Node's own offset (from OffsetNode, affects position in parent)
    offset: Point,
}

type LayoutModifierNodeData = (
    usize,
    Rc<RefCell<Box<dyn cranpose_foundation::ModifierNode>>>,
);

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
    tmp_records: ScratchVecPool<(NodeId, ChildRecord)>,
    tmp_child_ids: ScratchVecPool<NodeId>,
    tmp_layout_node_data: ScratchVecPool<LayoutModifierNodeData>,
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

/// Semantic role describing how a node should participate in accessibility and hit testing.
/// Roles are now derived from SemanticsConfiguration rather than widget types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemanticsRole {
    /// Generic container or layout node
    Layout,
    /// Subcomposition boundary
    Subcompose,
    /// Text content derived from the text node semantics payload.
    Text { value: String },
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
    /// Where this node sits in the tree (layout, text, subcomposition …).
    pub role: SemanticsRole,
    /// What kind of control this node is, as a screen reader announces it —
    /// Compose's `Role`. Separate from [`SemanticsNode::role`] because a
    /// clickable `Row` is structurally a layout node and semantically a button.
    pub widget_role: Option<SemanticsWidgetRole>,
    pub actions: Vec<SemanticsAction>,
    pub children: Vec<SemanticsNode>,
    pub description: Option<String>,
    pub state_description: Option<String>,
    pub on_click_label: Option<String>,
    pub selected: Option<bool>,
    pub toggled: Option<bool>,
    pub enabled: bool,
    pub custom_actions: Vec<SemanticsCustomAction>,
    /// Controls this node drew rather than laid out; their bounds are relative
    /// to this node's own top-left. See [`CanvasSemanticsNode`].
    pub canvas_children: Vec<CanvasSemanticsNode>,
    pub editable_text: bool,
    pub text_selection: Option<TextRange>,
}

impl Default for SemanticsNode {
    fn default() -> Self {
        Self {
            node_id: 0,
            role: SemanticsRole::Unknown,
            widget_role: None,
            actions: Vec::new(),
            children: Vec::new(),
            description: None,
            state_description: None,
            on_click_label: None,
            selected: None,
            toggled: None,
            enabled: true,
            custom_actions: Vec::new(),
            canvas_children: Vec::new(),
            editable_text: false,
            text_selection: None,
        }
    }
}

/// Rooted semantics tree extracted after layout.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsTree {
    root: SemanticsNode,
}

impl SemanticsTree {
    fn new(root: SemanticsNode) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &SemanticsNode {
        &self.root
    }
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
    pub modifier_click_handler_count: usize,
    pub modifier_click_handler_capacity: usize,
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
    pub semantics_text_role_bytes: usize,
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
        self.modifier_click_handler_count += stats.click_handler_count;
        self.modifier_click_handler_capacity += stats.click_handler_capacity;
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
    pub modifier: Modifier,
    pub resolved_modifiers: ResolvedModifiers,
    pub modifier_slices: Rc<ModifierNodeSlices>,
    pub kind: LayoutNodeKind,
}

impl LayoutNodeData {
    pub fn new(
        modifier: Modifier,
        resolved_modifiers: ResolvedModifiers,
        modifier_slices: Rc<ModifierNodeSlices>,
        kind: LayoutNodeKind,
    ) -> Self {
        Self {
            modifier,
            resolved_modifiers,
            modifier_slices,
            kind,
        }
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
    SemanticsTree::new(build_semantics_node_from_layout_box(layout_tree.root()))
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
    fn snapshot(
        applier: &mut MemoryApplier,
        node_id: NodeId,
    ) -> Result<Option<(crate::widgets::nodes::layout_node::LayoutState, Vec<NodeId>)>, NodeError>
    {
        match applier.with_node::<LayoutNode, _>(node_id, |node| {
            (node.layout_state(), node.children.clone())
        }) {
            Ok(snapshot) => return Ok(Some(snapshot)),
            Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => {}
            Err(err) => return Err(err),
        }

        match applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            (node.layout_state(), node.active_children())
        }) {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn place(
        applier: &mut MemoryApplier,
        node_id: NodeId,
        parent_content_origin: Point,
        // Accumulated ancestor graphics-layer translation (window px). A node's
        // drawn content is offset by every ancestor layer's translation on top
        // of its layout position, so a text field's true on-screen origin must
        // add this. Scroll offsets are already baked into `parent_content_origin`
        // via placement; this carries the extra translation-transform component.
        parent_layer_translation: Point,
    ) -> Result<Option<LayoutBox>, NodeError> {
        let Some((state, child_ids)) = snapshot(applier, node_id)? else {
            return Ok(None);
        };
        if !state.is_placed {
            return Ok(None);
        }

        let top_left = Point {
            x: parent_content_origin.x + state.position.x,
            y: parent_content_origin.y + state.position.y,
        };
        let rect = GeometryRect {
            x: top_left.x,
            y: top_left.y,
            width: state.size.width,
            height: state.size.height,
        };
        let info = runtime_metadata_for(applier, node_id)?;
        let kind = layout_kind_from_metadata(node_id, &info);
        let RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            ..
        } = info;

        // Add this node's own graphics-layer translation to the accumulated
        // ancestor translation: it shifts where this node (and its subtree) is
        // drawn relative to its layout box. (Scale/rotation are not folded in —
        // handle positioning under a zoom/rotation layer is a documented gap.)
        let layer_translation = match modifier_slices.graphics_layer() {
            Some(layer) => Point {
                x: parent_layer_translation.x + layer.translation_x,
                y: parent_layer_translation.y + layer.translation_y,
            },
            None => parent_layer_translation,
        };

        // Publish the field's TRUE composited window origin for its selection
        // handles: layout position (scroll already baked in) + accumulated layer
        // translation. Re-read every layout pass so handles track live scrolling.
        if let Some(sink) = modifier_slices.text_field_window_origin() {
            sink.set(Point {
                x: top_left.x + layer_translation.x,
                y: top_left.y + layer_translation.y,
            });
        }

        // Publish a scroll container's composited viewport rect (window
        // coordinates) for its `BringIntoViewResponder`, so a focused
        // descendant field can be scrolled above the soft keyboard.
        if let Some(sink) = modifier_slices.viewport_window_rect() {
            sink.set(GeometryRect {
                x: top_left.x + layer_translation.x,
                y: top_left.y + layer_translation.y,
                width: state.size.width,
                height: state.size.height,
            });
        }

        // Publish this node's resolved size to its `pointer_input` handlers so
        // `PointerInputScope::size()` reports the node's real dimensions (the
        // box the dispatched event positions are local to), not `0x0`.
        modifier_slices.publish_pointer_input_size(state.size);

        let data = LayoutNodeData::new(modifier, resolved_modifiers, modifier_slices, kind);
        let child_origin = Point {
            x: top_left.x + state.content_offset.x,
            y: top_left.y + state.content_offset.y,
        };
        let mut children = Vec::with_capacity(child_ids.len());
        for child_id in child_ids {
            if let Some(child) = place(applier, child_id, child_origin, layer_translation)? {
                children.push(child);
            }
        }

        Ok(Some(LayoutBox::new(
            node_id,
            rect,
            state.content_offset,
            data,
            children,
        )))
    }

    place(applier, root, Point::default(), Point::default()).map(|root| root.map(LayoutTree::new))
}

/// Builds a semantics snapshot from retained layout state in the live applier tree.
///
/// This is the on-demand counterpart to [`build_layout_tree_from_applier`].
/// It follows the currently placed child set, including subcompose active
/// children, and clears semantics dirty flags for nodes it visits.
pub fn build_semantics_tree_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<Option<SemanticsTree>, NodeError> {
    fn node(
        applier: &mut MemoryApplier,
        node_id: NodeId,
    ) -> Result<Option<SemanticsNode>, NodeError> {
        match applier.with_node::<LayoutNode, _>(node_id, |layout| {
            let state = layout.layout_state();
            if !state.is_placed {
                return None;
            }
            let role = role_from_modifier_slices(&layout.modifier_slices_snapshot());
            let config = layout.semantics_configuration();
            let children = layout.children.clone();
            layout.clear_needs_semantics();
            Some((role, config, children))
        }) {
            Ok(Some((role, config, child_ids))) => {
                let mut children = Vec::with_capacity(child_ids.len());
                for child_id in child_ids {
                    if let Some(child) = node(applier, child_id)? {
                        children.push(child);
                    }
                }
                return Ok(Some(semantics_node_from_parts(
                    node_id, role, config, children,
                )));
            }
            Ok(None) => return Ok(None),
            Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => {}
            Err(err) => return Err(err),
        }

        match applier.with_node::<SubcomposeLayoutNode, _>(node_id, |subcompose| {
            let state = subcompose.layout_state();
            if !state.is_placed {
                return None;
            }
            let config = collect_semantics_from_modifier(&subcompose.modifier());
            let children = subcompose.active_children();
            subcompose.clear_needs_semantics();
            Some((config, children))
        }) {
            Ok(Some((config, child_ids))) => {
                let mut children = Vec::with_capacity(child_ids.len());
                for child_id in child_ids {
                    if let Some(child) = node(applier, child_id)? {
                        children.push(child);
                    }
                }
                Ok(Some(semantics_node_from_parts(
                    node_id,
                    SemanticsRole::Subcompose,
                    config,
                    children,
                )))
            }
            Ok(None) | Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => {
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }

    node(applier, root).map(|root| root.map(SemanticsTree::new))
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
    Ok(applier.get_mut(root)?.needs_layout())
}

/// Check if the root semantics snapshot is dirty.
///
/// Semantics invalidations bubble to the root the same way layout invalidations do,
/// so a root check is sufficient to determine whether the next layout pass needs to
/// rebuild semantic data even when geometry is otherwise unchanged.
pub fn tree_needs_semantics(applier: &mut dyn Applier, root: NodeId) -> Result<bool, NodeError> {
    Ok(applier.get_mut(root)?.needs_semantics())
}

/// Test helper: bubbles layout dirty flag to root.
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
    process_pending_layout_repasses(applier, root)?;
    let after_repasses = Instant::now();

    let constraints = Constraints {
        min_width: 0.0,
        max_width: max_size.width,
        min_height: 0.0,
        max_height: max_size.height,
    };

    // Selective measure: only increment epoch if something needs MEASURING (not just layout)
    // O(1) check - just look at root's dirty flag (bubbling ensures correctness)
    //
    // CRITICAL: We check needs_MEASURE, not needs_LAYOUT!
    // - needs_measure: size may change, caches must be invalidated
    // - needs_layout: position may change but size is cached (e.g., scroll)
    //
    // Scroll operations bubble needs_layout to ancestors, but NOT needs_measure.
    // Using needs_layout here would wipe ALL caches on every scroll frame, causing
    // O(N) full remeasurement instead of O(changed nodes).
    let (needs_remeasure, _needs_semantics, cached_epoch) = match applier
        .with_node::<LayoutNode, _>(root, |node| {
            (
                node.needs_measure(), // CORRECT: check needs_measure, not needs_layout
                node.needs_semantics(),
                node.cache_handles().epoch(),
            )
        }) {
        Ok(tuple) => tuple,
        Err(NodeError::TypeMismatch { .. }) => {
            let node = applier.get_mut(root)?;
            // Non-LayoutNode roots still expose Node dirty flags.
            // Use needs_measure here so layout-only subtree repasses can reuse
            // the existing cache epoch instead of invalidating the whole tree.
            let measure_dirty = node.needs_measure();
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
        // Fallback when caller root isn't a LayoutNode (e.g. tests using Spacer directly).
        crate::render_state::current_layout_cache_epoch()
    };

    // Move the current applier into a host and set up a guard that will
    // ALWAYS restore:
    // - the MemoryApplier back into `applier`
    // - the SlotTable back into that MemoryApplier
    //
    // IMPORTANT: Declare the guard *before* the builder so the builder
    // is dropped first (both on Ok and on unwind).
    let guard = ApplierSlotGuard::new(applier);
    let applier_host = guard.host();
    let slots_handle = guard.slots_handle();
    let after_guard = Instant::now();

    // Give the builder the shared slots handle - both guard and builder
    // now share access to the same SlotTable via Rc<RefCell<_>>.
    let frame_arena = crate::render_state::take_layout_frame_arena();
    let mut builder = LayoutBuilder::new_with_epoch(
        Rc::clone(&applier_host),
        epoch,
        Rc::clone(&slots_handle),
        frame_arena,
    );
    let after_builder = Instant::now();

    // ---- Measurement -------------------------------------------------------
    // If measurement fails, the guard will restore slots from the shared handle
    // on drop - this is safe because the handle always contains valid slots.

    let measured = builder.measure_node(root, normalize_constraints(constraints))?;
    let after_measure = Instant::now();

    // Root node has no parent to place it, so we must explicitly place it at (0,0).
    // This ensures is_placed=true, allowing the renderer to traverse the tree.
    // Handle both LayoutNode and SubcomposeLayoutNode as potential roots.
    if let Ok(mut applier) = applier_host.try_borrow_typed() {
        if applier
            .with_node::<LayoutNode, _>(root, |node| {
                node.set_position(Point::default());
            })
            .is_err()
        {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(root, |node| {
                node.set_position(Point::default());
            });
        }
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

    // Drop builder before guard - slots are already in the shared handle.
    // Guard's Drop will write them back to the applier.
    drop(builder);
    let after_builder_drop = Instant::now();

    // DO NOT manually unwrap `applier_host` or replace `applier` here.
    // `ApplierSlotGuard::drop` will restore everything when this function returns.
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
    // Measure repasses re-*size* a subtree (and its ancestors), so an enclosing
    // LazyColumn re-measures the node instead of reusing its cached item slot.
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
    applier.get_mut(root)?.mark_needs_layout();
    Ok(())
}

struct LayoutBuilder {
    state: Rc<RefCell<LayoutBuilderState>>,
}

impl LayoutBuilder {
    fn new_with_epoch(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        epoch: u64,
        slots: Rc<RefCell<SlotTable>>,
        frame_arena: FrameLayoutArena,
    ) -> Self {
        Self {
            state: Rc::new(RefCell::new(LayoutBuilderState::new_with_epoch(
                applier,
                epoch,
                slots,
                frame_arena,
            ))),
        }
    }

    fn measure_node(
        &mut self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        LayoutBuilderState::measure_node(Rc::clone(&self.state), node_id, constraints)
    }

    fn set_runtime_handle(&mut self, handle: Option<RuntimeHandle>) {
        self.state.borrow_mut().runtime_handle = handle;
    }
}

impl Drop for LayoutBuilder {
    fn drop(&mut self) {
        if Rc::strong_count(&self.state) != 1 {
            return;
        }
        let Ok(mut state) = self.state.try_borrow_mut() else {
            return;
        };
        crate::render_state::replace_layout_frame_arena(std::mem::take(&mut state.frame_arena));
    }
}

struct LayoutBuilderState {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: Option<RuntimeHandle>,
    /// Shared handle to the slot table. This is shared with ApplierSlotGuard
    /// to ensure panic-safety: even if we panic, the guard can restore slots.
    slots: Rc<RefCell<SlotTable>>,
    cache_epoch: u64,
    frame_arena: FrameLayoutArena,
}

struct LayoutRuntimeFrameBindingCleanup {
    state: Rc<RefCell<LayoutRuntimeState>>,
}

impl LayoutRuntimeFrameBindingCleanup {
    fn new(state: Rc<RefCell<LayoutRuntimeState>>) -> Self {
        Self { state }
    }
}

impl Drop for LayoutRuntimeFrameBindingCleanup {
    fn drop(&mut self) {
        self.state.borrow().clear_frame_bindings();
    }
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
            runtime_handle,
            slots,
            cache_epoch: epoch,
            frame_arena,
        }
    }

    fn try_with_applier_result<R>(
        state_rc: &Rc<RefCell<Self>>,
        f: impl FnOnce(&mut MemoryApplier) -> Result<R, NodeError>,
    ) -> Option<Result<R, NodeError>> {
        let host = {
            let state = state_rc.borrow();
            Rc::clone(&state.applier)
        };

        // Try to borrow - if already borrowed (nested call), return None
        let Ok(mut applier) = host.try_borrow_typed() else {
            return None;
        };

        Some(f(&mut applier))
    }

    fn with_applier_result<R>(
        state_rc: &Rc<RefCell<Self>>,
        f: impl FnOnce(&mut MemoryApplier) -> Result<R, NodeError>,
    ) -> Result<R, NodeError> {
        Self::try_with_applier_result(state_rc, f).unwrap_or_else(|| {
            Err(NodeError::MissingContext {
                id: NodeId::default(),
                reason: "applier already borrowed",
            })
        })
    }

    /// Clears the is_placed flag for a node at the start of measurement.
    /// This ensures nodes that drop out of placement won't render with stale geometry.
    fn clear_node_placed(state_rc: &Rc<RefCell<Self>>, node_id: NodeId) {
        let host = {
            let state = state_rc.borrow();
            Rc::clone(&state.applier)
        };
        let Ok(mut applier) = host.try_borrow_typed() else {
            return;
        };
        // Try LayoutNode first, then SubcomposeLayoutNode
        if applier
            .with_node::<LayoutNode, _>(node_id, |node| {
                node.clear_placed();
            })
            .is_err()
        {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.clear_placed();
            });
        }
    }

    fn measure_node(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let telemetry_start = Instant::now();
        // Clear is_placed at the start of measurement.
        // Nodes that are placed will have is_placed set to true via Placeable::place().
        // Nodes that drop out of placement (not placed this pass) will remain is_placed=false.
        Self::clear_node_placed(&state_rc, node_id);

        // Try SubcomposeLayoutNode first
        if let Some(subcompose) =
            Self::try_measure_subcompose(Rc::clone(&state_rc), node_id, constraints)?
        {
            log_node_measure_telemetry(
                "subcompose",
                node_id,
                constraints,
                subcompose.size,
                subcompose.children.len(),
                telemetry_start,
            );
            return Ok(subcompose);
        }

        // Try LayoutNode (the primary modern path)
        if let Some(result) = Self::try_with_applier_result(&state_rc, |applier| {
            match applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                LayoutNodeSnapshot::from_layout_node(layout_node)
            }) {
                Ok(snapshot) => Ok(Some(snapshot)),
                Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => Ok(None),
                Err(err) => Err(err),
            }
        }) {
            // Applier was available, process the result
            if let Some(snapshot) = result? {
                let measured = Self::measure_layout_node(
                    Rc::clone(&state_rc),
                    node_id,
                    snapshot,
                    constraints,
                )?;
                log_node_measure_telemetry(
                    "layout",
                    node_id,
                    constraints,
                    measured.size,
                    measured.children.len(),
                    telemetry_start,
                );
                return Ok(measured);
            }
        }
        // If applier was busy (None) or snapshot was None, fall through to fallback

        // No alternate fallbacks - all widgets use LayoutNode or SubcomposeLayoutNode
        // If we reach here, it's an unknown node type (shouldn't happen in normal use)
        let measured = Rc::new(MeasuredNode::new(
            node_id,
            Size::default(),
            Point { x: 0.0, y: 0.0 },
            Point::default(), // No content offset for fallback nodes
            Vec::new(),
        ));
        log_node_measure_telemetry(
            "fallback",
            node_id,
            constraints,
            measured.size,
            measured.children.len(),
            telemetry_start,
        );
        Ok(measured)
    }

    fn cached_measure_node_with_applier(
        applier: &mut MemoryApplier,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let Some(data) = Self::layout_child_measure_data(applier, node_id)? else {
            return Ok(None);
        };
        if data.needs_measure || data.cache.epoch() == 0 {
            return Ok(None);
        }

        let Some(measured) = data.cache.get_measurement(constraints) else {
            return Ok(None);
        };

        if let Some(layout_state) = data.layout_state {
            let mut layout_state = layout_state.borrow_mut();
            layout_state.size = measured.size;
            layout_state.measurement_constraints = constraints;
            drop(layout_state);
            let _ = applier.with_node::<LayoutNode, _>(node_id, |node| {
                if data.needs_layout {
                    node.clear_needs_layout();
                }
            });
        } else {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                node.set_measured_size(measured.size);
                if data.needs_layout {
                    node.clear_needs_layout();
                }
            });
        }

        Ok(Some(measured))
    }

    fn try_measure_subcompose(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Option<Rc<MeasuredNode>>, NodeError> {
        let applier_host = {
            let state = state_rc.borrow();
            Rc::clone(&state.applier)
        };

        let (node_handle, resolved_modifiers) = {
            // Try to borrow - if already borrowed (nested measurement), return None
            let Ok(mut applier) = applier_host.try_borrow_typed() else {
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
                let resolved_modifiers = handle.resolved_modifiers();
                (handle, resolved_modifiers)
            } else {
                return Ok(None);
            }
        };

        let runtime_handle = {
            let mut state = state_rc.borrow_mut();
            if state.runtime_handle.is_none() {
                // Try to borrow - if already borrowed, we can't get runtime handle
                if let Ok(applier) = applier_host.try_borrow_typed() {
                    state.runtime_handle = applier.runtime_handle();
                }
            }
            state
                .runtime_handle
                .clone()
                .ok_or(NodeError::MissingContext {
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

        let mut slots_guard = SlotsGuard::take(Rc::clone(&state_rc));
        let slots_host = slots_guard.host();
        let applier_host_dyn: Rc<dyn ApplierHost> = applier_host.clone();
        let observer = SnapshotStateObserver::new(|callback| callback());
        let composer = Composer::new(
            Rc::clone(&slots_host),
            applier_host_dyn,
            runtime_handle.clone(),
            observer,
            Some(node_id),
        );
        composer.enter_phase(Phase::Measure);

        let state_rc_clone = Rc::clone(&state_rc);
        let measure_error = RefCell::new(None);
        let state_rc_for_subcompose = Rc::clone(&state_rc_clone);
        let error_for_subcompose = &measure_error;
        let measured_children = node_handle.measured_children_scratch();
        let measured_children_for_subcompose = Rc::clone(&measured_children);
        let state_rc_for_cached = Rc::clone(&state_rc_clone);
        let error_for_cached = &measure_error;
        let measured_children_for_cached = Rc::clone(&measured_children);
        let measured_children_for_lookup = Rc::clone(&measured_children);
        let measured_children_for_retained = Rc::clone(&measured_children);

        let measure_result = node_handle.measure_with_cached_batch(
            &composer,
            node_id,
            inner_constraints,
            CachedBatchMeasureInputs {
                measurer: Box::new(
                    move |child_id: NodeId, child_constraints: Constraints| -> Size {
                        match Self::measure_node(
                            Rc::clone(&state_rc_for_subcompose),
                            child_id,
                            child_constraints,
                        ) {
                            Ok(measured) => {
                                measured_children_for_subcompose
                                    .borrow_mut()
                                    .insert(child_id, Rc::clone(&measured));
                                measured.size
                            }
                            Err(err) => {
                                let mut slot = error_for_subcompose.borrow_mut();
                                if slot.is_none() {
                                    *slot = Some(err);
                                }
                                Size::default()
                            }
                        }
                    },
                ),
                cached_measure_batch_registrar: Box::new(
                    move |child_ids: &[NodeId],
                          child_constraints: Constraints,
                          out: &mut Vec<Option<Size>>| {
                        out.clear();
                        out.resize(child_ids.len(), None);

                        let applier_host = {
                            let state = state_rc_for_cached.borrow();
                            Rc::clone(&state.applier)
                        };
                        let Ok(mut applier) = applier_host.try_borrow_typed() else {
                            return;
                        };

                        let mut measured_children = measured_children_for_cached.borrow_mut();
                        for (index, &child_id) in child_ids.iter().enumerate() {
                            match Self::cached_measure_node_with_applier(
                                &mut applier,
                                child_id,
                                child_constraints,
                            ) {
                                Ok(Some(measured)) => {
                                    out[index] = Some(measured.size);
                                    measured_children.insert(child_id, Rc::clone(&measured));
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    let mut slot = error_for_cached.borrow_mut();
                                    if slot.is_none() {
                                        *slot = Some(err);
                                    }
                                    break;
                                }
                            }
                        }
                    },
                ),
                retained_measure_lookup: Box::new(move |child_id| {
                    measured_children_for_lookup
                        .borrow()
                        .get(&child_id)
                        .cloned()
                }),
                retained_measure_registrar: Box::new(move |measurements| {
                    let mut measured_children = measured_children_for_retained.borrow_mut();
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

        // NOTE: Children are now managed by the composer via insert_child commands
        // (from parent_stack initialization with root). set_active_children is no longer used.

        let cranpose_ui_layout::MeasureResult {
            size: measured_size,
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
        let mut measured_children_by_id = measured_children.borrow_mut();

        // Update the SubcomposeLayoutNode's size (position will be set by parent's placement)
        if let Ok(mut applier) = applier_host.try_borrow_typed() {
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
                // Policies may place subcomposed children without calling `measure()` first
                // (for example, when they only need a slot's rendered content). Keep the
                // existing fallback for that case, but preserve the policy-time measurement
                // whenever it exists so we don't silently remeasure lazy items with the
                // container's tighter constraints.
                Self::measure_node(Rc::clone(&state_rc), placement.node_id, inner_constraints)?
            };
            let policy_position = Point {
                x: padding.left + placement.x,
                y: padding.top + placement.y,
            };
            // Subcompose policies return raw placements and bypass the
            // child's `Placeable::place`, which normally adds the modifier
            // chain's node offset. Retained rendering reads the live node
            // position, so it must receive that same final coordinate. The
            // measured-tree edge keeps the raw policy position because
            // `build_layout_tree` applies `child.offset` itself.
            let retained_position = Point {
                x: policy_position.x + child.offset.x,
                y: policy_position.y + child.offset.y,
            };

            // Critical: Update the child's retained placement state.
            // Standard layouts do this via Placeable::place(), but SubcomposeLayout
            // logic bypasses Placeables and returns raw Placements. A subcomposed
            // child can itself be a SubcomposeLayout (e.g. a `BoxWithConstraints`
            // inside a `LazyColumn` item), so both node kinds must be positioned
            // and marked placed; otherwise the applier-traversal render, layout,
            // and semantics builds cull the child's whole subtree (issue #305).
            if let Ok(mut applier) = applier_host.try_borrow_typed() {
                if applier
                    .with_node::<LayoutNode, _>(placement.node_id, |node| {
                        node.set_position(retained_position);
                    })
                    .is_err()
                {
                    let _ =
                        applier.with_node::<SubcomposeLayoutNode, _>(placement.node_id, |node| {
                            node.set_position(retained_position);
                        });
                }
            }

            children.push(MeasuredChild {
                node: child,
                offset: policy_position,
            });
        }

        // Update the SubcomposeLayoutNode's active children for rendering
        node_handle.set_active_children(children.iter().map(|c| c.node.node_id));
        node_handle.recycle_placement_scratch(placements);

        Ok(Some(Rc::new(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            Point::default(), // Subcompose nodes: content_offset handled by child layout
            children,
        ))))
    }
    /// Measures through the layout modifier coordinator chain using reconciled modifier nodes.
    /// Iterates through LayoutModifierNode instances from the ModifierNodeChain and calls
    /// their measure() methods through the retained coordinator chain.
    ///
    /// Always succeeds, measuring either directly or through retained layout modifier nodes.
    ///
    fn measure_through_modifier_chain(
        state_rc: &Rc<RefCell<Self>>,
        node_id: NodeId,
        runtime_state: &mut LayoutRuntimeState,
        measure_policy: &Rc<dyn MeasurePolicy>,
        constraints: Constraints,
        layout_node_data: &mut Vec<LayoutModifierNodeData>,
        placements: &mut Vec<Placement>,
    ) -> ModifierChainMeasurement {
        use cranpose_foundation::NodeCapabilities;

        // Collect layout node information from the modifier chain
        layout_node_data.clear();
        let mut offset = Point::default();

        {
            let state = state_rc.borrow();
            let mut applier = state.applier.borrow_typed();

            let _ = applier.with_node::<LayoutNode, _>(node_id, |layout_node| {
                let chain_handle = layout_node.modifier_chain();

                if !chain_handle.has_layout_nodes() {
                    return;
                }

                // Collect indices and node Rc clones for layout modifier nodes
                chain_handle.chain().for_each_forward_matching(
                    NodeCapabilities::LAYOUT,
                    |node_ref| {
                        if let Some(index) = node_ref.entry_index() {
                            // Get the Rc clone for this node
                            if let Some(node_rc) = chain_handle.chain().get_node_rc(index) {
                                layout_node_data.push((index, Rc::clone(&node_rc)));
                            }

                            // Extract offset from OffsetNode for the node's own position
                            // The coordinator chain handles placement_offset (for children),
                            // but the node's offset affects where IT is positioned in the parent
                            node_ref.with_node(|node| {
                                if let Some(offset_node) =
                                    node.as_any()
                                        .downcast_ref::<crate::modifier_nodes::OffsetNode>()
                                {
                                    let delta = offset_node.offset();
                                    offset.x += delta.x;
                                    offset.y += delta.y;
                                }
                            });
                        }
                    },
                );
            });
        }

        // Fast path: if there are no layout modifiers, measure directly without the
        // retained coordinator chain frame.
        if layout_node_data.is_empty() {
            let final_size = measure_policy.measure_into(
                runtime_state.child_measurables(),
                constraints,
                placements,
            );

            return ModifierChainMeasurement {
                size: final_size,
                content_offset: Point::default(),
                offset,
            };
        }

        runtime_state.reconcile_coordinator_chain(layout_node_data.as_slice());
        let frame = CoordinatorFrame::new(
            measure_policy,
            runtime_state.child_measurables(),
            placements,
        );

        // Measure through the complete coordinator chain
        let placeable = runtime_state
            .coordinator_chain()
            .measure_from(0, &frame, constraints);
        let final_size = Size {
            width: placeable.width(),
            height: placeable.height(),
        };

        // Get accumulated content offset from the placeable (computed during measure)
        let content_offset = placeable.content_offset();
        let all_placement_offset = Point {
            x: content_offset.0,
            y: content_offset.1,
        };

        // The content_offset for scroll/inner transforms is the accumulated placement offset
        // MINUS the node's own offset (which affects its position in the parent, not content position).
        // This properly separates: node position (offset) vs inner content position (content_offset).
        let content_offset = Point {
            x: all_placement_offset.x - offset.x,
            y: all_placement_offset.y - offset.y,
        };

        // offset was already extracted from OffsetNode above

        // Process any invalidations requested during measurement
        let invalidations = frame.take_invalidations();
        if !invalidations.is_empty() {
            // Mark the LayoutNode as needing the appropriate passes
            Self::with_applier_result(state_rc, |applier| {
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
            size: final_size,
            content_offset,
            offset,
        }
    }

    fn layout_child_measure_data(
        applier: &mut MemoryApplier,
        child_id: NodeId,
    ) -> Result<Option<LayoutChildMeasureData>, NodeError> {
        match applier.with_node::<LayoutNode, _>(child_id, |n| LayoutChildMeasureData {
            cache: n.cache_handles(),
            layout_state: Some(n.layout_state_handle()),
            needs_layout: n.needs_layout(),
            needs_measure: n.needs_measure(),
        }) {
            Ok(data) => Ok(Some(data)),
            Err(NodeError::TypeMismatch { .. }) => {
                match applier.with_node::<SubcomposeLayoutNode, _>(child_id, |n| {
                    LayoutChildMeasureData {
                        cache: n.cache_handles(),
                        layout_state: None,
                        needs_layout: n.needs_layout(),
                        needs_measure: n.needs_measure(),
                    }
                }) {
                    Ok(data) => Ok(Some(data)),
                    Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => {
                        Ok(None)
                    }
                    Err(err) => Err(err),
                }
            }
            Err(NodeError::Missing { .. }) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn measure_layout_node(
        state_rc: Rc<RefCell<Self>>,
        node_id: NodeId,
        snapshot: LayoutNodeSnapshot,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let cache_epoch = {
            let state = state_rc.borrow();
            state.cache_epoch
        };
        let LayoutNodeSnapshot {
            measure_policy,
            cache,
            layout_runtime_state,
            needs_layout,
            needs_measure,
        } = snapshot;
        cache.activate(cache_epoch);

        if needs_measure {
            // Node has needs_measure=true
        }

        // Only check cache when the node is fully clean.
        // needs_layout=true means either the node itself or one of its descendants
        // must be revisited even if the node's own measured size can stay cached.
        if !needs_measure && !needs_layout {
            // Check cache for current constraints
            if let Some(cached) = cache.get_measurement(constraints) {
                // Clear dirty flag after successful cache hit
                Self::with_applier_result(&state_rc, |applier| {
                    applier.with_node::<LayoutNode, _>(node_id, |node| {
                        node.clear_needs_measure();
                        node.clear_needs_layout();
                    })
                })
                .ok();
                return Ok(cached);
            }
        }

        let (runtime_handle, applier_host) = {
            let state = state_rc.borrow();
            (state.runtime_handle.clone(), Rc::clone(&state.applier))
        };

        let measure_handle = LayoutMeasureHandle::new(Rc::clone(&state_rc));
        let error = Rc::new(RefCell::new(None));
        let mut pools = VecPools::acquire(Rc::clone(&state_rc));
        let (records, child_ids, layout_node_data, placements) = pools.parts();

        applier_host
            .borrow_typed()
            .with_node::<LayoutNode, _>(node_id, |node| {
                child_ids.extend_from_slice(&node.children);
            })?;

        let mut valid_child_count = 0;
        for index in 0..child_ids.len() {
            let child_id = child_ids[index];
            let child_exists = {
                let mut applier = applier_host.borrow_typed();
                Self::layout_child_measure_data(&mut applier, child_id)?.is_some()
            };
            if child_exists {
                child_ids[valid_child_count] = child_id;
                valid_child_count += 1;
            }
        }
        child_ids.truncate(valid_child_count);

        let _frame_binding_cleanup =
            LayoutRuntimeFrameBindingCleanup::new(Rc::clone(&layout_runtime_state));

        {
            let mut runtime_state = layout_runtime_state.borrow_mut();
            runtime_state.reconcile_child_measurables(child_ids.as_slice());

            for (index, &child_id) in child_ids.iter().enumerate() {
                let data = {
                    let mut applier = applier_host.borrow_typed();
                    Self::layout_child_measure_data(&mut applier, child_id)?
                };
                let Some(data) = data else {
                    continue;
                };

                let child_is_dirty = data.needs_layout || data.needs_measure;
                let child_cache_epoch = if child_is_dirty {
                    cache_epoch
                } else {
                    data.cache.epoch()
                };
                let child_state = runtime_state.child_state(index);
                child_state.configure(LayoutChildMeasureConfig {
                    applier: Rc::clone(&applier_host),
                    node_id: child_id,
                    error: Rc::clone(&error),
                    runtime_handle: runtime_handle.clone(),
                    cache: data.cache,
                    cache_epoch: child_cache_epoch,
                    force_remeasure: child_is_dirty,
                    measure_handle: Some(measure_handle.clone()),
                    layout_state: data.layout_state,
                });
                records.push((child_id, ChildRecord { state: child_state }));
            }
        }

        let chain_constraints = constraints;

        let modifier_chain_result = {
            let mut runtime_state = layout_runtime_state.borrow_mut();
            Self::measure_through_modifier_chain(
                &state_rc,
                node_id,
                &mut runtime_state,
                &measure_policy,
                chain_constraints,
                layout_node_data,
                placements,
            )
        };

        // Modifier chain always succeeds - use the node-driven measurement.
        let (width, height, content_offset, offset) = {
            let result = modifier_chain_result;
            // The size is already correct from the modifier chain (modifiers like SizeNode
            // have already enforced their constraints), so we use it directly.
            if let Some(err) = error.borrow_mut().take() {
                return Err(err);
            }

            (
                result.size.width,
                result.size.height,
                result.content_offset,
                result.offset,
            )
        };

        let mut measured_children = Vec::with_capacity(records.len());
        for (child_id, record) in records.iter() {
            if let Some(measured) = record.state.take_measured() {
                let base_position = placements
                    .iter()
                    .find(|placement| placement.node_id == *child_id)
                    .map(|placement| Point {
                        x: placement.x,
                        y: placement.y,
                    })
                    .or_else(|| record.state.last_position())
                    .unwrap_or(Point { x: 0.0, y: 0.0 });
                // Apply content_offset (from scroll/transforms) to child positioning
                let position = Point {
                    x: content_offset.x + base_position.x,
                    y: content_offset.y + base_position.y,
                };
                measured_children.push(MeasuredChild {
                    node: measured,
                    offset: position,
                });
            }
        }

        let measured = Rc::new(MeasuredNode::new(
            node_id,
            Size { width, height },
            offset,
            content_offset,
            measured_children,
        ));

        cache.store_measurement(constraints, Rc::clone(&measured));

        // Clear dirty flags and update derived state
        Self::with_applier_result(&state_rc, |applier| {
            applier.with_node::<LayoutNode, _>(node_id, |node| {
                node.clear_needs_measure();
                node.clear_needs_layout();
                node.set_measured_size(Size { width, height });
                node.set_content_offset(content_offset);
            })
        })
        .ok();

        Ok(measured)
    }
}

struct LayoutChildMeasureData {
    cache: LayoutNodeCacheHandles,
    layout_state: Option<Rc<RefCell<LayoutState>>>,
    needs_layout: bool,
    needs_measure: bool,
}

/// Snapshot of a LayoutNode's data for measuring.
/// This is a temporary copy used during the measure phase, not a live node.
///
/// Note: We capture `needs_measure` here because it's checked during measure to enable
/// selective measure optimization at the individual node level. Even if the tree is partially
/// dirty (some nodes changed), clean nodes can skip measure and use cached results.
struct LayoutNodeSnapshot {
    measure_policy: Rc<dyn MeasurePolicy>,
    cache: LayoutNodeCacheHandles,
    layout_runtime_state: Rc<RefCell<LayoutRuntimeState>>,
    needs_layout: bool,
    /// Whether this specific node needs to be measured (vs using cached measurement)
    needs_measure: bool,
}

impl LayoutNodeSnapshot {
    fn from_layout_node(node: &LayoutNode) -> Self {
        Self {
            measure_policy: Rc::clone(&node.measure_policy),
            cache: node.cache_handles(),
            layout_runtime_state: node.layout_runtime_state_handle(),
            needs_layout: node.needs_layout(),
            needs_measure: node.needs_measure(),
        }
    }
}

// Helper types for accessing subsets of LayoutBuilderState
struct VecPools {
    state: Rc<RefCell<LayoutBuilderState>>,
    records: Vec<(NodeId, ChildRecord)>,
    child_ids: Vec<NodeId>,
    layout_node_data: Vec<LayoutModifierNodeData>,
    placements: Vec<Placement>,
}

impl VecPools {
    fn acquire(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        let (records, child_ids, layout_node_data, placements) = {
            let mut state_mut = state.borrow_mut();
            (
                state_mut.frame_arena.tmp_records.acquire(),
                state_mut.frame_arena.tmp_child_ids.acquire(),
                state_mut.frame_arena.tmp_layout_node_data.acquire(),
                state_mut.frame_arena.tmp_placements.acquire(),
            )
        };
        Self {
            state,
            records,
            child_ids,
            layout_node_data,
            placements,
        }
    }

    #[allow(clippy::type_complexity)] // Returns internal Vec references for layout operations
    fn parts(
        &mut self,
    ) -> (
        &mut Vec<(NodeId, ChildRecord)>,
        &mut Vec<NodeId>,
        &mut Vec<LayoutModifierNodeData>,
        &mut Vec<Placement>,
    ) {
        (
            &mut self.records,
            &mut self.child_ids,
            &mut self.layout_node_data,
            &mut self.placements,
        )
    }
}

impl Drop for VecPools {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        state
            .frame_arena
            .tmp_records
            .release(std::mem::take(&mut self.records));
        state
            .frame_arena
            .tmp_child_ids
            .release(std::mem::take(&mut self.child_ids));
        state
            .frame_arena
            .tmp_layout_node_data
            .release(std::mem::take(&mut self.layout_node_data));
        state
            .frame_arena
            .tmp_placements
            .release(std::mem::take(&mut self.placements));
    }
}

struct SlotsGuard {
    state: Rc<RefCell<LayoutBuilderState>>,
    slots: Option<SlotTable>,
}

impl SlotsGuard {
    fn take(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        let slots = {
            let state_ref = state.borrow();
            let mut slots_ref = state_ref.slots.borrow_mut();
            std::mem::take(&mut *slots_ref)
        };
        Self {
            state,
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

impl Drop for SlotsGuard {
    fn drop(&mut self) {
        if let Some(slots) = self.slots.take() {
            let state_ref = self.state.borrow();
            *state_ref.slots.borrow_mut() = slots;
        }
    }
}

#[derive(Clone)]
struct LayoutMeasureHandle {
    state: Rc<RefCell<LayoutBuilderState>>,
}

impl LayoutMeasureHandle {
    fn new(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        Self { state }
    }

    fn measure(
        &self,
        node_id: NodeId,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        LayoutBuilderState::measure_node(Rc::clone(&self.state), node_id, constraints)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MeasuredNode {
    node_id: NodeId,
    size: Size,
    /// Node's position offset relative to parent (from OffsetNode etc.)
    offset: Point,
    /// Content offset for scroll/inner transforms (NOT node position)
    content_offset: Point,
    children: Vec<MeasuredChild>,
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
            children,
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
    node: Rc<MeasuredNode>,
    offset: Point,
}

struct ChildRecord {
    state: Rc<LayoutChildMeasureState>,
}

struct CoordinatorFrame<'a> {
    measure_policy: &'a Rc<dyn MeasurePolicy>,
    measurables: &'a [Box<dyn Measurable>],
    placements: RefCell<&'a mut Vec<Placement>>,
    context: RefCell<LayoutNodeContext>,
}

impl<'a> CoordinatorFrame<'a> {
    fn new(
        measure_policy: &'a Rc<dyn MeasurePolicy>,
        measurables: &'a [Box<dyn Measurable>],
        placements: &'a mut Vec<Placement>,
    ) -> Self {
        Self {
            measure_policy,
            measurables,
            placements: RefCell::new(placements),
            context: RefCell::new(LayoutNodeContext::new()),
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
}

impl Measurable for CoordinatorLink<'_, '_, '_> {
    fn measure(&self, constraints: Constraints) -> Placeable {
        self.chain.measure_from(self.index, self.frame, constraints)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.chain
            .min_intrinsic_width_from(self.index, self.frame, height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.chain
            .max_intrinsic_width_from(self.index, self.frame, height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.chain
            .min_intrinsic_height_from(self.index, self.frame, width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.chain
            .max_intrinsic_height_from(self.index, self.frame, width)
    }
}

struct CoordinatorNode {
    modifier_index: usize,
    node: Rc<RefCell<Box<dyn cranpose_foundation::ModifierNode>>>,
    measured_size: Cell<Size>,
    accumulated_offset: Cell<Point>,
}

impl CoordinatorNode {
    fn new(
        modifier_index: usize,
        node: Rc<RefCell<Box<dyn cranpose_foundation::ModifierNode>>>,
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
        node: &Rc<RefCell<Box<dyn cranpose_foundation::ModifierNode>>>,
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
}

impl CoordinatorChain {
    fn reconcile(&mut self, layout_node_data: &[LayoutModifierNodeData]) {
        if self.matches(layout_node_data) {
            return;
        }

        let mut previous_nodes = std::mem::take(&mut self.nodes);
        self.nodes.reserve(layout_node_data.len());

        for (modifier_index, node) in layout_node_data.iter() {
            if let Some(position) = previous_nodes
                .iter()
                .position(|candidate| candidate.matches(*modifier_index, node))
            {
                self.nodes.push(previous_nodes.swap_remove(position));
            } else {
                self.nodes
                    .push(CoordinatorNode::new(*modifier_index, Rc::clone(node)));
            }
        }
    }

    fn matches(&self, layout_node_data: &[LayoutModifierNodeData]) -> bool {
        self.nodes.len() == layout_node_data.len()
            && self
                .nodes
                .iter()
                .zip(layout_node_data.iter())
                .all(|(node, (modifier_index, node_rc))| node.matches(*modifier_index, node_rc))
    }

    fn measure_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        constraints: Constraints,
    ) -> Placeable {
        let Some(node) = self.nodes.get(index) else {
            let mut placements = frame.placements.borrow_mut();
            let size =
                frame
                    .measure_policy
                    .measure_into(frame.measurables, constraints, &mut placements);
            return Placeable::value(size.width, size.height, NodeId::default());
        };

        let wrapped = CoordinatorLink {
            chain: self,
            frame,
            index: index + 1,
        };
        let node_borrow = node.node.borrow();

        let Some(layout_node) = node_borrow.as_layout_node() else {
            let placeable = wrapped.measure(constraints);
            let child_accumulated = self.total_content_offset_from(index + 1);
            node.accumulated_offset.set(child_accumulated);
            return Placeable::value_with_offset(
                placeable.width(),
                placeable.height(),
                NodeId::default(),
                (child_accumulated.x, child_accumulated.y),
            );
        };

        let result = match frame.context.try_borrow_mut() {
            Ok(mut context) => layout_node.measure(&mut *context, &wrapped, constraints),
            Err(_) => {
                let mut temp = LayoutNodeContext::new();
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
    }

    fn min_intrinsic_width_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        height: f32,
    ) -> f32 {
        let Some(node) = self.nodes.get(index) else {
            return frame
                .measure_policy
                .min_intrinsic_width(frame.measurables, height);
        };
        let wrapped = CoordinatorLink {
            chain: self,
            frame,
            index: index + 1,
        };
        let node_borrow = node.node.borrow();
        node_borrow
            .as_layout_node()
            .map(|layout_node| layout_node.min_intrinsic_width(&wrapped, height))
            .unwrap_or_else(|| wrapped.min_intrinsic_width(height))
    }

    fn max_intrinsic_width_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        height: f32,
    ) -> f32 {
        let Some(node) = self.nodes.get(index) else {
            return frame
                .measure_policy
                .max_intrinsic_width(frame.measurables, height);
        };
        let wrapped = CoordinatorLink {
            chain: self,
            frame,
            index: index + 1,
        };
        let node_borrow = node.node.borrow();
        node_borrow
            .as_layout_node()
            .map(|layout_node| layout_node.max_intrinsic_width(&wrapped, height))
            .unwrap_or_else(|| wrapped.max_intrinsic_width(height))
    }

    fn min_intrinsic_height_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        width: f32,
    ) -> f32 {
        let Some(node) = self.nodes.get(index) else {
            return frame
                .measure_policy
                .min_intrinsic_height(frame.measurables, width);
        };
        let wrapped = CoordinatorLink {
            chain: self,
            frame,
            index: index + 1,
        };
        let node_borrow = node.node.borrow();
        node_borrow
            .as_layout_node()
            .map(|layout_node| layout_node.min_intrinsic_height(&wrapped, width))
            .unwrap_or_else(|| wrapped.min_intrinsic_height(width))
    }

    fn max_intrinsic_height_from(
        &self,
        index: usize,
        frame: &CoordinatorFrame<'_>,
        width: f32,
    ) -> f32 {
        let Some(node) = self.nodes.get(index) else {
            return frame
                .measure_policy
                .max_intrinsic_height(frame.measurables, width);
        };
        let wrapped = CoordinatorLink {
            chain: self,
            frame,
            index: index + 1,
        };
        let node_borrow = node.node.borrow();
        node_borrow
            .as_layout_node()
            .map(|layout_node| layout_node.max_intrinsic_height(&wrapped, width))
            .unwrap_or_else(|| wrapped.max_intrinsic_height(width))
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

#[derive(Default)]
pub(crate) struct LayoutRuntimeState {
    child_ids: Vec<NodeId>,
    child_states: Vec<Rc<LayoutChildMeasureState>>,
    child_measurables: Vec<Box<dyn Measurable>>,
    coordinator_chain: CoordinatorChain,
}

impl LayoutRuntimeState {
    fn reconcile_child_measurables(&mut self, child_ids: &[NodeId]) {
        if self.child_ids == child_ids {
            return;
        }

        let mut previous_ids = std::mem::take(&mut self.child_ids);
        let mut previous_states = std::mem::take(&mut self.child_states);
        let mut previous_measurables = std::mem::take(&mut self.child_measurables);

        self.child_ids.reserve(child_ids.len());
        self.child_states.reserve(child_ids.len());
        self.child_measurables.reserve(child_ids.len());

        for &child_id in child_ids {
            if let Some(position) = previous_ids.iter().position(|&id| id == child_id) {
                self.child_ids.push(previous_ids.swap_remove(position));
                self.child_states
                    .push(previous_states.swap_remove(position));
                self.child_measurables
                    .push(previous_measurables.swap_remove(position));
            } else {
                let state = LayoutChildMeasureState::new(child_id);
                self.child_ids.push(child_id);
                self.child_states.push(Rc::clone(&state));
                self.child_measurables
                    .push(Box::new(LayoutChildMeasurable::new(state)));
            }
        }
    }

    fn child_state(&self, index: usize) -> Rc<LayoutChildMeasureState> {
        Rc::clone(&self.child_states[index])
    }

    fn child_measurables(&self) -> &[Box<dyn Measurable>] {
        self.child_measurables.as_slice()
    }

    fn reconcile_coordinator_chain(&mut self, layout_node_data: &[LayoutModifierNodeData]) {
        self.coordinator_chain.reconcile(layout_node_data);
    }

    fn coordinator_chain(&self) -> &CoordinatorChain {
        &self.coordinator_chain
    }

    fn clear_frame_bindings(&self) {
        for child_state in &self.child_states {
            child_state.clear_frame_bindings();
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

struct LayoutChildMeasureConfig {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    node_id: NodeId,
    error: Rc<RefCell<Option<NodeError>>>,
    runtime_handle: Option<RuntimeHandle>,
    cache: LayoutNodeCacheHandles,
    cache_epoch: u64,
    force_remeasure: bool,
    measure_handle: Option<LayoutMeasureHandle>,
    layout_state: Option<Rc<RefCell<LayoutState>>>,
}

struct LayoutChildMeasureState {
    applier: RefCell<Option<Rc<ConcreteApplierHost<MemoryApplier>>>>,
    node_id: Cell<NodeId>,
    measured: RefCell<Option<Rc<MeasuredNode>>>,
    last_position: Cell<Option<Point>>,
    error: RefCell<Option<Rc<RefCell<Option<NodeError>>>>>,
    runtime_handle: RefCell<Option<RuntimeHandle>>,
    cache: RefCell<LayoutNodeCacheHandles>,
    cache_epoch: Cell<u64>,
    force_remeasure: Cell<bool>,
    measure_handle: RefCell<Option<LayoutMeasureHandle>>,
    layout_state: RefCell<Option<Rc<RefCell<LayoutState>>>>,
}

impl LayoutChildMeasureState {
    fn new(node_id: NodeId) -> Rc<Self> {
        Rc::new(Self {
            applier: RefCell::new(None),
            node_id: Cell::new(node_id),
            measured: RefCell::new(None),
            last_position: Cell::new(None),
            error: RefCell::new(None),
            runtime_handle: RefCell::new(None),
            cache: RefCell::new(LayoutNodeCacheHandles::default()),
            cache_epoch: Cell::new(0),
            force_remeasure: Cell::new(true),
            measure_handle: RefCell::new(None),
            layout_state: RefCell::new(None),
        })
    }

    fn configure(&self, config: LayoutChildMeasureConfig) {
        config.cache.activate(config.cache_epoch);
        *self.applier.borrow_mut() = Some(config.applier);
        self.node_id.set(config.node_id);
        self.measured.borrow_mut().take();
        self.last_position.set(None);
        *self.error.borrow_mut() = Some(config.error);
        *self.runtime_handle.borrow_mut() = config.runtime_handle;
        *self.cache.borrow_mut() = config.cache;
        self.cache_epoch.set(config.cache_epoch);
        self.force_remeasure.set(config.force_remeasure);
        *self.measure_handle.borrow_mut() = config.measure_handle;
        *self.layout_state.borrow_mut() = config.layout_state;
    }

    fn clear_frame_bindings(&self) {
        self.measured.borrow_mut().take();
        *self.applier.borrow_mut() = None;
        *self.error.borrow_mut() = None;
        *self.runtime_handle.borrow_mut() = None;
        *self.measure_handle.borrow_mut() = None;
        *self.layout_state.borrow_mut() = None;
    }

    fn node_id(&self) -> NodeId {
        self.node_id.get()
    }

    fn cache(&self) -> LayoutNodeCacheHandles {
        self.cache.borrow().clone()
    }

    fn applier(&self) -> Option<Rc<ConcreteApplierHost<MemoryApplier>>> {
        self.applier.borrow().clone()
    }

    fn layout_state(&self) -> Option<Rc<RefCell<LayoutState>>> {
        self.layout_state.borrow().clone()
    }

    fn take_measured(&self) -> Option<Rc<MeasuredNode>> {
        self.measured.borrow_mut().take()
    }

    fn last_position(&self) -> Option<Point> {
        self.last_position.get()
    }

    fn set_last_position(&self, position: Point) {
        self.last_position.set(Some(position));
    }

    fn set_measured(&self, measured: Option<Rc<MeasuredNode>>) {
        *self.measured.borrow_mut() = measured;
    }

    fn record_error(&self, err: NodeError) {
        let Some(error) = self.error.borrow().clone() else {
            return;
        };
        let mut slot = error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn perform_measure(&self, constraints: Constraints) -> Result<Rc<MeasuredNode>, NodeError> {
        let node_id = self.node_id();
        if let Some(handle) = self.measure_handle.borrow().clone() {
            return handle.measure(node_id, constraints);
        }
        let applier = self.applier().ok_or(NodeError::MissingContext {
            id: node_id,
            reason: "layout child applier not configured",
        })?;
        measure_node_with_host(
            applier,
            self.runtime_handle.borrow().clone(),
            node_id,
            constraints,
            self.cache_epoch.get(),
        )
    }

    fn intrinsic_measure(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        let cache = self.cache();
        cache.activate(self.cache_epoch.get());
        if !self.force_remeasure.get() {
            if let Some(cached) = cache.get_measurement(constraints) {
                return Some(cached);
            }
        }

        match self.perform_measure(constraints) {
            Ok(measured) => {
                self.force_remeasure.set(false);
                cache.store_measurement(constraints, Rc::clone(&measured));
                Some(measured)
            }
            Err(err) => {
                self.record_error(err);
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
        let applier = self.state.applier()?;
        let node_id = self.state.node_id();
        let Ok(mut applier) = applier.try_borrow_typed() else {
            return None;
        };

        applier
            .with_node::<LayoutNode, _>(node_id, |layout_node| {
                let props = layout_node.resolved_modifiers().layout_properties();
                let weight = props.weight().unwrap_or_default();
                cranpose_ui_layout::ParentData {
                    weight: weight.weight,
                    fill: weight.fill,
                    box_alignment: props.box_alignment(),
                    row_alignment: props.row_alignment(),
                    column_alignment: props.column_alignment(),
                }
            })
            .ok()
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measure(&self, constraints: Constraints) -> Placeable {
        let state = &self.state;
        let cache = state.cache();
        cache.activate(state.cache_epoch.get());
        let measured_size;
        if !state.force_remeasure.get() {
            if let Some(cached) = cache.get_measurement(constraints) {
                measured_size = cached.size;
                state.set_measured(Some(Rc::clone(&cached)));
            } else {
                match state.perform_measure(constraints) {
                    Ok(measured) => {
                        state.force_remeasure.set(false);
                        measured_size = measured.size;
                        cache.store_measurement(constraints, Rc::clone(&measured));
                        state.set_measured(Some(measured));
                    }
                    Err(err) => {
                        state.record_error(err);
                        state.set_measured(None);
                        measured_size = Size {
                            width: 0.0,
                            height: 0.0,
                        };
                    }
                }
            }
        } else {
            match state.perform_measure(constraints) {
                Ok(measured) => {
                    state.force_remeasure.set(false);
                    measured_size = measured.size;
                    cache.store_measurement(constraints, Rc::clone(&measured));
                    state.set_measured(Some(measured));
                }
                Err(err) => {
                    state.record_error(err);
                    state.set_measured(None);
                    measured_size = Size {
                        width: 0.0,
                        height: 0.0,
                    };
                }
            }
        }

        if let Some(layout_state) = state.layout_state() {
            let mut layout_state = layout_state.borrow_mut();
            layout_state.size = measured_size;
            layout_state.measurement_constraints = constraints;
        } else if let Some(applier) = state.applier() {
            let Ok(mut applier) = applier.try_borrow_typed() else {
                return Placeable::value(
                    measured_size.width,
                    measured_size.height,
                    state.node_id(),
                );
            };
            let _ = applier.with_node::<LayoutNode, _>(state.node_id(), |node| {
                node.set_measured_size(measured_size);
                node.set_measurement_constraints(constraints);
            });
        }

        let state = Rc::clone(&self.state);
        let applier = state.applier();
        let node_id = state.node_id();
        let layout_state = state.layout_state();

        let place_fn = Rc::new(move |x: f32, y: f32| {
            let internal_offset = state
                .measured
                .borrow()
                .as_ref()
                .map(|m| m.offset)
                .unwrap_or_default();

            let position = Point {
                x: x + internal_offset.x,
                y: y + internal_offset.y,
            };
            state.set_last_position(position);

            if let Some(layout_state) = &layout_state {
                let mut layout_state = layout_state.borrow_mut();
                layout_state.position = position;
                layout_state.is_placed = true;
            } else if let Some(applier) = &applier {
                let Ok(mut applier) = applier.try_borrow_typed() else {
                    return;
                };
                if applier
                    .with_node::<LayoutNode, _>(node_id, |node| {
                        node.set_position(position);
                    })
                    .is_err()
                {
                    let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
                        node.set_position(position);
                    });
                }
            }
        });

        Placeable::with_place_fn(measured_size.width, measured_size.height, node_id, place_fn)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        let kind = IntrinsicKind::MinWidth(height);
        let cache = self.state.cache();
        cache.activate(self.state.cache_epoch.get());
        if !self.state.force_remeasure.get() {
            if let Some(value) = cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        };
        if let Some(node) = self.state.intrinsic_measure(constraints) {
            let value = node.size.width;
            cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        let kind = IntrinsicKind::MaxWidth(height);
        let cache = self.state.cache();
        cache.activate(self.state.cache_epoch.get());
        if !self.state.force_remeasure.get() {
            if let Some(value) = cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: height,
        };
        if let Some(node) = self.state.intrinsic_measure(constraints) {
            let value = node.size.width;
            cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        let kind = IntrinsicKind::MinHeight(width);
        let cache = self.state.cache();
        cache.activate(self.state.cache_epoch.get());
        if !self.state.force_remeasure.get() {
            if let Some(value) = cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        if let Some(node) = self.state.intrinsic_measure(constraints) {
            let value = node.size.height;
            cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        let kind = IntrinsicKind::MaxHeight(width);
        let cache = self.state.cache();
        cache.activate(self.state.cache_epoch.get());
        if !self.state.force_remeasure.get() {
            if let Some(value) = cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        if let Some(node) = self.state.intrinsic_measure(constraints) {
            let value = node.size.height;
            cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
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

fn measure_node_with_host(
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: Option<RuntimeHandle>,
    node_id: NodeId,
    constraints: Constraints,
    epoch: u64,
) -> Result<Rc<MeasuredNode>, NodeError> {
    let runtime_handle = match runtime_handle {
        Some(handle) => Some(handle),
        None => applier.borrow_typed().runtime_handle(),
    };
    let mut builder = LayoutBuilder::new_with_epoch(
        applier,
        epoch,
        Rc::new(RefCell::new(SlotTable::default())),
        FrameLayoutArena::default(),
    );
    builder.set_runtime_handle(runtime_handle);
    builder.measure_node(node_id, constraints)
}

#[derive(Clone)]
struct RuntimeNodeMetadata {
    modifier: Modifier,
    resolved_modifiers: ResolvedModifiers,
    modifier_slices: Rc<ModifierNodeSlices>,
    role: SemanticsRole,
    button_handler: Option<Rc<RefCell<dyn FnMut()>>>,
}

impl Default for RuntimeNodeMetadata {
    fn default() -> Self {
        Self {
            modifier: Modifier::empty(),
            resolved_modifiers: ResolvedModifiers::default(),
            modifier_slices: Rc::default(),
            role: SemanticsRole::Unknown,
            button_handler: None,
        }
    }
}

fn role_from_modifier_slices(modifier_slices: &ModifierNodeSlices) -> SemanticsRole {
    modifier_slices
        .text_content()
        .map(|text| SemanticsRole::Text {
            value: text.to_string(),
        })
        .unwrap_or(SemanticsRole::Layout)
}

fn runtime_metadata_for(
    applier: &mut MemoryApplier,
    node_id: NodeId,
) -> Result<RuntimeNodeMetadata, NodeError> {
    // Try LayoutNode (the primary modern path)
    // IMPORTANT: We use with_node (reference) instead of try_clone because cloning
    // LayoutNode creates a NEW ModifierChainHandle with NEW nodes and NEW handlers,
    // which would lose gesture state like press_position.
    if let Ok(meta) = applier.with_node::<LayoutNode, _>(node_id, |layout| {
        let modifier = layout.modifier.clone();
        let resolved_modifiers = layout.resolved_modifiers();
        let modifier_slices = layout.modifier_slices_snapshot();
        let role = role_from_modifier_slices(&modifier_slices);

        RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            role,
            button_handler: None,
        }
    }) {
        return Ok(meta);
    }

    // Try SubcomposeLayoutNode
    if let Ok((modifier, resolved_modifiers, modifier_slices)) = applier
        .with_node::<SubcomposeLayoutNode, _>(node_id, |node| {
            (
                node.modifier(),
                node.resolved_modifiers(),
                node.modifier_slices_snapshot(),
            )
        })
    {
        return Ok(RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
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
                Ok(()) | Err(NodeError::Missing { .. }) | Err(NodeError::TypeMismatch { .. }) => {}
                Err(err) => return Err(err),
            }
        }
        Err(err) => return Err(err),
    }

    for child in &node.children {
        clear_semantics_dirty_flags(applier, &child.node)?;
    }

    Ok(())
}

fn build_semantics_tree_from_live_nodes(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<SemanticsTree, NodeError> {
    Ok(SemanticsTree::new(build_semantics_node_from_live_nodes(
        applier, node,
    )?))
}

fn semantics_node_from_parts(
    node_id: NodeId,
    mut role: SemanticsRole,
    config: Option<SemanticsConfiguration>,
    children: Vec<SemanticsNode>,
) -> SemanticsNode {
    let mut node = SemanticsNode {
        node_id,
        children,
        ..SemanticsNode::default()
    };

    if let Some(config) = config {
        if config.role == Some(SemanticsWidgetRole::Button) {
            role = SemanticsRole::Button;
        }
        // A named click label is how Compose declares `onClick`, so it carries
        // the action with it; an app should not have to set `is_clickable` as
        // well to be activatable.
        if config.is_activatable() {
            node.actions.push(SemanticsAction::Click {
                handler: SemanticsCallback::new(node_id),
            });
        }
        node.widget_role = config.role;
        node.description = config.content_description;
        node.state_description = config.state_description;
        node.on_click_label = config.on_click_label;
        node.selected = config.selected;
        node.toggled = config.toggled;
        node.enabled = config.enabled;
        node.custom_actions = config.custom_actions;
        node.canvas_children = config.canvas_children;
        node.editable_text = config.is_editable_text;
        node.text_selection = config.text_selection;
    }

    node.role = role;
    node
}

fn build_semantics_node_from_live_nodes(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<SemanticsNode, NodeError> {
    let (role, config) = match applier.with_node::<LayoutNode, _>(node.node_id, |layout| {
        let role = role_from_modifier_slices(&layout.modifier_slices_snapshot());
        let config = layout.semantics_configuration();
        layout.clear_needs_semantics();
        (role, config)
    }) {
        Ok(data) => data,
        Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => {
            match applier.with_node::<SubcomposeLayoutNode, _>(node.node_id, |subcompose| {
                subcompose.clear_needs_semantics();
                (
                    SemanticsRole::Subcompose,
                    collect_semantics_from_modifier(&subcompose.modifier()),
                )
            }) {
                Ok(data) => data,
                Err(NodeError::TypeMismatch { .. }) | Err(NodeError::Missing { .. }) => {
                    (SemanticsRole::Unknown, None)
                }
                Err(err) => return Err(err),
            }
        }
        Err(err) => return Err(err),
    };

    let mut children = Vec::with_capacity(node.children.len());
    for child in &node.children {
        children.push(build_semantics_node_from_live_nodes(applier, &child.node)?);
    }

    Ok(semantics_node_from_parts(
        node.node_id,
        role,
        config,
        children,
    ))
}

fn record_semantics_allocation_stats(node: &SemanticsNode, stats: &mut LayoutAllocationDebugStats) {
    stats.semantics_node_count += 1;
    stats.semantics_action_count += node.actions.len();
    stats.semantics_action_capacity += node.actions.capacity();
    stats.semantics_child_count += node.children.len();
    stats.semantics_child_capacity += node.children.capacity();
    stats.semantics_heap_bytes += node.actions.capacity() * size_of::<SemanticsAction>();
    stats.semantics_heap_bytes += node.children.capacity() * size_of::<SemanticsNode>();

    if let Some(description) = &node.description {
        stats.semantics_description_count += 1;
        stats.semantics_description_bytes += description.capacity();
        stats.semantics_heap_bytes += description.capacity();
    }
    if let SemanticsRole::Text { value } = &node.role {
        stats.semantics_text_role_bytes += value.capacity();
        stats.semantics_heap_bytes += value.capacity();
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
        // Accumulated ancestor graphics-layer translation (window px): a node's
        // drawn content is shifted by every ancestor layer's translation on top
        // of its layout position, so a text field's TRUE on-screen origin adds
        // this. Scroll offsets are already baked into `origin` via placement;
        // this carries the extra translation-transform component. Scale/rotation
        // are not folded in (handle placement under a zoom layer is a documented
        // gap).
        parent_layer_translation: Point,
    ) -> Result<LayoutBox, NodeError> {
        // Include the node's own offset (from OffsetNode) in its position
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
        let info = runtime_metadata_for(applier, node.node_id)?;
        let kind = layout_kind_from_metadata(node.node_id, &info);
        let RuntimeNodeMetadata {
            modifier,
            resolved_modifiers,
            modifier_slices,
            ..
        } = info;

        let layer_translation = match modifier_slices.graphics_layer() {
            Some(layer) => Point {
                x: parent_layer_translation.x + layer.translation_x,
                y: parent_layer_translation.y + layer.translation_y,
            },
            None => parent_layer_translation,
        };

        // Publish the field's TRUE composited window origin for its finger
        // selection handles: layout position (ancestor scroll already baked in
        // via placement) + accumulated graphics-layer translation. Re-read every
        // layout pass so the handles (and their window→offset inverse mapping)
        // track the field live as an enclosing list scrolls.
        if let Some(sink) = modifier_slices.text_field_window_origin() {
            sink.set(Point {
                x: top_left.x + layer_translation.x,
                y: top_left.y + layer_translation.y,
            });
        }

        // Publish a scroll container's composited viewport rect (window
        // coordinates) for its `BringIntoViewResponder`.
        if let Some(sink) = modifier_slices.viewport_window_rect() {
            sink.set(GeometryRect {
                x: top_left.x + layer_translation.x,
                y: top_left.y + layer_translation.y,
                width: node.size.width,
                height: node.size.height,
            });
        }

        // Publish this node's resolved size to its `pointer_input` handlers so
        // `PointerInputScope::size()` reports the node's real dimensions.
        modifier_slices.publish_pointer_input_size(node.size);

        let data = LayoutNodeData::new(modifier, resolved_modifiers, modifier_slices, kind);
        let mut children = Vec::with_capacity(node.children.len());
        for child in &node.children {
            let child_origin = Point {
                x: top_left.x + child.offset.x,
                y: top_left.y + child.offset.y,
            };
            children.push(place(
                applier,
                &child.node,
                child_origin,
                layer_translation,
            )?);
        }
        Ok(LayoutBox::new(
            node.node_id,
            rect,
            node.content_offset,
            data,
            children,
        ))
    }

    Ok(LayoutTree::new(place(
        applier,
        node,
        Point { x: 0.0, y: 0.0 },
        Point { x: 0.0, y: 0.0 },
    )?))
}

fn semantics_role_from_layout_box(layout_box: &LayoutBox) -> SemanticsRole {
    match &layout_box.node_data.kind {
        LayoutNodeKind::Subcompose => SemanticsRole::Subcompose,
        LayoutNodeKind::Spacer => SemanticsRole::Spacer,
        LayoutNodeKind::Unknown => SemanticsRole::Unknown,
        LayoutNodeKind::Button { .. } => SemanticsRole::Button,
        LayoutNodeKind::Layout => layout_box
            .node_data
            .modifier_slices()
            .text_content()
            .map(|text| SemanticsRole::Text {
                value: text.to_string(),
            })
            .unwrap_or(SemanticsRole::Layout),
    }
}

fn build_semantics_node_from_layout_box(layout_box: &LayoutBox) -> SemanticsNode {
    let children = layout_box
        .children
        .iter()
        .map(build_semantics_node_from_layout_box)
        .collect();

    semantics_node_from_parts(
        layout_box.node_id,
        semantics_role_from_layout_box(layout_box),
        collect_semantics_from_modifier(&layout_box.node_data.modifier),
        children,
    )
}

fn layout_kind_from_metadata(_node_id: NodeId, info: &RuntimeNodeMetadata) -> LayoutNodeKind {
    match &info.role {
        SemanticsRole::Layout => LayoutNodeKind::Layout,
        SemanticsRole::Subcompose => LayoutNodeKind::Subcompose,
        SemanticsRole::Text { .. } => {
            // Text content is now handled via TextModifierNode in the modifier chain
            // and collected in modifier_slices.text_content(). LayoutNodeKind should
            // reflect the layout policy (EmptyMeasurePolicy), not the content type.
            LayoutNodeKind::Layout
        }
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

#[cfg(test)]
pub(crate) fn align_horizontal(alignment: HorizontalAlignment, available: f32, child: f32) -> f32 {
    match alignment {
        HorizontalAlignment::Start => 0.0,
        HorizontalAlignment::CenterHorizontally => ((available - child) / 2.0).max(0.0),
        HorizontalAlignment::End => (available - child).max(0.0),
    }
}

#[cfg(test)]
pub(crate) fn align_vertical(alignment: VerticalAlignment, available: f32, child: f32) -> f32 {
    match alignment {
        VerticalAlignment::Top => 0.0,
        VerticalAlignment::CenterVertically => ((available - child) / 2.0).max(0.0),
        VerticalAlignment::Bottom => (available - child).max(0.0),
    }
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
        // Intrinsic sizing is resolved at a higher level where we have access to children.
        // At this point we just use the base size as a fallback.
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
