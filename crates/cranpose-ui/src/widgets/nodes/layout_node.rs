use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    collections::HashMap,
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{Node, NodeId};
use cranpose_foundation::{
    InvalidationKind, ModifierInvalidation, NodeCapabilities, SemanticsConfiguration,
};
use cranpose_ui_layout::{Constraints, MeasurePolicy};

#[cfg(test)]
use crate::layout::LayoutRuntimeDebugStats;
use crate::{
    layout::{LayoutRuntimeState, MeasuredNode},
    modifier::{
        Modifier, ModifierChainHandle, ModifierLocalSource, ModifierLocalToken,
        ModifierLocalsHandle, ModifierNodeSlices, Point, ResolvedModifierLocal, ResolvedModifiers,
        Size,
    },
};

#[derive(Clone, Copy)]
enum LayoutInvalidationDispatchDiag {
    Disabled,
    All,
    Node(NodeId),
}

fn layout_invalidation_dispatch_diag() -> LayoutInvalidationDispatchDiag {
    static MODE: std::sync::OnceLock<LayoutInvalidationDispatchDiag> = std::sync::OnceLock::new();
    *MODE.get_or_init(|| {
        let Some(value) = std::env::var_os("CRANPOSE_LAYOUT_INVALIDATION_DISPATCH_DIAG") else {
            return LayoutInvalidationDispatchDiag::Disabled;
        };
        if value == "all" {
            return LayoutInvalidationDispatchDiag::All;
        }
        value.to_string_lossy().parse::<NodeId>().map_or(
            LayoutInvalidationDispatchDiag::Disabled,
            LayoutInvalidationDispatchDiag::Node,
        )
    })
}

fn log_layout_invalidation_dispatch(
    id: NodeId,
    invalidation: &ModifierInvalidation,
    curr_caps: NodeCapabilities,
    prev_caps: NodeCapabilities,
    modifier: &Modifier,
) {
    let enabled = match layout_invalidation_dispatch_diag() {
        LayoutInvalidationDispatchDiag::Disabled => false,
        LayoutInvalidationDispatchDiag::All => true,
        LayoutInvalidationDispatchDiag::Node(target) => target == id,
    };
    if enabled {
        log::warn!(
            "[layout-invalidation-dispatch] node={id} invalidation={invalidation:?} curr_caps={curr_caps:?} prev_caps={prev_caps:?} modifier={modifier}"
        );
    }
}

/// The running layout pass and how many placed nodes it has cleared and not
/// placed again yet: the nodes it unplaces, unless their parents place them
/// before it ends. Ids start past 0, the pass a node no pass cleared names.
struct PlacementPass {
    id: Cell<u64>,
    unplaced: Cell<usize>,
}

thread_local! {
    static PLACEMENT_PASS: PlacementPass = const {
        PlacementPass {
            id: Cell::new(1),
            unplaced: Cell::new(0),
        }
    };
}

/// Starts the placement bookkeeping of a layout pass.
pub(crate) fn begin_placement_pass() {
    PLACEMENT_PASS.with(|pass| {
        pass.id.set(pass.id.get() + 1);
        pass.unplaced.set(0);
    });
}

/// The running pass, when it has left a node unplaced that it found placed.
pub(crate) fn placement_pass_with_unplaced_nodes() -> Option<u64> {
    PLACEMENT_PASS.with(|pass| (pass.unplaced.get() > 0).then(|| pass.id.get()))
}

/// Retained layout state for a LayoutNode.
/// This mirrors Jetpack Compose's approach where each node stores its own
/// measured size and placed position, eliminating the need for per-frame
/// LayoutTree reconstruction.
/// What a node below a node needs: layout, and measure when `measure` is set.
#[derive(Clone, Copy, Default)]
pub(crate) struct DescendantDirt {
    pub(crate) layout: bool,
    pub(crate) measure: bool,
}

impl DescendantDirt {
    pub(crate) fn marked(self, measure: bool) -> Self {
        Self {
            layout: true,
            measure: self.measure || measure,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LayoutState {
    size: Size,
    position: Point,
    is_placed: bool,
    node_id: Option<NodeId>,
    /// The layout pass that cleared the placed flag while the node was placed.
    cleared_in_pass: u64,
    content_offset: Point,
    semantics_stamp: u64,
}

impl LayoutState {
    pub fn size(&self) -> Size {
        self.size
    }

    pub fn position(&self) -> Point {
        self.position
    }

    /// Offset of the content box relative to the node origin (e.g. due to
    /// padding).
    pub fn content_offset(&self) -> Point {
        self.content_offset
    }

    /// Writes the content offset, reporting an actual change to the
    /// semantics tree.
    pub fn set_content_offset(&mut self, offset: Point) {
        if self.content_offset != offset {
            self.content_offset = offset;
            self.note_semantics_change();
        }
    }

    /// The same state placed at the origin: what a window root's own scene
    /// starts from, since the position its parent gave it belongs to the
    /// parent's window.
    pub fn at_origin(mut self) -> Self {
        self.position = Point::default();
        self
    }

    pub fn is_placed(&self) -> bool {
        self.is_placed
    }

    pub(crate) fn set_node_id(&mut self, node_id: NodeId) {
        self.node_id = Some(node_id);
    }

    /// Writes the measured size, self-reporting an actual change to the
    /// scene phase.
    pub fn set_size(&mut self, size: Size) {
        if self.size != size {
            if let Some(id) = self.node_id {
                crate::render_state::record_geometry_scene_node(id);
            }
            self.size = size;
            self.note_semantics_change();
        }
    }

    /// Writes the placed position and marks the node placed, self-reporting
    /// an actual move to the scene phase, and a placement of a node the
    /// scene does not draw: one the running pass did not find placed. A move
    /// is reported apart, so the scene can keep what it drew for the node.
    pub fn place(&mut self, position: Point) {
        let newly_placed = !self.is_placed && !self.restore_placement();
        let moved = self.position != position;
        if let Some(id) = self.node_id {
            if newly_placed {
                crate::render_state::record_geometry_scene_node(id);
            } else if moved {
                crate::render_state::record_moved_scene_node(id);
            }
        }
        self.position = position;
        if moved || !self.is_placed {
            self.is_placed = true;
            self.note_semantics_change();
        }
    }

    /// Whether the running pass cleared the flag of this node while it was
    /// placed, so placing it again changes nothing the scene draws.
    fn restore_placement(&self) -> bool {
        PLACEMENT_PASS.with(|pass| {
            let restored = self.cleared_in_pass == pass.id.get();
            if restored {
                pass.unplaced.set(pass.unplaced.get().saturating_sub(1));
            }
            restored
        })
    }

    /// Clears the placed flag at the start of a layout pass. Until its parent
    /// places it again the pass counts the node unplaced, and the pass names
    /// it to the scene phase if it ends that way.
    pub fn clear_placed(&mut self) {
        if !self.is_placed {
            return;
        }
        self.is_placed = false;
        self.cleared_in_pass = PLACEMENT_PASS.with(|pass| {
            pass.unplaced.set(pass.unplaced.get() + 1);
            pass.id.get()
        });
        self.note_semantics_change();
    }

    /// Whether `pass` found this node placed and left it unplaced.
    pub(crate) fn unplaced_in(&self, pass: u64) -> bool {
        !self.is_placed && self.cleared_in_pass == pass
    }

    pub(crate) fn note_semantics_change(&mut self) {
        if let Some(id) = self.node_id {
            crate::semantics_layout_log::record_layout_change(id, &mut self.semantics_stamp);
        }
    }
}

#[derive(Clone)]
struct MeasurementCacheEntry {
    constraints: Constraints,
    measured: Rc<MeasuredNode>,
}

#[derive(Clone, Copy, Debug)]
pub enum IntrinsicKind {
    MinWidth(f32),
    MaxWidth(f32),
    MinHeight(f32),
    MaxHeight(f32),
}

impl IntrinsicKind {
    fn discriminant(&self) -> u8 {
        match self {
            IntrinsicKind::MinWidth(_) => 0,
            IntrinsicKind::MaxWidth(_) => 1,
            IntrinsicKind::MinHeight(_) => 2,
            IntrinsicKind::MaxHeight(_) => 3,
        }
    }

    fn value_bits(&self) -> u32 {
        match self {
            IntrinsicKind::MinWidth(value)
            | IntrinsicKind::MaxWidth(value)
            | IntrinsicKind::MinHeight(value)
            | IntrinsicKind::MaxHeight(value) => value.to_bits(),
        }
    }
}

impl PartialEq for IntrinsicKind {
    fn eq(&self, other: &Self) -> bool {
        self.discriminant() == other.discriminant() && self.value_bits() == other.value_bits()
    }
}

impl Eq for IntrinsicKind {}

impl Hash for IntrinsicKind {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.discriminant().hash(state);
        self.value_bits().hash(state);
    }
}

#[derive(Default)]
struct NodeCacheState {
    epoch: u64,
    measurement: Option<MeasurementCacheEntry>,
    intrinsics: Vec<(IntrinsicKind, f32)>,
    /// Whether a parent read intrinsic sizes since the cache was cleared.
    intrinsics_read: bool,
}

#[derive(Default)]
pub(crate) struct LayoutNodeCacheHandles {
    state: Rc<RefCell<NodeCacheState>>,
}

impl Clone for LayoutNodeCacheHandles {
    fn clone(&self) -> Self {
        Self {
            state: Rc::clone(&self.state),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        if !Rc::ptr_eq(&self.state, &source.state) {
            self.state = Rc::clone(&source.state);
        }
    }
}

impl LayoutNodeCacheHandles {
    pub(crate) fn clear(&self) {
        let mut state = self.state.borrow_mut();
        state.measurement = None;
        state.intrinsics.clear();
        state.intrinsics_read = false;
        state.epoch = 0;
    }

    pub(crate) fn activate(&self, epoch: u64) {
        let mut state = self.state.borrow_mut();
        if state.epoch != epoch {
            state.measurement = None;
            state.intrinsics.clear();
            state.intrinsics_read = false;
            state.epoch = epoch;
        }
    }

    /// Drops the intrinsic sizes, which depend on every node below: one of
    /// them changed. A parent's earlier read still counts for
    /// [`Self::has_intrinsics`].
    pub(crate) fn forget_intrinsics(&self) {
        self.state.borrow_mut().intrinsics.clear();
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.state.borrow().epoch
    }

    pub(crate) fn get_measurement(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        let state = self.state.borrow();
        state
            .measurement
            .as_ref()
            .filter(|entry| entry.constraints == constraints)
            .map(|entry| Rc::clone(&entry.measured))
    }

    /// The constraints of the measurement the cache holds.
    pub(crate) fn measured_constraints(&self) -> Option<Constraints> {
        self.state
            .borrow()
            .measurement
            .as_ref()
            .map(|entry| entry.constraints)
    }

    /// Whether a parent read intrinsic sizes from this cache.
    pub(crate) fn has_intrinsics(&self) -> bool {
        self.state.borrow().intrinsics_read
    }

    pub(crate) fn store_measurement(&self, constraints: Constraints, measured: Rc<MeasuredNode>) {
        self.state.borrow_mut().measurement = Some(MeasurementCacheEntry {
            constraints,
            measured,
        });
    }

    pub(crate) fn get_intrinsic(&self, kind: &IntrinsicKind) -> Option<f32> {
        let state = self.state.borrow();
        state
            .intrinsics
            .iter()
            .find(|(stored_kind, _)| stored_kind == kind)
            .map(|(_, value)| *value)
    }

    pub(crate) fn store_intrinsic(&self, kind: IntrinsicKind, value: f32) {
        let mut state = self.state.borrow_mut();
        state.intrinsics_read = true;
        if let Some((_, existing)) = state
            .intrinsics
            .iter_mut()
            .find(|(stored_kind, _)| stored_kind == &kind)
        {
            *existing = value;
        } else {
            state.intrinsics.push((kind, value));
        }
    }
}

pub struct LayoutNode {
    /// Composable definitions responsible for this node, captured only for inspection builds.
    #[cfg(feature = "inspection")]
    pub source_trace: Rc<[cranpose_core::source_trace::SourceLocation]>,
    pub modifier: Modifier,
    modifier_chain: ModifierChainHandle,
    pub measure_policy: Rc<dyn MeasurePolicy>,
    density: crate::density::Density,
    /// The actual children of this node (folded view - includes virtual nodes as-is)
    pub children: Vec<NodeId>,
    cache: LayoutNodeCacheHandles,
    needs_measure: Cell<bool>,
    needs_layout: Cell<bool>,
    /// A node below this one needs layout or measure.
    descendant_dirt: Cell<DescendantDirt>,
    needs_semantics: Cell<bool>,
    /// The semantics tree has to read this node again, though its own
    /// semantics are unchanged: a node below it changed, or its placement
    /// or children did.
    descendant_needs_semantics: Cell<bool>,
    /// The chain's modal and hidden flags, read by the modal count and the
    /// modal walk: dropped whenever the chain syncs or semantics are
    /// invalidated, the two ways its semantics change.
    semantics_reach: Cell<Option<cranpose_foundation::SemanticsReach>>,
    needs_redraw: Cell<bool>,
    needs_pointer_pass: Cell<bool>,
    needs_focus_sync: Cell<bool>,
    parent: Cell<Option<NodeId>>,
    folded_parent: Cell<Option<NodeId>>,
    id: Cell<Option<NodeId>>,
    owner_context_id: Cell<Option<crate::render_state::AppContextId>>,
    debug_modifiers: Cell<bool>,
    is_virtual: bool,
    virtual_children_count: Cell<usize>,

    modifier_slices_snapshot: RefCell<Rc<ModifierNodeSlices>>,
    modifier_slices_dirty: Cell<bool>,

    layout_state: Rc<RefCell<LayoutState>>,
    layout_runtime_state: Rc<RefCell<LayoutRuntimeState>>,
    /// Where the chain's layout modifiers put their content, written by
    /// layout and read by the draws and text the slices collect.
    coordinator_geometry: Rc<crate::modifier::CoordinatorGeometry>,
}

pub(crate) const RECYCLED_LAYOUT_NODE_POOL_LIMIT: usize = 128;

thread_local! {
    static EMPTY_MEASURE_POLICY: Rc<dyn MeasurePolicy> =
        Rc::new(crate::layout::policies::EmptyMeasurePolicy);
}

fn empty_measure_policy() -> Rc<dyn MeasurePolicy> {
    EMPTY_MEASURE_POLICY.with(Rc::clone)
}

impl LayoutNode {
    pub fn new(modifier: Modifier, measure_policy: Rc<dyn MeasurePolicy>) -> Self {
        Self::new_with_virtual(modifier, measure_policy, false)
    }

    /// Create a virtual LayoutNode for subcomposition slot containers.
    /// Virtual nodes are transparent - their children are flattened into parent's children list.
    pub fn new_virtual() -> Self {
        Self::new_with_virtual(Modifier::empty(), empty_measure_policy(), true)
    }

    fn new_recycled_shell(is_virtual: bool) -> Self {
        let mut shell =
            Self::new_with_virtual(Modifier::empty(), empty_measure_policy(), is_virtual);
        shell.needs_measure.set(false);
        shell.needs_layout.set(false);
        shell.needs_semantics.set(false);
        shell.descendant_needs_semantics.set(false);
        shell.needs_redraw.set(false);
        shell.needs_pointer_pass.set(false);
        shell.needs_focus_sync.set(false);
        shell.parent.set(None);
        shell.folded_parent.set(None);
        shell.id.set(None);
        shell.owner_context_id.set(None);
        shell.debug_modifiers.set(false);
        shell.virtual_children_count.set(0);
        shell.modifier_slices_dirty = Cell::new(true);
        shell
    }

    fn new_with_virtual(
        modifier: Modifier,
        measure_policy: Rc<dyn MeasurePolicy>,
        is_virtual: bool,
    ) -> Self {
        let layout_runtime_state = Rc::new(RefCell::new(LayoutRuntimeState::new(Rc::clone(
            &measure_policy,
        ))));
        let mut node = Self {
            #[cfg(feature = "inspection")]
            source_trace: cranpose_core::source_trace::current_source_trace(),
            modifier,
            modifier_chain: ModifierChainHandle::new(),
            measure_policy,
            density: crate::density::Density::default(),
            children: Vec::new(),
            cache: LayoutNodeCacheHandles::default(),
            needs_measure: Cell::new(true),
            needs_layout: Cell::new(true),
            descendant_dirt: Cell::default(),
            needs_semantics: Cell::new(true),
            descendant_needs_semantics: Cell::new(false),
            semantics_reach: Cell::new(None),
            needs_redraw: Cell::new(true),
            needs_pointer_pass: Cell::new(false),
            needs_focus_sync: Cell::new(false),
            parent: Cell::new(None),
            folded_parent: Cell::new(None),
            id: Cell::new(None),
            owner_context_id: Cell::new(None),
            debug_modifiers: Cell::new(false),
            is_virtual,
            virtual_children_count: Cell::new(0),
            modifier_slices_snapshot: RefCell::new(Rc::default()),
            modifier_slices_dirty: Cell::new(true),
            layout_state: Rc::new(RefCell::new(LayoutState::default())),
            layout_runtime_state,
            coordinator_geometry: Rc::default(),
        };
        node.sync_modifier_chain(false);
        node
    }

    pub fn set_modifier(&mut self, modifier: Modifier) {
        #[cfg(feature = "inspection")]
        {
            let trace = cranpose_core::source_trace::current_source_trace();
            if !trace.is_empty() {
                self.source_trace = trace;
            }
        }
        // An equal modifier leaves every element's node as it was. Only a
        // chain reading modifier locals resyncs, since its parent may now
        // provide others.
        if self.modifier == modifier
            && !self
                .modifier_capabilities()
                .contains(NodeCapabilities::MODIFIER_LOCALS)
        {
            return;
        }
        let modifier_changed = !self.modifier.structural_eq(&modifier);
        // A text that changed and nothing else, as a ticker's does, keeps
        // the slices the chain gave: only their text moves to the new layout.
        let text_only = modifier_changed
            && self
                .modifier
                .differs_only_in::<crate::text_modifier_node::TextModifierElement>(&modifier);
        self.modifier = modifier;
        self.sync_modifier_chain(text_only);
        if modifier_changed {
            self.cache.clear();
            self.request_semantics_update();
        }
    }

    /// Reconciles the chain with the node's modifier. `text_only` says the
    /// modifier changed only in its text elements: current slices are then
    /// kept, with their text pointed at the new layout.
    fn sync_modifier_chain(&mut self, text_only: bool) {
        let prev_caps = self.modifier_capabilities();
        let start_parent = self.parent();
        let mut resolver = move |token: &ModifierLocalToken| {
            resolve_modifier_local_from_parent_chain(start_parent, token)
        };
        self.modifier_chain
            .set_debug_logging(self.debug_modifiers.get());
        self.modifier_chain.set_node_id(self.id.get());
        let modifier_local_invalidations = self
            .modifier_chain
            .update_with_resolver(&self.modifier, &mut resolver);
        if prev_caps.contains(NodeCapabilities::WINDOW_ROOT)
            != self
                .modifier_capabilities()
                .contains(NodeCapabilities::WINDOW_ROOT)
        {
            self.note_semantics_layout_change();
        }
        self.forget_semantics_reach();
        let keep_slices =
            text_only && !self.modifier_slices_dirty.get() && self.point_slices_at_text();
        if !keep_slices {
            self.modifier_slices_dirty.set(true);
        }

        let mut invalidations = self.modifier_chain.take_invalidations();
        invalidations.extend(modifier_local_invalidations);
        self.dispatch_modifier_invalidations_with_prev(&invalidations, prev_caps, keep_slices);
        self.refresh_registry_state();
    }

    /// Points the current slices' text at the chain's text node, and
    /// returns whether the chain has one.
    fn point_slices_at_text(&self) -> bool {
        let mut layout = None;
        self.modifier_chain.chain().for_each_forward_matching(
            NodeCapabilities::LAYOUT,
            |node_ref| {
                node_ref.with_node(|node| {
                    if layout.is_none()
                        && let Some(text) = node
                            .as_any()
                            .downcast_ref::<crate::text_modifier_node::TextModifierNode>()
                    {
                        layout = Some(text.prepared_layout_handle());
                    }
                });
            },
        );
        let Some(layout) = layout else {
            return false;
        };
        Rc::make_mut(&mut *self.modifier_slices_snapshot.borrow_mut()).replace_text_layout(layout);
        true
    }

    fn update_modifier_slices_cache(&self) {
        let mut snapshot = self.modifier_slices_snapshot.borrow_mut();
        crate::modifier::collect_modifier_slices_into_shared(
            self.modifier_chain.chain(),
            &mut snapshot,
            &self.coordinator_geometry,
            self.density.density(),
        );
        self.modifier_slices_dirty.set(false);
    }

    /// Whether the modifier slices are built and current.
    #[cfg(test)]
    pub(crate) fn modifier_slices_ready(&self) -> bool {
        !self.modifier_slices_dirty.get()
    }

    pub(crate) fn mark_modifier_slices_dirty(&self) {
        self.modifier_slices_dirty.set(true);
    }

    #[cfg(test)]
    fn dispatch_modifier_invalidations(&self, invalidations: &[ModifierInvalidation]) {
        self.dispatch_modifier_invalidations_with_prev(
            invalidations,
            NodeCapabilities::empty(),
            false,
        );
    }

    /// Applies `invalidations` to the node. Each marks the slices for a
    /// rebuild unless `keep_slices`.
    fn dispatch_modifier_invalidations_with_prev(
        &self,
        invalidations: &[ModifierInvalidation],
        prev_caps: NodeCapabilities,
        keep_slices: bool,
    ) {
        let curr_caps = self.modifier_capabilities();
        for invalidation in invalidations {
            if !keep_slices {
                self.modifier_slices_dirty.set(true);
            }
            let has_capability =
                |capability| curr_caps.contains(capability) || prev_caps.contains(capability);
            match invalidation.kind() {
                InvalidationKind::Layout => {
                    if has_capability(NodeCapabilities::LAYOUT) {
                        self.mark_needs_measure();
                        if let Some(id) = self.id.get() {
                            log_layout_invalidation_dispatch(
                                id,
                                invalidation,
                                curr_caps,
                                prev_caps,
                                &self.modifier,
                            );
                            let inside_composition =
                                cranpose_core::composer_context::try_with_composer(|_| ())
                                    .is_some();
                            if inside_composition {
                                cranpose_core::bubble_measure_dirty_in_composer(id);
                            } else {
                                crate::schedule_layout_repass(id);
                            }
                        }
                    }
                }
                InvalidationKind::Draw => {
                    if has_capability(NodeCapabilities::DRAW)
                        || invalidation.capabilities().contains(NodeCapabilities::DRAW)
                    {
                        self.mark_needs_redraw();
                    }
                }
                InvalidationKind::PointerInput => {
                    if has_capability(NodeCapabilities::POINTER_INPUT) {
                        self.mark_needs_pointer_pass();
                        crate::request_pointer_invalidation();
                        if let Some(id) = self.id.get() {
                            crate::schedule_pointer_repass(id);
                        }
                    }
                }
                InvalidationKind::Semantics => {
                    self.request_semantics_update();
                }
                InvalidationKind::Focus => {
                    if has_capability(NodeCapabilities::FOCUS) {
                        self.mark_needs_focus_sync();
                        crate::request_focus_invalidation();
                        if let Some(id) = self.id.get() {
                            crate::schedule_focus_invalidation(id);
                        }
                    }
                }
            }
        }
    }

    /// The grid this node was composed against.
    pub fn density(&self) -> crate::density::Density {
        self.density
    }

    /// Records the grid the composition provided, re-measuring if it moved.
    pub fn set_density(&mut self, density: crate::density::Density) {
        if self.density != density {
            self.density = density;
            self.cache.clear();
            self.modifier_slices_dirty.set(true);
            self.mark_needs_measure();
        }
    }

    /// Updates relative modifier placement for this node's composition scope.
    pub fn set_layout_direction(&mut self, direction: crate::LayoutDirection) {
        if self.modifier_chain.set_layout_direction(direction) {
            self.cache.clear();
            self.modifier_slices_dirty.set(true);
            self.mark_needs_measure();
        }
    }

    pub fn set_measure_policy(&mut self, policy: Rc<dyn MeasurePolicy>) {
        if !Rc::ptr_eq(&self.measure_policy, &policy) {
            self.measure_policy = policy;
            self.cache.clear();
            self.mark_needs_measure();
            if let Some(id) = self.id.get() {
                cranpose_core::bubble_measure_dirty_in_composer(id);
            }
        }
    }

    /// Mark this node as needing measure. Also marks it as needing layout.
    pub fn mark_needs_measure(&self) {
        self.needs_measure.set(true);
        self.needs_layout.set(true);
    }

    /// Mark this node as needing layout (but not necessarily measure).
    pub fn mark_needs_layout(&self) {
        self.needs_layout.set(true);
    }

    /// Mark this node as needing redraw without forcing measure/layout.
    pub fn mark_needs_redraw(&self) {
        self.needs_redraw.set(true);
        if let Some(id) = self.id.get() {
            crate::schedule_draw_repass(id);
        }
        crate::request_render_invalidation();
    }

    /// Check if this node needs measure.
    pub fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    /// Check if this node needs layout.
    pub fn needs_layout(&self) -> bool {
        self.needs_layout.get()
    }

    /// Mark this node as needing semantics recomputation.
    pub fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
        self.forget_semantics_reach();
    }

    pub(crate) fn clear_needs_semantics(&self) {
        self.needs_semantics.set(false);
        self.descendant_needs_semantics.set(false);
    }

    /// Returns true when this node's semantics or a descendant's need to be
    /// recomputed.
    pub fn needs_semantics(&self) -> bool {
        self.needs_semantics.get() || self.descendant_needs_semantics.get()
    }

    /// Whether this node's own semantics changed since the tree last read
    /// them, as opposed to only a descendant's.
    pub(crate) fn semantics_changed(&self) -> bool {
        self.needs_semantics.get()
    }

    /// Returns true when this node requested a redraw since the last render pass.
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.get()
    }

    pub fn clear_needs_redraw(&self) {
        self.needs_redraw.set(false);
    }

    fn request_semantics_update(&self) {
        let already_dirty = self.needs_semantics.replace(true);
        if already_dirty {
            return;
        }

        if let Some(id) = self.id.get() {
            cranpose_core::queue_semantics_invalidation(id);
        }
    }

    pub(crate) fn clear_needs_measure(&self) {
        self.needs_measure.set(false);
    }

    /// Clears the node's layout flag and its descendants': a layout pass
    /// visited them.
    pub(crate) fn clear_needs_layout(&self) {
        self.needs_layout.set(false);
        self.descendant_dirt.set(DescendantDirt::default());
    }

    /// Marks this node as needing a fresh pointer-input pass.
    pub fn mark_needs_pointer_pass(&self) {
        self.needs_pointer_pass.set(true);
    }

    /// Returns true when pointer-input state needs to be recomputed.
    pub fn needs_pointer_pass(&self) -> bool {
        self.needs_pointer_pass.get()
    }

    /// Clears the pointer-input dirty flag after hosts service it.
    pub fn clear_needs_pointer_pass(&self) {
        self.needs_pointer_pass.set(false);
    }

    /// Marks this node as needing a focus synchronization.
    pub fn mark_needs_focus_sync(&self) {
        self.needs_focus_sync.set(true);
    }

    /// Returns true when focus state needs to be synchronized.
    pub fn needs_focus_sync(&self) -> bool {
        self.needs_focus_sync.get()
    }

    /// Clears the focus dirty flag after the focus manager processes it.
    pub fn clear_needs_focus_sync(&self) {
        self.needs_focus_sync.set(false);
    }

    /// Set this node's ID (called by applier after creation).
    pub fn set_node_id(&mut self, id: NodeId) {
        if let Some(existing) = self.id.replace(Some(id))
            && let Some(owner_context_id) = self.owner_context_id.take()
        {
            unregister_layout_node(owner_context_id, existing);
        }
        self.layout_state.borrow_mut().set_node_id(id);
        let owner_context_id = register_layout_node(id, self);
        self.owner_context_id.set(Some(owner_context_id));
        self.refresh_registry_state();

        self.modifier_chain.set_node_id(Some(id));
        let invalidations = self.modifier_chain.take_invalidations();
        self.dispatch_modifier_invalidations_with_prev(
            &invalidations,
            NodeCapabilities::empty(),
            false,
        );
        // The slices carry the node's id; they are collected again when next read.
        self.modifier_slices_dirty.set(true);
    }

    /// Get this node's ID.
    pub fn node_id(&self) -> Option<NodeId> {
        self.id.get()
    }

    /// Set this node's parent (called when node is added as child).
    /// Sets both folded_parent (direct) and parent (first non-virtual ancestor for bubbling).
    pub fn set_parent(&self, parent: NodeId) {
        self.folded_parent.set(Some(parent));
        self.parent.set(Some(parent));
        self.refresh_registry_state();
    }

    /// Clear this node's parent (called when node is removed from parent).
    pub fn clear_parent(&self) {
        self.folded_parent.set(None);
        self.parent.set(None);
        self.refresh_registry_state();
    }

    /// Get this node's parent for dirty flag bubbling (may skip virtual nodes).
    pub fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    /// Get this node's direct parent (may be a virtual node).
    pub fn folded_parent(&self) -> Option<NodeId> {
        self.folded_parent.get()
    }

    pub(crate) fn cache_handles(&self) -> &LayoutNodeCacheHandles {
        &self.cache
    }

    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.modifier_chain.resolved_modifiers()
    }

    pub fn modifier_capabilities(&self) -> NodeCapabilities {
        self.modifier_chain.capabilities()
    }

    /// Whether this node's modifier chain makes it the root of a separate
    /// window. Its parent lays out as if it had no size and its parent's
    /// scene skips it; its own scene starts here.
    pub fn is_window_root(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::WINDOW_ROOT)
    }

    pub fn modifier_child_capabilities(&self) -> NodeCapabilities {
        self.modifier_chain.aggregate_child_capabilities()
    }

    pub fn set_debug_modifiers(&mut self, enabled: bool) {
        self.debug_modifiers.set(enabled);
        self.modifier_chain.set_debug_logging(enabled);
    }

    pub fn debug_modifiers_enabled(&self) -> bool {
        self.debug_modifiers.get()
    }

    /// The node's modifier locals, or `None` while its modifiers have never
    /// provided or read one.
    pub fn modifier_locals_handle(&self) -> Option<ModifierLocalsHandle> {
        self.modifier_chain.modifier_locals_handle()
    }

    pub fn has_layout_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::LAYOUT)
    }

    pub fn has_draw_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::DRAW)
    }

    pub fn has_pointer_input_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::POINTER_INPUT)
    }

    pub fn has_semantics_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::SEMANTICS)
    }

    pub fn has_focus_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::FOCUS)
    }

    fn refresh_registry_state(&self) {
        if let (Some(id), Some(owner_context_id)) = (self.id.get(), self.owner_context_id.get()) {
            let parent = self.parent();
            let capabilities = self.modifier_child_capabilities();
            let modifier_locals = self.modifier_locals_handle();
            let _ = crate::render_state::with_layout_node_registry_by_app_context(
                owner_context_id,
                |registry| {
                    registry.update_entry(id, parent, capabilities, modifier_locals);
                },
            );
        }
    }

    pub fn modifier_slices_snapshot(&self) -> Rc<ModifierNodeSlices> {
        if self.modifier_slices_dirty.get() {
            self.update_modifier_slices_cache();
        }
        self.modifier_slices_snapshot.borrow().clone()
    }

    /// Returns a clone of the current layout state.
    pub fn layout_state(&self) -> LayoutState {
        self.layout_state.borrow().clone()
    }

    /// Returns the measured size of this node.
    pub fn measured_size(&self) -> Size {
        self.layout_state.borrow().size
    }

    /// Returns the position of this node relative to its parent.
    pub fn position(&self) -> Point {
        self.layout_state.borrow().position
    }

    /// Returns true if this node has been placed in the current layout pass.
    pub fn is_placed(&self) -> bool {
        self.layout_state.borrow().is_placed
    }

    /// Updates the measured size of this node. Called during measurement.
    /// [`LayoutState::set_size`] self-reports actual changes to the scene
    /// phase.
    pub fn set_measured_size(&self, size: Size) {
        self.layout_state.borrow_mut().set_size(size);
    }

    /// Updates the position of this node. Called during placement.
    /// [`LayoutState::place`] self-reports actual moves to the scene phase.
    pub fn set_position(&self, position: Point) {
        self.layout_state.borrow_mut().place(position);
    }

    /// Records the content offset (e.g. from padding).
    pub fn set_content_offset(&self, offset: Point) {
        self.layout_state.borrow_mut().set_content_offset(offset);
    }

    /// Clears the is_placed flag. Called at the start of a layout pass.
    pub fn clear_placed(&self) {
        self.layout_state.borrow_mut().clear_placed();
    }

    fn note_semantics_layout_change(&self) {
        if let Ok(mut state) = self.layout_state.try_borrow_mut() {
            state.note_semantics_change();
        }
    }

    pub fn semantics_configuration(&self) -> Option<SemanticsConfiguration> {
        crate::modifier::collect_semantics_from_chain(self.modifier_chain.chain())
    }

    /// Drops the cached reach and queues the node for the modal count.
    fn forget_semantics_reach(&self) {
        self.semantics_reach.set(None);
        crate::modal_nodes::reach_changed(self.id.get());
    }

    /// Whether this node's modifiers make it modal or hidden.
    pub fn semantics_reach(&self) -> cranpose_foundation::SemanticsReach {
        // A chain without semantics reaches nothing, known without touching it.
        if !self
            .modifier_capabilities()
            .contains(NodeCapabilities::SEMANTICS)
        {
            return cranpose_foundation::SemanticsReach::default();
        }
        if let Some(reach) = self.semantics_reach.get() {
            return reach;
        }
        let reach = crate::modifier::semantics_reach_of_chain(self.modifier_chain.chain());
        self.semantics_reach.set(Some(reach));
        reach
    }

    pub(crate) fn modifier_chain(&self) -> &ModifierChainHandle {
        &self.modifier_chain
    }

    /// Access the text field modifier node (if present) with a mutable callback.
    ///
    /// This is used for keyboard event dispatch to text fields.
    /// Returns `None` if no text field modifier is found in the chain.
    pub fn with_text_field_modifier_mut<R>(
        &mut self,
        f: impl FnMut(&mut crate::TextFieldModifierNode) -> R,
    ) -> Option<R> {
        self.modifier_chain.with_text_field_modifier_mut(f)
    }

    /// Returns the handle to the shared layout state, which layout clones
    /// to update the state without borrowing the Applier.
    pub fn layout_state_handle(&self) -> &Rc<RefCell<LayoutState>> {
        &self.layout_state
    }

    pub(crate) fn coordinator_geometry(&self) -> &crate::modifier::CoordinatorGeometry {
        &self.coordinator_geometry
    }

    pub(crate) fn layout_runtime_state_handle(&self) -> Rc<RefCell<LayoutRuntimeState>> {
        self.layout_runtime_state.clone()
    }

    #[cfg(test)]
    pub(crate) fn layout_runtime_debug_stats(&self) -> LayoutRuntimeDebugStats {
        self.layout_runtime_state.borrow().debug_stats()
    }
}
impl Clone for LayoutNode {
    fn clone(&self) -> Self {
        let mut node = Self {
            #[cfg(feature = "inspection")]
            source_trace: self.source_trace.clone(),
            modifier: self.modifier.clone(),
            modifier_chain: ModifierChainHandle::for_layout_direction(
                self.modifier_chain.layout_direction(),
            ),
            measure_policy: self.measure_policy.clone(),
            density: self.density,
            children: self.children.clone(),
            cache: self.cache.clone(),
            needs_measure: Cell::new(self.needs_measure.get()),
            needs_layout: Cell::new(self.needs_layout.get()),
            descendant_dirt: Cell::new(self.descendant_dirt.get()),
            needs_semantics: Cell::new(self.needs_semantics.get()),
            descendant_needs_semantics: Cell::new(self.descendant_needs_semantics.get()),
            semantics_reach: Cell::new(None),
            needs_redraw: Cell::new(self.needs_redraw.get()),
            needs_pointer_pass: Cell::new(self.needs_pointer_pass.get()),
            needs_focus_sync: Cell::new(self.needs_focus_sync.get()),
            parent: Cell::new(self.parent.get()),
            folded_parent: Cell::new(self.folded_parent.get()),
            id: Cell::new(None),
            owner_context_id: Cell::new(None),
            debug_modifiers: Cell::new(self.debug_modifiers.get()),
            is_virtual: self.is_virtual,
            virtual_children_count: Cell::new(self.virtual_children_count.get()),
            modifier_slices_snapshot: RefCell::new(Rc::default()),
            modifier_slices_dirty: Cell::new(true),
            layout_state: self.layout_state.clone(),
            layout_runtime_state: self.layout_runtime_state.clone(),
            coordinator_geometry: Rc::clone(&self.coordinator_geometry),
        };
        node.sync_modifier_chain(false);
        node
    }
}

impl Node for LayoutNode {
    fn mount(&mut self) {
        let (chain, mut context) = self.modifier_chain.chain_and_context_mut();
        chain.repair_chain();
        chain.attach_nodes(&mut *context);
        crate::modal_nodes::reach_changed(self.id.get());
    }

    fn unmount(&mut self) {
        self.modifier_chain.chain_mut().detach_nodes();
    }

    fn set_node_id(&mut self, id: NodeId) {
        LayoutNode::set_node_id(self, id);
    }

    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
        }
        if is_virtual_node(child) {
            let count = self.virtual_children_count.get();
            self.virtual_children_count.set(count + 1);
        }
        self.children.push(child);
        self.cache.clear();
        self.mark_needs_measure();
        self.note_semantics_layout_change();
        true
    }

    fn remove_child(&mut self, child: NodeId) -> bool {
        let before = self.children.len();
        self.children.retain(|&id| id != child);
        let removed = self.children.len() < before;
        if removed {
            if is_virtual_node(child) {
                let count = self.virtual_children_count.get();
                if count > 0 {
                    self.virtual_children_count.set(count - 1);
                }
            }
            self.cache.clear();
            self.mark_needs_measure();
            self.note_semantics_layout_change();
        }
        removed
    }

    fn move_child(&mut self, from: usize, to: usize) {
        if from == to || from >= self.children.len() {
            return;
        }
        let child = self.children.remove(from);
        let target = to.min(self.children.len());
        self.children.insert(target, child);
        self.cache.clear();
        self.mark_needs_measure();
        self.note_semantics_layout_change();
    }

    fn update_children(&mut self, children: &[NodeId]) {
        self.children.clear();
        self.children.extend_from_slice(children);
        self.cache.clear();
        self.mark_needs_measure();
        self.note_semantics_layout_change();
    }

    fn collect_children_into(&self, out: &mut smallvec::SmallVec<[NodeId; 8]>) {
        out.clear();
        out.extend_from_slice(&self.children);
    }

    fn owned_child_index(&self, child: NodeId) -> Option<usize> {
        self.children.iter().position(|&id| id == child)
    }

    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.set_parent(parent);
    }

    fn on_removed_from_parent(&mut self) {
        self.clear_parent();
    }

    fn parent(&self) -> Option<NodeId> {
        self.parent.get()
    }

    fn mark_needs_layout(&self) {
        self.needs_layout.set(true);
    }

    fn needs_layout(&self) -> bool {
        self.needs_layout.get()
    }

    fn mark_needs_measure(&self) {
        self.needs_measure.set(true);
        self.needs_layout.set(true);
    }

    fn needs_measure(&self) -> bool {
        self.needs_measure.get()
    }

    fn mark_descendant_needs_layout(&self, measure: bool) {
        self.descendant_dirt
            .set(self.descendant_dirt.get().marked(measure));
        if measure {
            self.cache.forget_intrinsics();
        }
    }

    fn descendant_needs_layout(&self) -> bool {
        self.descendant_dirt.get().layout
    }

    fn is_virtual(&self) -> bool {
        self.is_virtual
    }

    fn descendant_needs_measure(&self) -> bool {
        self.descendant_dirt.get().measure
    }

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
        self.forget_semantics_reach();
    }

    fn mark_descendant_needs_semantics(&self) {
        self.descendant_needs_semantics.set(true);
    }

    fn needs_semantics(&self) -> bool {
        LayoutNode::needs_semantics(self)
    }

    fn set_parent_for_bubbling(&mut self, parent: NodeId) {
        if self.parent.get().is_none() {
            self.parent.set(Some(parent));
        }
    }

    fn recycle_key(&self) -> Option<TypeId> {
        Some(TypeId::of::<Self>())
    }

    fn recycle_pool_limit(&self) -> Option<usize> {
        Some(RECYCLED_LAYOUT_NODE_POOL_LIMIT)
    }

    fn prepare_for_recycle(&mut self) {
        *self = Self::new_recycled_shell(self.is_virtual);
    }

    fn rehouse_for_recycle(&self) -> Option<Box<dyn cranpose_core::Node>> {
        Some(Box::new(Self::new_recycled_shell(self.is_virtual)))
    }

    fn rehouse_for_live_compaction(&mut self) -> Option<Box<dyn cranpose_core::Node>> {
        let mut previous = std::mem::replace(self, Self::new_recycled_shell(self.is_virtual));
        let node_id = previous.id.replace(None);
        let parent = previous.parent.get();
        let folded_parent = previous.folded_parent.get();
        let debug_modifiers = previous.debug_modifiers.get();
        let needs_measure = previous.needs_measure.get();
        let needs_layout = previous.needs_layout.get();
        let needs_semantics = previous.needs_semantics.get();
        let descendant_needs_semantics = previous.descendant_needs_semantics.get();
        let needs_redraw = previous.needs_redraw.get();
        let needs_pointer_pass = previous.needs_pointer_pass.get();
        let needs_focus_sync = previous.needs_focus_sync.get();
        let virtual_children_count = previous.virtual_children_count.get();
        let children = previous.children.to_vec();
        let modifier = previous.modifier.rehouse_for_live_compaction();
        let measure_policy = previous.measure_policy.clone();
        let layout_state = previous.layout_state.clone();
        let layout_runtime_state = previous.layout_runtime_state.clone();
        let coordinator_geometry = Rc::clone(&previous.coordinator_geometry);

        previous.modifier_chain.chain_mut().detach_nodes();

        let mut compact = Self::new_with_virtual(modifier, measure_policy, previous.is_virtual);
        compact.children = children;
        #[cfg(feature = "inspection")]
        {
            compact.source_trace = previous.source_trace.clone();
        }
        compact.parent.set(parent);
        compact.folded_parent.set(folded_parent);
        compact.id.set(node_id);
        compact.debug_modifiers.set(debug_modifiers);
        compact.needs_measure.set(needs_measure);
        compact.needs_layout.set(needs_layout);
        compact.needs_semantics.set(needs_semantics);
        compact
            .descendant_needs_semantics
            .set(descendant_needs_semantics);
        compact.needs_redraw.set(needs_redraw);
        compact.needs_pointer_pass.set(needs_pointer_pass);
        compact.needs_focus_sync.set(needs_focus_sync);
        compact.virtual_children_count.set(virtual_children_count);
        compact.layout_state = layout_state;
        compact.layout_runtime_state = layout_runtime_state;
        compact.coordinator_geometry = coordinator_geometry;
        compact.sync_modifier_chain(false);
        if let Some(id) = node_id {
            let owner_context_id = register_layout_node(id, &compact);
            compact.owner_context_id.set(Some(owner_context_id));
        }

        Some(Box::new(compact))
    }
}

impl Drop for LayoutNode {
    fn drop(&mut self) {
        if let (Some(id), Some(owner_context_id)) = (self.id.get(), self.owner_context_id.get()) {
            unregister_layout_node(owner_context_id, id);
        }
    }
}

const MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY: usize = 128;
const VIRTUAL_NODE_ID_START: NodeId = 0xC0000000;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LayoutNodeRegistryDebugStats {
    len: usize,
    capacity: usize,
}

struct LayoutNodeRegistryEntry {
    parent: Option<NodeId>,
    modifier_child_capabilities: NodeCapabilities,
    modifier_locals: Option<ModifierLocalsHandle>,
    is_virtual: bool,
}

pub(crate) struct LayoutNodeRegistryState {
    entries: RefCell<HashMap<NodeId, LayoutNodeRegistryEntry>>,
    virtual_node_id_counter: Cell<NodeId>,
}

impl LayoutNodeRegistryState {
    pub(crate) fn new() -> Self {
        Self {
            entries: RefCell::new(HashMap::new()),
            virtual_node_id_counter: Cell::new(VIRTUAL_NODE_ID_START),
        }
    }

    fn register(&self, id: NodeId, node: &LayoutNode) {
        self.entries.borrow_mut().insert(
            id,
            LayoutNodeRegistryEntry {
                parent: node.parent(),
                modifier_child_capabilities: node.modifier_child_capabilities(),
                modifier_locals: node.modifier_locals_handle(),
                is_virtual: node.is_virtual(),
            },
        );
    }

    fn unregister(&self, id: NodeId) {
        let mut entries = self.entries.borrow_mut();
        entries.remove(&id);
        let should_shrink = (entries.len() <= MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY
            && entries.capacity() > MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY)
            || entries.capacity()
                > entries
                    .len()
                    .max(MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY)
                    .saturating_mul(4);
        if should_shrink {
            let retained = entries
                .len()
                .max(MIN_RETAINED_LAYOUT_NODE_REGISTRY_CAPACITY);
            let mut rebuilt = HashMap::new();
            rebuilt.reserve(retained);
            rebuilt.extend(entries.drain());
            *entries = rebuilt;
        }
    }

    fn update_entry(
        &self,
        id: NodeId,
        parent: Option<NodeId>,
        modifier_child_capabilities: NodeCapabilities,
        modifier_locals: Option<ModifierLocalsHandle>,
    ) {
        if let Some(entry) = self.entries.borrow_mut().get_mut(&id) {
            entry.parent = parent;
            entry.modifier_child_capabilities = modifier_child_capabilities;
            entry.modifier_locals = modifier_locals;
        }
    }

    #[cfg(test)]
    fn stats(&self) -> LayoutNodeRegistryDebugStats {
        let entries = self.entries.borrow();
        LayoutNodeRegistryDebugStats {
            len: entries.len(),
            capacity: entries.capacity(),
        }
    }

    fn is_virtual_node(&self, id: NodeId) -> bool {
        self.entries
            .borrow()
            .get(&id)
            .is_some_and(|entry| entry.is_virtual)
    }

    fn allocate_virtual_node_id(&self) -> NodeId {
        let id = self.virtual_node_id_counter.get();
        self.virtual_node_id_counter.set(id.wrapping_add(1));
        id
    }

    fn resolve_modifier_local_from_parent_chain(
        &self,
        start: Option<NodeId>,
        token: &ModifierLocalToken,
    ) -> Option<ResolvedModifierLocal> {
        let mut current = start;
        while let Some(parent_id) = current {
            let (next_parent, resolved) = {
                let entries = self.entries.borrow();
                if let Some(entry) = entries.get(&parent_id) {
                    let resolved = entry
                        .modifier_child_capabilities
                        .contains(NodeCapabilities::MODIFIER_LOCALS)
                        .then_some(entry.modifier_locals.as_ref())
                        .flatten()
                        .and_then(|locals| locals.borrow().resolve(token))
                        .map(|value| value.with_source(ModifierLocalSource::Ancestor));
                    (entry.parent, resolved)
                } else {
                    (None, None)
                }
            };
            if let Some(value) = resolved {
                return Some(value);
            }
            current = next_parent;
        }
        None
    }
}

pub(crate) fn register_layout_node(
    id: NodeId,
    node: &LayoutNode,
) -> crate::render_state::AppContextId {
    let owner_context_id = crate::render_state::current_app_context_id();
    let _ = crate::render_state::with_layout_node_registry_by_app_context(
        owner_context_id,
        |registry| {
            registry.register(id, node);
        },
    );
    owner_context_id
}

pub(crate) fn unregister_layout_node(
    owner_context_id: crate::render_state::AppContextId,
    id: NodeId,
) {
    let _ = crate::render_state::with_layout_node_registry_by_app_context(
        owner_context_id,
        |registry| {
            registry.unregister(id);
        },
    );
}

#[cfg(test)]
fn layout_node_registry_stats() -> LayoutNodeRegistryDebugStats {
    crate::render_state::with_layout_node_registry(LayoutNodeRegistryState::stats)
}

pub(crate) fn is_virtual_node(id: NodeId) -> bool {
    crate::render_state::with_layout_node_registry(|registry| registry.is_virtual_node(id))
}

pub(crate) fn allocate_virtual_node_id() -> NodeId {
    crate::render_state::with_layout_node_registry(
        LayoutNodeRegistryState::allocate_virtual_node_id,
    )
}

fn resolve_modifier_local_from_parent_chain(
    start: Option<NodeId>,
    token: &ModifierLocalToken,
) -> Option<ResolvedModifierLocal> {
    crate::render_state::with_layout_node_registry(|registry| {
        registry.resolve_modifier_local_from_parent_chain(start, token)
    })
}

#[cfg(test)]
#[path = "tests/layout_node_tests.rs"]
mod tests;
