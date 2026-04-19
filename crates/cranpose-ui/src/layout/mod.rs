// WIP: Layout system infrastructure - many helper types not yet fully wired up

pub mod coordinator;
pub mod core;
pub mod policies;

use cranpose_core::collections::map::HashMap;
use std::{
    cell::{Cell, RefCell},
    fmt,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};

use cranpose_core::{
    Applier, ApplierHost, Composer, ConcreteApplierHost, MemoryApplier, Node, NodeError, NodeId,
    Phase, RuntimeHandle, SlotTable, SlotsHost, SnapshotStateObserver,
};

use self::coordinator::NodeCoordinator;
use self::core::Measurable;
use self::core::Placeable;
#[cfg(test)]
use self::core::{HorizontalAlignment, VerticalAlignment};
use crate::modifier::{
    collect_semantics_from_modifier, DimensionConstraint, EdgeInsets, Modifier, ModifierNodeSlices,
    Point, Rect as GeometryRect, ResolvedModifiers, Size,
};

use crate::subcompose_layout::SubcomposeLayoutNode;
use crate::widgets::nodes::{IntrinsicKind, LayoutNode, LayoutNodeCacheHandles};
use cranpose_foundation::{
    InvalidationKind, ModifierNodeContext, NodeCapabilities, SemanticsConfiguration,
};
use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult};

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

static NEXT_CACHE_EPOCH: AtomicU64 = AtomicU64::new(1);

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
    NEXT_CACHE_EPOCH.fetch_add(1, Ordering::Relaxed);
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
    result: MeasureResult,
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
    /// Text content (derived from TextNode for backward compatibility)
    Text { value: String },
    /// Spacer (non-interactive)
    Spacer,
    /// Button (derived from is_button semantics flag)
    Button,
    /// Unknown or unspecified role
    Unknown,
}

/// A single node within the semantics tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticsNode {
    pub node_id: NodeId,
    pub role: SemanticsRole,
    pub actions: Vec<SemanticsAction>,
    pub children: Vec<SemanticsNode>,
    pub description: Option<String>,
}

impl SemanticsNode {
    fn new(
        node_id: NodeId,
        role: SemanticsRole,
        actions: Vec<SemanticsAction>,
        children: Vec<SemanticsNode>,
        description: Option<String>,
    ) -> Self {
        Self {
            node_id,
            role,
            actions,
            children,
            description,
        }
    }
}

/// Rooted semantics tree extracted after layout.
#[derive(Clone, Debug, PartialEq, Eq)]
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
        Ok(measurements.into_layout_tree())
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

    /// Consumes the measurements and produces a [`LayoutTree`].
    pub fn into_layout_tree(self) -> LayoutTree {
        self.layout_tree
            .expect("layout tree was not built for these measurements")
    }

    /// Returns a borrowed [`LayoutTree`] for rendering.
    pub fn layout_tree(&self) -> LayoutTree {
        self.layout_tree
            .clone()
            .expect("layout tree was not built for these measurements")
    }
}

/// Builds a semantics tree from an existing [`LayoutTree`].
///
/// This is useful for consumers that need semantics on demand without forcing
/// every layout pass to eagerly allocate a full [`SemanticsTree`].
pub fn build_semantics_tree_from_layout_tree(layout_tree: &LayoutTree) -> SemanticsTree {
    SemanticsTree::new(build_semantics_node_from_layout_box(layout_tree.root()))
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
    process_pending_layout_repasses(applier, root)?;

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
        NEXT_CACHE_EPOCH.fetch_add(1, Ordering::Relaxed)
    } else if cached_epoch != 0 {
        cached_epoch
    } else {
        // Fallback when caller root isn't a LayoutNode (e.g. tests using Spacer directly).
        NEXT_CACHE_EPOCH.load(Ordering::Relaxed)
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

    // Give the builder the shared slots handle - both guard and builder
    // now share access to the same SlotTable via Rc<RefCell<_>>.
    let mut builder =
        LayoutBuilder::new_with_epoch(Rc::clone(&applier_host), epoch, Rc::clone(&slots_handle));

    // ---- Measurement -------------------------------------------------------
    // If measurement fails, the guard will restore slots from the shared handle
    // on drop - this is safe because the handle always contains valid slots.

    let measured = builder.measure_node(root, normalize_constraints(constraints))?;

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

    // Drop builder before guard - slots are already in the shared handle.
    // Guard's Drop will write them back to the applier.
    drop(builder);

    // DO NOT manually unwrap `applier_host` or replace `applier` here.
    // `ApplierSlotGuard::drop` will restore everything when this function returns.

    Ok(LayoutMeasurements::new(measured, semantics, layout_tree))
}

fn process_pending_layout_repasses(
    applier: &mut MemoryApplier,
    root: NodeId,
) -> Result<(), NodeError> {
    let repass_nodes = crate::take_layout_repass_nodes();
    if repass_nodes.is_empty() {
        return Ok(());
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
    ) -> Self {
        Self {
            state: Rc::new(RefCell::new(LayoutBuilderState::new_with_epoch(
                applier, epoch, slots,
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

struct LayoutBuilderState {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    runtime_handle: Option<RuntimeHandle>,
    /// Shared handle to the slot table. This is shared with ApplierSlotGuard
    /// to ensure panic-safety: even if we panic, the guard can restore slots.
    slots: Rc<RefCell<SlotTable>>,
    cache_epoch: u64,
    tmp_measurables: ScratchVecPool<Box<dyn Measurable>>,
    tmp_records: ScratchVecPool<(NodeId, ChildRecord)>,
    tmp_child_ids: ScratchVecPool<NodeId>,
    tmp_layout_node_data: ScratchVecPool<LayoutModifierNodeData>,
}

impl LayoutBuilderState {
    fn new_with_epoch(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        epoch: u64,
        slots: Rc<RefCell<SlotTable>>,
    ) -> Self {
        let runtime_handle = applier.borrow_typed().runtime_handle();

        Self {
            applier,
            runtime_handle,
            slots,
            cache_epoch: epoch,
            tmp_measurables: ScratchVecPool::default(),
            tmp_records: ScratchVecPool::default(),
            tmp_child_ids: ScratchVecPool::default(),
            tmp_layout_node_data: ScratchVecPool::default(),
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
        // Clear is_placed at the start of measurement.
        // Nodes that are placed will have is_placed set to true via Placeable::place().
        // Nodes that drop out of placement (not placed this pass) will remain is_placed=false.
        Self::clear_node_placed(&state_rc, node_id);

        // Try SubcomposeLayoutNode first
        if let Some(subcompose) =
            Self::try_measure_subcompose(Rc::clone(&state_rc), node_id, constraints)?
        {
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
                return Self::measure_layout_node(
                    Rc::clone(&state_rc),
                    node_id,
                    snapshot,
                    constraints,
                );
            }
        }
        // If applier was busy (None) or snapshot was None, fall through to fallback

        // No alternate fallbacks - all widgets use LayoutNode or SubcomposeLayoutNode
        // If we reach here, it's an unknown node type (shouldn't happen in normal use)
        Ok(Rc::new(MeasuredNode::new(
            node_id,
            Size::default(),
            Point { x: 0.0, y: 0.0 },
            Point::default(), // No content offset for fallback nodes
            Vec::new(),
        )))
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
        let measured_children: Rc<RefCell<HashMap<NodeId, Rc<MeasuredNode>>>> =
            Rc::new(RefCell::new(HashMap::default()));
        let measured_children_for_subcompose = Rc::clone(&measured_children);

        let measure_result = node_handle.measure(
            &composer,
            node_id,
            inner_constraints,
            Box::new(
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
            &measure_error,
        )?;
        slots_guard.restore(slots_host.take());

        if let Some(err) = measure_error.borrow_mut().take() {
            return Err(err);
        }

        // NOTE: Children are now managed by the composer via insert_child commands
        // (from parent_stack initialization with root). set_active_children is no longer used.

        let mut width = measure_result.size.width + padding.horizontal_sum();
        let mut height = measure_result.size.height + padding.vertical_sum();

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

        let mut children = Vec::with_capacity(measure_result.placements.len());
        let mut measured_children_by_id = measured_children.borrow_mut();

        // Update the SubcomposeLayoutNode's size (position will be set by parent's placement)
        if let Ok(mut applier) = applier_host.try_borrow_typed() {
            let _ = applier.with_node::<SubcomposeLayoutNode, _>(node_id, |parent_node| {
                parent_node.set_measured_size(Size { width, height });
                parent_node.clear_needs_measure();
                parent_node.clear_needs_layout();
            });
        }

        for placement in measure_result.placements {
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
            let position = Point {
                x: padding.left + placement.x,
                y: padding.top + placement.y,
            };

            // Critical: Update the child LayoutNode's retained state.
            // Standard layouts do this via Placeable::place(), but SubcomposeLayout logic
            // bypasses Placeables and returns raw Placements.
            if let Ok(mut applier) = applier_host.try_borrow_typed() {
                let _ = applier.with_node::<LayoutNode, _>(placement.node_id, |node| {
                    node.set_position(position);
                });
            }

            children.push(MeasuredChild {
                node: child,
                offset: position,
            });
        }

        // Update the SubcomposeLayoutNode's active children for rendering
        node_handle.set_active_children(children.iter().map(|c| c.node.node_id));

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
    /// their measure() methods, mirroring Jetpack Compose's LayoutModifierNodeCoordinator pattern.
    ///
    /// Always succeeds, building a coordinator chain (possibly just InnerCoordinator) to measure.
    ///
    fn measure_through_modifier_chain(
        state_rc: &Rc<RefCell<Self>>,
        node_id: NodeId,
        measurables: &[Box<dyn Measurable>],
        measure_policy: &Rc<dyn MeasurePolicy>,
        constraints: Constraints,
        layout_node_data: &mut Vec<LayoutModifierNodeData>,
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

        // Fast path: if there are no layout modifiers, measure directly without coordinator chain.
        // This saves 3 allocations (shared_context, policy_result, InnerCoordinator box).
        if layout_node_data.is_empty() {
            let result = measure_policy.measure(measurables, constraints);
            let final_size = result.size;
            let placements = result.placements;

            return ModifierChainMeasurement {
                result: MeasureResult {
                    size: final_size,
                    placements,
                },
                content_offset: Point::default(),
                offset,
            };
        }

        // Slow path: build coordinator chain for layout modifiers.
        // Popping from the end preserves the "rightmost modifier measures first" order
        // without allocating or cloning the collected node list.
        // Create a shared context for this measurement pass to track invalidations
        let shared_context = Rc::new(RefCell::new(LayoutNodeContext::new()));

        // Create the inner coordinator that wraps the measure policy
        let policy_result = Rc::new(RefCell::new(None));
        let inner_coordinator: Box<dyn NodeCoordinator + '_> =
            Box::new(coordinator::InnerCoordinator::new(
                Rc::clone(measure_policy),
                measurables,
                Rc::clone(&policy_result),
            ));

        // Wrap each layout modifier node in a coordinator, building the chain
        let mut current_coordinator = inner_coordinator;
        while let Some((_, node_rc)) = layout_node_data.pop() {
            current_coordinator = Box::new(coordinator::LayoutModifierCoordinator::new(
                node_rc,
                current_coordinator,
                Rc::clone(&shared_context),
            ));
        }

        // Measure through the complete coordinator chain
        let placeable = current_coordinator.measure(constraints);
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

        let placements = policy_result
            .borrow_mut()
            .take()
            .map(|result| result.placements)
            .unwrap_or_default();

        // Process any invalidations requested during measurement
        let invalidations = shared_context.borrow_mut().take_invalidations();
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
            result: MeasureResult {
                size: final_size,
                placements,
            },
            content_offset,
            offset,
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
        let (measurables, records, child_ids, layout_node_data) = pools.parts();

        applier_host
            .borrow_typed()
            .with_node::<LayoutNode, _>(node_id, |node| {
                child_ids.extend_from_slice(&node.children);
            })?;

        for &child_id in child_ids.iter() {
            let measured = Rc::new(RefCell::new(None));
            let position = Rc::new(RefCell::new(None));

            let data = {
                let mut applier = applier_host.borrow_typed();
                match applier.with_node::<LayoutNode, _>(child_id, |n| {
                    (
                        n.cache_handles(),
                        n.layout_state_handle(),
                        n.needs_layout(),
                        n.needs_measure(),
                    )
                }) {
                    Ok((cache, state, needs_layout, needs_measure)) => {
                        Some((cache, Some(state), needs_layout, needs_measure))
                    }
                    Err(NodeError::TypeMismatch { .. }) => {
                        match applier.with_node::<SubcomposeLayoutNode, _>(child_id, |n| {
                            (n.needs_layout(), n.needs_measure())
                        }) {
                            Ok((needs_layout, needs_measure)) => Some((
                                LayoutNodeCacheHandles::default(),
                                None,
                                needs_layout,
                                needs_measure,
                            )),
                            Err(NodeError::TypeMismatch { .. }) => None,
                            Err(NodeError::Missing { .. }) => None,
                            Err(err) => return Err(err),
                        }
                    }
                    Err(NodeError::Missing { .. }) => None,
                    Err(err) => return Err(err),
                }
            };

            let Some((cache_handles, layout_state, needs_layout, needs_measure)) = data else {
                continue;
            };

            cache_handles.activate(cache_epoch);

            records.push((
                child_id,
                ChildRecord {
                    measured: Rc::clone(&measured),
                    last_position: Rc::clone(&position),
                },
            ));
            measurables.push(Box::new(LayoutChildMeasurable::new(
                Rc::clone(&applier_host),
                child_id,
                measured,
                position,
                Rc::clone(&error),
                runtime_handle.clone(),
                cache_handles,
                cache_epoch,
                needs_layout || needs_measure,
                Some(measure_handle.clone()),
                layout_state,
            )));
        }

        let chain_constraints = constraints;

        let modifier_chain_result = Self::measure_through_modifier_chain(
            &state_rc,
            node_id,
            measurables.as_slice(),
            &measure_policy,
            chain_constraints,
            layout_node_data,
        );

        // Modifier chain always succeeds - use the node-driven measurement.
        let (width, height, policy_result, content_offset, offset) = {
            let result = modifier_chain_result;
            // The size is already correct from the modifier chain (modifiers like SizeNode
            // have already enforced their constraints), so we use it directly.
            if let Some(err) = error.borrow_mut().take() {
                return Err(err);
            }

            (
                result.result.size.width,
                result.result.size.height,
                result.result,
                result.content_offset,
                result.offset,
            )
        };

        let mut measured_children = Vec::with_capacity(records.len());
        for (child_id, record) in records.iter() {
            if let Some(measured) = record.measured.borrow_mut().take() {
                let base_position = policy_result
                    .placements
                    .iter()
                    .find(|placement| placement.node_id == *child_id)
                    .map(|placement| Point {
                        x: placement.x,
                        y: placement.y,
                    })
                    .or_else(|| record.last_position.borrow().as_ref().copied())
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

/// Snapshot of a LayoutNode's data for measuring.
/// This is a temporary copy used during the measure phase, not a live node.
///
/// Note: We capture `needs_measure` here because it's checked during measure to enable
/// selective measure optimization at the individual node level. Even if the tree is partially
/// dirty (some nodes changed), clean nodes can skip measure and use cached results.
struct LayoutNodeSnapshot {
    measure_policy: Rc<dyn MeasurePolicy>,
    cache: LayoutNodeCacheHandles,
    needs_layout: bool,
    /// Whether this specific node needs to be measured (vs using cached measurement)
    needs_measure: bool,
}

impl LayoutNodeSnapshot {
    fn from_layout_node(node: &LayoutNode) -> Self {
        Self {
            measure_policy: Rc::clone(&node.measure_policy),
            cache: node.cache_handles(),
            needs_layout: node.needs_layout(),
            needs_measure: node.needs_measure(),
        }
    }
}

// Helper types for accessing subsets of LayoutBuilderState
struct VecPools {
    state: Rc<RefCell<LayoutBuilderState>>,
    measurables: Option<Vec<Box<dyn Measurable>>>,
    records: Option<Vec<(NodeId, ChildRecord)>>,
    child_ids: Option<Vec<NodeId>>,
    layout_node_data: Option<Vec<LayoutModifierNodeData>>,
}

impl VecPools {
    fn acquire(state: Rc<RefCell<LayoutBuilderState>>) -> Self {
        let (measurables, records, child_ids, layout_node_data) = {
            let mut state_mut = state.borrow_mut();
            (
                state_mut.tmp_measurables.acquire(),
                state_mut.tmp_records.acquire(),
                state_mut.tmp_child_ids.acquire(),
                state_mut.tmp_layout_node_data.acquire(),
            )
        };
        Self {
            state,
            measurables: Some(measurables),
            records: Some(records),
            child_ids: Some(child_ids),
            layout_node_data: Some(layout_node_data),
        }
    }

    #[allow(clippy::type_complexity)] // Returns internal Vec references for layout operations
    fn parts(
        &mut self,
    ) -> (
        &mut Vec<Box<dyn Measurable>>,
        &mut Vec<(NodeId, ChildRecord)>,
        &mut Vec<NodeId>,
        &mut Vec<LayoutModifierNodeData>,
    ) {
        let measurables = self
            .measurables
            .as_mut()
            .expect("measurables already returned");
        let records = self.records.as_mut().expect("records already returned");
        let child_ids = self.child_ids.as_mut().expect("child_ids already returned");
        let layout_node_data = self
            .layout_node_data
            .as_mut()
            .expect("layout_node_data already returned");
        (measurables, records, child_ids, layout_node_data)
    }
}

impl Drop for VecPools {
    fn drop(&mut self) {
        let mut state = self.state.borrow_mut();
        if let Some(measurables) = self.measurables.take() {
            state.tmp_measurables.release(measurables);
        }
        if let Some(records) = self.records.take() {
            state.tmp_records.release(records);
        }
        if let Some(child_ids) = self.child_ids.take() {
            state.tmp_child_ids.release(child_ids);
        }
        if let Some(layout_node_data) = self.layout_node_data.take() {
            state.tmp_layout_node_data.release(layout_node_data);
        }
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
}

#[derive(Debug, Clone)]
struct MeasuredChild {
    node: Rc<MeasuredNode>,
    offset: Point,
}

struct ChildRecord {
    measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
    last_position: Rc<RefCell<Option<Point>>>,
}

struct LayoutChildMeasurable {
    applier: Rc<ConcreteApplierHost<MemoryApplier>>,
    node_id: NodeId,
    measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
    last_position: Rc<RefCell<Option<Point>>>,
    error: Rc<RefCell<Option<NodeError>>>,
    runtime_handle: Option<RuntimeHandle>,
    cache: LayoutNodeCacheHandles,
    cache_epoch: u64,
    force_remeasure: Cell<bool>,
    measure_handle: Option<LayoutMeasureHandle>,
    layout_state: Option<Rc<RefCell<crate::widgets::nodes::layout_node::LayoutState>>>,
}

impl LayoutChildMeasurable {
    #[allow(clippy::too_many_arguments)] // Constructor needs all layout state for child measurement
    fn new(
        applier: Rc<ConcreteApplierHost<MemoryApplier>>,
        node_id: NodeId,
        measured: Rc<RefCell<Option<Rc<MeasuredNode>>>>,
        last_position: Rc<RefCell<Option<Point>>>,
        error: Rc<RefCell<Option<NodeError>>>,
        runtime_handle: Option<RuntimeHandle>,
        cache: LayoutNodeCacheHandles,
        cache_epoch: u64,
        force_remeasure: bool,
        measure_handle: Option<LayoutMeasureHandle>,
        layout_state: Option<Rc<RefCell<crate::widgets::nodes::layout_node::LayoutState>>>,
    ) -> Self {
        cache.activate(cache_epoch);
        Self {
            applier,
            node_id,
            measured,
            last_position,
            error,
            runtime_handle,
            cache,
            cache_epoch,
            force_remeasure: Cell::new(force_remeasure),
            measure_handle,
            layout_state,
        }
    }

    fn record_error(&self, err: NodeError) {
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn perform_measure(&self, constraints: Constraints) -> Result<Rc<MeasuredNode>, NodeError> {
        if let Some(handle) = &self.measure_handle {
            handle.measure(self.node_id, constraints)
        } else {
            measure_node_with_host(
                Rc::clone(&self.applier),
                self.runtime_handle.clone(),
                self.node_id,
                constraints,
                self.cache_epoch,
            )
        }
    }

    fn intrinsic_measure(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        self.cache.activate(self.cache_epoch);
        if !self.force_remeasure.get() {
            if let Some(cached) = self.cache.get_measurement(constraints) {
                return Some(cached);
            }
        }

        match self.perform_measure(constraints) {
            Ok(measured) => {
                self.force_remeasure.set(false);
                self.cache
                    .store_measurement(constraints, Rc::clone(&measured));
                Some(measured)
            }
            Err(err) => {
                self.record_error(err);
                None
            }
        }
    }
}

impl Measurable for LayoutChildMeasurable {
    fn measure(&self, constraints: Constraints) -> Placeable {
        self.cache.activate(self.cache_epoch);
        let measured_size;
        if !self.force_remeasure.get() {
            if let Some(cached) = self.cache.get_measurement(constraints) {
                measured_size = cached.size;
                *self.measured.borrow_mut() = Some(Rc::clone(&cached));
            } else {
                match self.perform_measure(constraints) {
                    Ok(measured) => {
                        self.force_remeasure.set(false);
                        measured_size = measured.size;
                        self.cache
                            .store_measurement(constraints, Rc::clone(&measured));
                        *self.measured.borrow_mut() = Some(measured);
                    }
                    Err(err) => {
                        self.record_error(err);
                        self.measured.borrow_mut().take();
                        measured_size = Size {
                            width: 0.0,
                            height: 0.0,
                        };
                    }
                }
            }
        } else {
            match self.perform_measure(constraints) {
                Ok(measured) => {
                    self.force_remeasure.set(false);
                    measured_size = measured.size;
                    self.cache
                        .store_measurement(constraints, Rc::clone(&measured));
                    *self.measured.borrow_mut() = Some(measured);
                }
                Err(err) => {
                    self.record_error(err);
                    self.measured.borrow_mut().take();
                    measured_size = Size {
                        width: 0.0,
                        height: 0.0,
                    };
                }
            }
        }

        // Update retained LayoutNode state with measured size (new architecture).
        // PRIORITIZE direct handle to avoid Applier borrow conflicts during layout!
        if let Some(state) = &self.layout_state {
            let mut state = state.borrow_mut();
            state.size = measured_size;
            state.measurement_constraints = constraints;
        } else if let Ok(mut applier) = self.applier.try_borrow_typed() {
            let _ = applier.with_node::<LayoutNode, _>(self.node_id, |node| {
                node.set_measured_size(measured_size);
                node.set_measurement_constraints(constraints);
            });
        }

        // Build the place closure that captures all state needed for placement
        let applier = Rc::clone(&self.applier);
        let node_id = self.node_id;
        let measured = Rc::clone(&self.measured);
        let last_position = Rc::clone(&self.last_position);
        let layout_state = self.layout_state.clone();

        let place_fn = Rc::new(move |x: f32, y: f32| {
            // Retrieve the node's own offset (from modifiers like offset(), padding(), etc.)
            let internal_offset = measured
                .borrow()
                .as_ref()
                .map(|m| m.offset)
                .unwrap_or_default();

            let position = Point {
                x: x + internal_offset.x,
                y: y + internal_offset.y,
            };
            *last_position.borrow_mut() = Some(position);

            // Update retained LayoutNode state
            if let Some(state) = &layout_state {
                let mut state = state.borrow_mut();
                state.position = position;
                state.is_placed = true;
            } else if let Ok(mut applier) = applier.try_borrow_typed() {
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

        Placeable::with_place_fn(
            measured_size.width,
            measured_size.height,
            self.node_id,
            place_fn,
        )
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        let kind = IntrinsicKind::MinWidth(height);
        self.cache.activate(self.cache_epoch);
        if !self.force_remeasure.get() {
            if let Some(value) = self.cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.width;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        let kind = IntrinsicKind::MaxWidth(height);
        self.cache.activate(self.cache_epoch);
        if !self.force_remeasure.get() {
            if let Some(value) = self.cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: height,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.width;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        let kind = IntrinsicKind::MinHeight(width);
        self.cache.activate(self.cache_epoch);
        if !self.force_remeasure.get() {
            if let Some(value) = self.cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.height;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        let kind = IntrinsicKind::MaxHeight(width);
        self.cache.activate(self.cache_epoch);
        if !self.force_remeasure.get() {
            if let Some(value) = self.cache.get_intrinsic(&kind) {
                return value;
            }
        }
        let constraints = Constraints {
            min_width: 0.0,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        };
        if let Some(node) = self.intrinsic_measure(constraints) {
            let value = node.size.height;
            self.cache.store_intrinsic(kind, value);
            value
        } else {
            0.0
        }
    }

    fn flex_parent_data(&self) -> Option<cranpose_ui_layout::FlexParentData> {
        // Try to borrow the applier - if it's already borrowed (nested measurement), return None.
        // This is safe because parent data doesn't change during measurement.
        let Ok(mut applier) = self.applier.try_borrow_typed() else {
            return None;
        };

        applier
            .with_node::<LayoutNode, _>(self.node_id, |layout_node| {
                let props = layout_node.resolved_modifiers().layout_properties();
                props.weight().map(|weight_data| {
                    cranpose_ui_layout::FlexParentData::new(weight_data.weight, weight_data.fill)
                })
            })
            .ok()
            .flatten()
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
    let mut builder =
        LayoutBuilder::new_with_epoch(applier, epoch, Rc::new(RefCell::new(SlotTable::default())));
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
    if let Err(err) = applier.with_node::<LayoutNode, _>(node.node_id, |layout| {
        layout.clear_needs_semantics();
    }) {
        match err {
            NodeError::Missing { .. } | NodeError::TypeMismatch { .. } => {}
            _ => return Err(err),
        }
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
    let mut actions = Vec::new();
    let mut description = None;

    if let Some(config) = config {
        if config.is_button {
            role = SemanticsRole::Button;
        }
        if config.is_clickable {
            actions.push(SemanticsAction::Click {
                handler: SemanticsCallback::new(node_id),
            });
        }
        description = config.content_description;
    }

    SemanticsNode::new(node_id, role, actions, children, description)
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

fn build_layout_tree(
    applier: &mut MemoryApplier,
    node: &MeasuredNode,
) -> Result<LayoutTree, NodeError> {
    fn place(
        applier: &mut MemoryApplier,
        node: &MeasuredNode,
        origin: Point,
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
        let data = LayoutNodeData::new(modifier, resolved_modifiers, modifier_slices, kind);
        let mut children = Vec::with_capacity(node.children.len());
        for child in &node.children {
            let child_origin = Point {
                x: top_left.x + child.offset.x,
                y: top_left.y + child.offset.y,
            };
            children.push(place(applier, &child.node, child_origin)?);
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
