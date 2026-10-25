use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
};

use cranpose_core::{Node, NodeId, collections::map::HashMap};
use cranpose_foundation::{
    InvalidationKind, ModifierInvalidation, ModifierInvalidations, NodeCapabilities,
    SemanticsConfiguration,
};
use cranpose_ui_layout::{Constraints, MeasurePolicy};

#[cfg(test)]
use crate::layout::LayoutRuntimeDebugStats;
use crate::{
    layout::{LayoutRuntimeState, MeasuredNode},
    modifier::{
        InPlaceUpdate, Modifier, ModifierChainHandle, ModifierLocalSource, ModifierLocalToken,
        ModifierLocalsHandle, ModifierNodeSlices, Point, ResolvedModifierLocal, ResolvedModifiers,
        Size,
    },
    render_state::AppContext,
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
pub(crate) struct PlacementPass {
    id: Cell<u64>,
    unplaced: Cell<usize>,
}

impl PlacementPass {
    pub(crate) const fn new() -> Self {
        Self {
            id: Cell::new(1),
            unplaced: Cell::new(0),
        }
    }

    /// Starts the placement bookkeeping of a layout pass.
    pub(crate) fn begin(&self) {
        self.id.set(self.id.get() + 1);
        self.unplaced.set(0);
    }

    /// The running pass, when it has left a node unplaced that it found placed.
    pub(crate) fn with_unplaced_nodes(&self) -> Option<u64> {
        (self.unplaced.get() > 0).then(|| self.id.get())
    }

    /// Counts a placed node the running pass cleared, and returns the pass.
    fn clear(&self) -> u64 {
        self.unplaced.set(self.unplaced.get() + 1);
        self.id.get()
    }

    /// Whether a node cleared in pass `cleared_in` was cleared by the running
    /// pass, which then counts it placed again.
    fn restore(&self, cleared_in: u64) -> bool {
        let restored = cleared_in == self.id.get();
        if restored {
            self.unplaced.set(self.unplaced.get().saturating_sub(1));
        }
        restored
    }
}

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

/// Whether a layout node, or a node below it, needs measure or layout.
#[derive(Clone, Copy)]
pub(crate) struct LayoutDirt {
    pub(crate) measure: bool,
    pub(crate) layout: bool,
    pub(crate) descendant: DescendantDirt,
}

impl LayoutDirt {
    /// Nothing at or below the node needs measure or layout.
    pub(crate) const CLEAN: Self = Self {
        measure: false,
        layout: false,
        descendant: DescendantDirt {
            layout: false,
            measure: false,
        },
    };

    /// Whether the node or a node below it needs layout.
    pub(crate) fn any(self) -> bool {
        self.measure || self.layout || self.descendant.layout
    }
}

/// A new node needs a measure, and a cache whose node keeps its flags
/// elsewhere reads as dirty.
impl Default for LayoutDirt {
    fn default() -> Self {
        Self {
            measure: true,
            layout: true,
            descendant: DescendantDirt::default(),
        }
    }
}

/// The measured size and placed position that a layout node kept from its latest layout pass.
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
    /// The take of the moved-node list this node was last noted in.
    moved_noted_in: u64,
    /// The take of the reshaped-node list this node was last noted in.
    reshaped_noted_in: u64,
}

impl LayoutState {
    /// Returns the measured size, in logical pixels.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Returns the placed position relative to the parent, in logical pixels.
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

    /// Returns whether layout has placed the node.
    pub fn is_placed(&self) -> bool {
        self.is_placed
    }

    pub(crate) fn set_node_id(&mut self, node_id: NodeId) {
        self.node_id = Some(node_id);
    }

    /// Writes the measured size, self-reporting an actual change to the
    /// scene phase of `context`, the node's app context.
    pub fn set_size(&mut self, size: Size, context: &AppContext) {
        if self.size != size {
            if let Some(id) = self.node_id {
                context.note_geometry_scene_node(id, &mut self.reshaped_noted_in);
            }
            self.size = size;
            self.note_semantics_change();
        }
    }

    /// Writes the placed position and marks the node placed, self-reporting
    /// to the scene phase of `context`, the node's app context, an actual
    /// move, and a placement of a node the scene does not draw: one the
    /// running pass did not find placed. A move is reported apart, with
    /// `placer`, the node whose placement put the node there, so the scene
    /// can keep what it drew for the node.
    pub fn place(&mut self, position: Point, placer: Option<NodeId>, context: &AppContext) {
        // A node the running pass cleared while it was placed changes
        // nothing the scene draws by being placed again.
        let newly_placed =
            !self.is_placed && !context.placement_pass().restore(self.cleared_in_pass);
        let moved = self.position != position;
        if let Some(id) = self.node_id {
            if newly_placed {
                context.note_geometry_scene_node(id, &mut self.reshaped_noted_in);
            } else if moved {
                context.record_moved_scene_node(id, placer, &mut self.moved_noted_in);
            }
        }
        self.position = position;
        if moved || !self.is_placed {
            self.is_placed = true;
            self.note_semantics_change();
        }
    }

    /// Clears the placed flag at the start of a layout pass of `context`.
    /// Until its parent places it again the pass counts the node unplaced,
    /// and the pass names it to the scene phase if it ends that way.
    pub fn clear_placed(&mut self, context: &AppContext) {
        if !self.is_placed {
            return;
        }
        self.is_placed = false;
        self.cleared_in_pass = context.placement_pass().clear();
        self.note_semantics_change();
    }

    /// Whether `pass` found this node placed and left it unplaced.
    pub(crate) fn unplaced_in(&self, pass: u64) -> bool {
        !self.is_placed && self.cleared_in_pass == pass
    }

    #[inline]
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
    /// The other incoming constraints the measurement holds for.
    hold: Option<cranpose_ui_layout::ConstraintsHold>,
    /// Whether no child's size entered the measurement: see
    /// [`cranpose_ui_layout::MeasurePolicy::ignores_child_sizes`].
    ignores_child_sizes: bool,
}

/// Selects which intrinsic size of a node to query, together with the size on the other axis.
#[derive(Clone, Copy, Debug)]
pub enum IntrinsicKind {
    /// Minimum intrinsic width for the given height, in logical pixels.
    MinWidth(f32),
    /// Maximum intrinsic width for the given height, in logical pixels.
    MaxWidth(f32),
    /// Minimum intrinsic height for the given width, in logical pixels.
    MinHeight(f32),
    /// Maximum intrinsic height for the given width, in logical pixels.
    MaxHeight(f32),
}

impl IntrinsicKind {
    fn discriminant(&self) -> u8 {
        match self {
            Self::MinWidth(_) => 0,
            Self::MaxWidth(_) => 1,
            Self::MinHeight(_) => 2,
            Self::MaxHeight(_) => 3,
        }
    }

    fn value_bits(&self) -> u32 {
        match self {
            Self::MinWidth(value)
            | Self::MaxWidth(value)
            | Self::MinHeight(value)
            | Self::MaxHeight(value) => value.to_bits(),
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
    /// The measurement the cache last let go of. It is never served: the
    /// node's next measure takes it to fill its memory in place of new
    /// allocations.
    spare: Option<Rc<MeasuredNode>>,
    intrinsics: Vec<(IntrinsicKind, f32)>,
    /// Whether a parent read intrinsic sizes since the cache was cleared.
    intrinsics_read: bool,
}

impl NodeCacheState {
    fn retire_measurement(&mut self) {
        if let Some(entry) = self.measurement.take() {
            self.spare = Some(entry.measured);
        }
    }
}

/// What a node's cache shares with its parent's record of the node: the
/// cache and the node's dirty flags, which the parent's keep walk reads
/// there without looking the node up.
#[derive(Default)]
struct CacheShared {
    cache: RefCell<NodeCacheState>,
    dirt: Cell<LayoutDirt>,
}

#[derive(Default)]
pub(crate) struct LayoutNodeCacheHandles {
    state: Rc<CacheShared>,
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
    /// The flags of the node whose cache this is: dirty for a node that
    /// keeps its flags elsewhere.
    pub(crate) fn dirt(&self) -> LayoutDirt {
        self.state.dirt.get()
    }

    fn set_dirt(&self, dirt: LayoutDirt) {
        self.state.dirt.set(dirt);
    }

    /// Clears the node's layout flag and its descendants' flags, and returns
    /// the flags as they were, which [`Self::restore_dirt`] puts back.
    pub(crate) fn take_layout_dirt(&self) -> LayoutDirt {
        let dirt = self.dirt();
        self.clear_layout_dirt();
        dirt
    }

    /// Puts back flags [`Self::take_layout_dirt`] took.
    pub(crate) fn restore_dirt(&self, dirt: LayoutDirt) {
        self.set_dirt(dirt);
    }

    /// Clears the node's layout flag and its descendants' flags: the pass
    /// laid it out.
    pub(crate) fn clear_layout_dirt(&self) {
        self.set_dirt(LayoutDirt {
            layout: false,
            descendant: DescendantDirt::default(),
            ..self.dirt()
        });
    }

    pub(crate) fn clear(&self) {
        let mut state = self.state.cache.borrow_mut();
        state.retire_measurement();
        state.intrinsics.clear();
        state.intrinsics_read = false;
        state.epoch = 0;
    }

    pub(crate) fn activate(&self, epoch: u64) {
        let mut state = self.state.cache.borrow_mut();
        if state.epoch != epoch {
            state.retire_measurement();
            state.intrinsics.clear();
            state.intrinsics_read = false;
            state.epoch = epoch;
        }
    }

    /// The node's last measurement, whatever constraints it was made under:
    /// the one the cache holds, or else the one it last let go of.
    pub(crate) fn held_measurement(&self) -> Option<Rc<MeasuredNode>> {
        let state = self.state.cache.borrow();
        state
            .measurement
            .as_ref()
            .map(|entry| &entry.measured)
            .or(state.spare.as_ref())
            .cloned()
    }

    /// Takes the measurement the cache last let go of, or else the one it
    /// holds, for a measure of the node that replaces it.
    pub(crate) fn take_previous_measurement(&self) -> Option<Rc<MeasuredNode>> {
        let mut state = self.state.cache.borrow_mut();
        state
            .spare
            .take()
            .or_else(|| state.measurement.take().map(|entry| entry.measured))
    }

    /// Drops the intrinsic sizes, which depend on every node below: one of
    /// them changed. A parent's earlier read still counts for
    /// [`Self::has_intrinsics`].
    pub(crate) fn forget_intrinsics(&self) {
        self.state.cache.borrow_mut().intrinsics.clear();
    }

    pub(crate) fn epoch(&self) -> u64 {
        self.state.cache.borrow().epoch
    }

    /// The measurement the cache holds for `constraints`: one made under
    /// them, or one whose hold contains them. A caller asks only while the
    /// node and everything below it are clean.
    pub(crate) fn get_measurement(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        let state = self.state.cache.borrow();
        state
            .measurement
            .as_ref()
            .filter(|entry| {
                entry.constraints == constraints
                    || entry.hold.is_some_and(|hold| hold.contains(constraints))
            })
            .map(|entry| Rc::clone(&entry.measured))
    }

    /// The constraints of the measurement the cache holds.
    pub(crate) fn measured_constraints(&self) -> Option<Constraints> {
        self.state
            .cache
            .borrow()
            .measurement
            .as_ref()
            .map(|entry| entry.constraints)
    }

    /// Whether a parent read intrinsic sizes from this cache.
    pub(crate) fn has_intrinsics(&self) -> bool {
        self.state.cache.borrow().intrinsics_read
    }

    /// The hold of the measurement the cache has, when it is `measured`.
    pub(crate) fn hold_of(
        &self,
        measured: &Rc<MeasuredNode>,
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        self.state
            .cache
            .borrow()
            .measurement
            .as_ref()
            .filter(|entry| Rc::ptr_eq(&entry.measured, measured))
            .and_then(|entry| entry.hold)
    }

    /// Stores `measured` for `constraints`. The hold of a measurement the
    /// cache already has stays with it, as does whether it ignores its
    /// children's sizes.
    pub(crate) fn store_measurement(&self, constraints: Constraints, measured: Rc<MeasuredNode>) {
        let mut state = self.state.cache.borrow_mut();
        let (hold, ignores_child_sizes) = state
            .measurement
            .as_ref()
            .filter(|entry| Rc::ptr_eq(&entry.measured, &measured))
            .map_or((None, false), |entry| {
                (entry.hold, entry.ignores_child_sizes)
            });
        state.measurement = Some(MeasurementCacheEntry {
            constraints,
            measured,
            hold,
            ignores_child_sizes,
        });
    }

    /// Stores `measured` for `constraints` and the other constraints `hold`
    /// says it holds for.
    pub(crate) fn store_held_measurement(
        &self,
        constraints: Constraints,
        measured: Rc<MeasuredNode>,
        hold: Option<cranpose_ui_layout::ConstraintsHold>,
        ignores_child_sizes: bool,
    ) {
        self.state.cache.borrow_mut().measurement = Some(MeasurementCacheEntry {
            constraints,
            measured,
            hold,
            ignores_child_sizes,
        });
    }

    /// Stores `measured`, a measurement the cache has, for `constraints`
    /// alone: a child of it measured to another size, which the other
    /// constraints its hold names may not take.
    pub(crate) fn store_resized_measurement(
        &self,
        constraints: Constraints,
        measured: Rc<MeasuredNode>,
    ) {
        self.store_held_measurement(constraints, measured, None, true);
    }

    /// Runs `read` on the measurement the cache holds, with whether it
    /// ignores its children's sizes; `None` while it holds none.
    pub(crate) fn read_measurement<R>(
        &self,
        read: impl FnOnce(&MeasuredNode, bool) -> R,
    ) -> Option<R> {
        let state = self.state.cache.borrow();
        state
            .measurement
            .as_ref()
            .map(|entry| read(&entry.measured, entry.ignores_child_sizes))
    }

    /// Whether `measured`, the measurement the cache has, ignores its
    /// children's sizes.
    pub(crate) fn ignores_child_sizes_of(&self, measured: &Rc<MeasuredNode>) -> bool {
        self.state
            .cache
            .borrow()
            .measurement
            .as_ref()
            .is_some_and(|entry| entry.ignores_child_sizes && Rc::ptr_eq(&entry.measured, measured))
    }

    pub(crate) fn get_intrinsic(&self, kind: &IntrinsicKind) -> Option<f32> {
        let state = self.state.cache.borrow();
        state
            .intrinsics
            .iter()
            .find(|(stored_kind, _)| stored_kind == kind)
            .map(|(_, value)| *value)
    }

    pub(crate) fn store_intrinsic(&self, kind: IntrinsicKind, value: f32) {
        let mut state = self.state.cache.borrow_mut();
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

/// A node id or none in one word, where `Cell<Option<NodeId>>` takes two: a
/// layout node keeps three, and the 24 bytes kept it out of jemalloc's
/// 640-byte size class.
struct NodeIdCell(Cell<NodeId>);

/// The id a [`NodeIdCell`] holds for none: no node gets it, as ids index
/// the applier's nodes.
const NO_NODE_ID: NodeId = NodeId::MAX;

impl NodeIdCell {
    fn new(id: Option<NodeId>) -> Self {
        Self(Cell::new(id.unwrap_or(NO_NODE_ID)))
    }

    fn get(&self) -> Option<NodeId> {
        let id = self.0.get();
        (id != NO_NODE_ID).then_some(id)
    }

    fn set(&self, id: Option<NodeId>) {
        debug_assert_ne!(id, Some(NO_NODE_ID), "no node takes the none id");
        self.0.set(id.unwrap_or(NO_NODE_ID));
    }

    fn replace(&self, id: Option<NodeId>) -> Option<NodeId> {
        let previous = self.get();
        self.set(id);
        previous
    }
}

/// A node of the layout tree.
///
/// Its fields keep the order they are declared in: what a layout pass reads
/// for every child it binds comes first and fits one cache line, then what
/// measuring the node reads, then what composition reads. Layout reads
/// thousands of nodes a frame, and on a phone its cost is the lines it loads.
#[repr(C)]
pub struct LayoutNode {
    cache: LayoutNodeCacheHandles,
    layout_state: Rc<RefCell<LayoutState>>,
    layout_runtime_state: Rc<RefCell<LayoutRuntimeState>>,
    /// The actual children of this node (folded view - includes virtual nodes as-is)
    pub children: Vec<NodeId>,

    /// Policy that measures and places the children of this node. Change it through
    /// `set_measure_policy` so that a measure is scheduled.
    pub measure_policy: Rc<dyn MeasurePolicy>,
    /// Where the chain's layout modifiers put their content, written by
    /// layout and read by the draws and text the slices collect.
    coordinator_geometry: Rc<crate::modifier::CoordinatorGeometry>,
    density: crate::density::Density,
    is_virtual: bool,
    needs_semantics: Cell<bool>,
    /// The semantics tree has to read this node again, though its own
    /// semantics are unchanged: a node below it changed, or its placement
    /// or children did.
    descendant_needs_semantics: Cell<bool>,
    needs_redraw: Cell<bool>,
    needs_pointer_pass: Cell<bool>,
    needs_focus_sync: Cell<bool>,
    modifier_slices_dirty: Cell<bool>,
    debug_modifiers: Cell<bool>,

    id: NodeIdCell,
    parent: NodeIdCell,
    folded_parent: NodeIdCell,
    modifier_slices_snapshot: crate::modifier::SlicesSnapshot,

    /// The chain's modal and hidden flags, read by the modal count and the
    /// modal walk: dropped whenever the chain syncs or semantics are
    /// invalidated, the two ways its semantics change.
    semantics_reach: Cell<Option<cranpose_foundation::SemanticsReach>>,
    owner_context_id: Cell<Option<crate::render_state::AppContextId>>,
    modifier_chain: ModifierChainHandle,
    /// The modifier of this node. Change it through `set_modifier` so that the modifier chain
    /// updates.
    pub modifier: Modifier,
    /// Composable definitions responsible for this node, captured only for inspection builds.
    #[cfg(feature = "inspection")]
    pub source_trace: Rc<[cranpose_core::source_trace::SourceLocation]>,
}

pub(crate) const RECYCLED_LAYOUT_NODE_POOL_LIMIT: usize = 128;

pub(crate) fn empty_measure_policy() -> Rc<dyn MeasurePolicy> {
    crate::thread_state::with(|state| {
        Rc::clone(
            state
                .empty_measure_policy
                .get_or_init(|| Rc::new(crate::layout::policies::EmptyMeasurePolicy)),
        )
    })
}

impl LayoutNode {
    /// Creates a node with `modifier` whose children `measure_policy` measures and places.
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
        shell.cache.set_dirt(LayoutDirt::CLEAN);
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
            modifier_chain: ModifierChainHandle::detached(),
            measure_policy,
            density: crate::density::Density::default(),
            children: Vec::new(),
            cache: LayoutNodeCacheHandles::default(),
            needs_semantics: Cell::new(true),
            descendant_needs_semantics: Cell::new(false),
            semantics_reach: Cell::new(None),
            needs_redraw: Cell::new(true),
            needs_pointer_pass: Cell::new(false),
            needs_focus_sync: Cell::new(false),
            parent: NodeIdCell::new(None),
            folded_parent: NodeIdCell::new(None),
            id: NodeIdCell::new(None),
            owner_context_id: Cell::new(None),
            debug_modifiers: Cell::new(false),
            is_virtual,
            modifier_slices_snapshot: crate::modifier::SlicesSnapshot::default(),
            modifier_slices_dirty: Cell::new(true),
            layout_state: Rc::new(RefCell::new(LayoutState::default())),
            layout_runtime_state,
            coordinator_geometry: Rc::default(),
        };
        node.sync_modifier_chain(None);
        node
    }

    /// Replaces the node's modifier and updates its modifier chain, invalidating the work that the
    /// change affects.
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
        // A change only in elements nothing else reads, as a ticker's text
        // or an animated size, keeps the slices the chain gave.
        let update = self.modifier.in_place_update(&modifier);
        let modifier_changed = update.is_some() || !self.modifier.structural_eq(&modifier);
        self.modifier = modifier;
        self.sync_modifier_chain(update);
        if modifier_changed {
            self.cache.clear();
            self.request_semantics_update();
        }
    }

    /// Reconciles the chain with the node's modifier. `in_place` says how
    /// far the change reaches when the modifier changed only in elements the
    /// chain can update in place: when it does, the current slices are kept.
    fn sync_modifier_chain(&mut self, in_place: Option<InPlaceUpdate>) {
        let prev_caps = self.modifier_capabilities();
        let start_parent = self.parent();
        let mut resolver = move |token: &ModifierLocalToken| {
            resolve_modifier_local_from_parent_chain(start_parent, token)
        };
        self.modifier_chain
            .set_debug_logging(self.debug_modifiers.get());
        let mut context = self.modifier_chain.context(self.id.get());
        let in_place = in_place.is_some_and(|update| {
            self.modifier_chain
                .update_elements_in_place(&self.modifier, update, &mut context)
        });
        let modifier_local_invalidations = if in_place {
            ModifierInvalidations::new()
        } else {
            self.modifier_chain
                .update_with_resolver(&self.modifier, &mut resolver, &mut context)
        };
        if prev_caps.contains(NodeCapabilities::WINDOW_ROOT)
            != self
                .modifier_capabilities()
                .contains(NodeCapabilities::WINDOW_ROOT)
        {
            self.note_semantics_layout_change();
        }
        // An element updated in place reaches nothing, so the update leaves
        // the chain's reach as it was, and the modal count does not read it
        // again.
        if !in_place {
            self.forget_semantics_reach();
        }
        let keep_slices = in_place && !self.modifier_slices_dirty.get();
        if !keep_slices {
            self.modifier_slices_dirty.set(true);
        }

        let mut invalidations = context.take_invalidations();
        invalidations.extend(modifier_local_invalidations);
        self.dispatch_modifier_invalidations_with_prev(&invalidations, prev_caps, keep_slices);
        // An update in place leaves the node's parent, capabilities and
        // modifier locals as the registry holds them.
        if !in_place {
            self.refresh_registry_state();
        }
    }

    fn update_modifier_slices_cache(&self) {
        self.modifier_slices_snapshot.collect(
            self.modifier_chain.chain(),
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

    /// Takes `policy` unless the node measures with an equal one already.
    pub(crate) fn update_measure_policy<P: MeasurePolicy + PartialEq>(&mut self, policy: P) {
        let current: &dyn std::any::Any = &*self.measure_policy;
        if current.downcast_ref::<P>() != Some(&policy) {
            self.set_measure_policy(Rc::new(policy));
        }
    }

    /// Replaces the measure policy and schedules a measure. Does nothing if `policy` is the same
    /// `Rc` as the current one.
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
        self.update_dirt(|dirt| LayoutDirt {
            measure: true,
            layout: true,
            ..dirt
        });
    }

    /// Mark this node as needing layout (but not necessarily measure).
    pub fn mark_needs_layout(&self) {
        self.update_dirt(|dirt| LayoutDirt {
            layout: true,
            ..dirt
        });
    }

    fn update_dirt(&self, update: impl FnOnce(LayoutDirt) -> LayoutDirt) {
        self.cache.set_dirt(update(self.cache.dirt()));
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
        self.cache.dirt().measure
    }

    /// Check if this node needs layout.
    pub fn needs_layout(&self) -> bool {
        self.cache.dirt().layout
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

    /// Clears the pending redraw flag once the node has been drawn.
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
        self.update_dirt(|dirt| LayoutDirt {
            measure: false,
            ..dirt
        });
    }

    /// Clears the node's layout flag and its descendants': a layout pass
    /// visited them.
    pub(crate) fn clear_needs_layout(&self) {
        self.cache.clear_layout_dirt();
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
        // A redraw asked for before the node had an id went to no draw
        // repass.
        if self.needs_redraw.get() {
            crate::render_state::note_retained_redraw(id);
        }
        let previous = self.id.replace(Some(id));
        if let Some(existing) = previous
            && let Some(owner_context_id) = self.owner_context_id.take()
        {
            unregister_layout_node(owner_context_id, existing);
        }
        self.layout_state.borrow_mut().set_node_id(id);
        // The new entry holds the node's parent, capabilities and modifier
        // locals as they are now.
        let owner_context_id = register_layout_node(id, self);
        self.owner_context_id.set(Some(owner_context_id));

        let mut context = self.modifier_chain.context(Some(id));
        self.modifier_chain.move_to_node(previous, &mut context);
        let invalidations = context.take_invalidations();
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
        // The registry holds the parent the node has; the same one again
        // changes nothing.
        if self.parent.get() == Some(parent) && self.folded_parent.get() == Some(parent) {
            return;
        }
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

    /// Returns the padding, layout properties and offset that the modifier chain resolved to.
    pub fn resolved_modifiers(&self) -> ResolvedModifiers {
        self.modifier_chain.resolved_modifiers()
    }

    /// What the chain tells the parent's measure policy about this node.
    pub(crate) fn parent_data(&self) -> cranpose_ui_layout::ParentData {
        self.modifier_chain.parent_data()
    }

    /// Returns the union of the capabilities of all nodes in the modifier chain.
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

    /// Returns the capabilities of the chain's nodes counted from the head of the chain. Compose's
    /// `aggregateChildKindSet`.
    pub fn modifier_child_capabilities(&self) -> NodeCapabilities {
        self.modifier_chain.aggregate_child_capabilities()
    }

    /// Turns debug logging of the modifier chain on or off.
    pub fn set_debug_modifiers(&mut self, enabled: bool) {
        self.debug_modifiers.set(enabled);
        self.modifier_chain.set_debug_logging(enabled);
    }

    /// Returns whether debug logging of the modifier chain is on.
    pub fn debug_modifiers_enabled(&self) -> bool {
        self.debug_modifiers.get()
    }

    /// The node's modifier locals, or `None` while its modifiers have never
    /// provided or read one.
    pub fn modifier_locals_handle(&self) -> Option<ModifierLocalsHandle> {
        self.modifier_chain.modifier_locals_handle()
    }

    /// Returns whether the modifier chain contains layout modifier nodes.
    pub fn has_layout_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::LAYOUT)
    }

    /// Returns whether the modifier chain contains draw modifier nodes.
    pub fn has_draw_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::DRAW)
    }

    /// Returns whether the modifier chain contains pointer input modifier nodes.
    pub fn has_pointer_input_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::POINTER_INPUT)
    }

    /// Returns whether the modifier chain contains semantics modifier nodes.
    pub fn has_semantics_modifier_nodes(&self) -> bool {
        self.modifier_capabilities()
            .contains(NodeCapabilities::SEMANTICS)
    }

    /// Returns whether the modifier chain contains focus modifier nodes.
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

    /// Returns the draw commands, pointer input handlers, text and graphics layer data collected
    /// from the modifier nodes. The snapshot is rebuilt after the chain changed.
    pub fn modifier_slices_snapshot(&self) -> Rc<ModifierNodeSlices> {
        if self.modifier_slices_dirty.get() {
            self.update_modifier_slices_cache();
        }
        self.modifier_slices_snapshot.get()
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
    pub fn set_measured_size(&self, size: Size, context: &AppContext) {
        self.layout_state.borrow_mut().set_size(size, context);
    }

    /// Notes to the scene phase of `context` that the layout pass changed
    /// the geometry of this node, `node_id`.
    pub(crate) fn note_geometry_change(&self, node_id: NodeId, context: &AppContext) {
        context.note_geometry_scene_node(
            node_id,
            &mut self.layout_state.borrow_mut().reshaped_noted_in,
        );
    }

    /// Updates the position of this node, which `placer` placed. Called
    /// during placement. [`LayoutState::place`] self-reports actual moves to
    /// the scene phase.
    pub fn set_position(&self, position: Point, placer: Option<NodeId>, context: &AppContext) {
        self.layout_state
            .borrow_mut()
            .place(position, placer, context);
    }

    /// Records the content offset (e.g. from padding).
    pub fn set_content_offset(&self, offset: Point) {
        self.layout_state.borrow_mut().set_content_offset(offset);
    }

    /// Clears the is_placed flag. Called at the start of a layout pass.
    pub fn clear_placed(&self, context: &AppContext) {
        self.layout_state.borrow_mut().clear_placed(context);
    }

    fn note_semantics_layout_change(&self) {
        if let Ok(mut state) = self.layout_state.try_borrow_mut() {
            state.note_semantics_change();
        }
    }

    /// Returns the semantics that the modifier chain reports, or `None` if it reports none.
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

    /// Returns the handle to the shared layout state, which layout clones
    /// to update the state without borrowing the Applier.
    pub fn layout_state_handle(&self) -> &Rc<RefCell<LayoutState>> {
        &self.layout_state
    }

    pub(crate) fn coordinator_geometry(&self) -> &crate::modifier::CoordinatorGeometry {
        &self.coordinator_geometry
    }

    pub(crate) fn layout_runtime_state_handle(&self) -> Rc<RefCell<LayoutRuntimeState>> {
        Rc::clone(&self.layout_runtime_state)
    }

    /// The node's layout runtime state, borrowed where
    /// [`Self::layout_runtime_state_handle`] would share it.
    pub(crate) fn layout_runtime_state(&self) -> &RefCell<LayoutRuntimeState> {
        &self.layout_runtime_state
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
            source_trace: Rc::clone(&self.source_trace),
            modifier: self.modifier.clone(),
            modifier_chain: ModifierChainHandle::for_layout_direction(
                self.modifier_chain.layout_direction(),
            ),
            measure_policy: Rc::clone(&self.measure_policy),
            density: self.density,
            children: self.children.clone(),
            cache: self.cache.clone(),
            needs_semantics: Cell::new(self.needs_semantics.get()),
            descendant_needs_semantics: Cell::new(self.descendant_needs_semantics.get()),
            semantics_reach: Cell::new(None),
            needs_redraw: Cell::new(self.needs_redraw.get()),
            needs_pointer_pass: Cell::new(self.needs_pointer_pass.get()),
            needs_focus_sync: Cell::new(self.needs_focus_sync.get()),
            parent: NodeIdCell::new(self.parent.get()),
            folded_parent: NodeIdCell::new(self.folded_parent.get()),
            id: NodeIdCell::new(None),
            owner_context_id: Cell::new(None),
            debug_modifiers: Cell::new(self.debug_modifiers.get()),
            is_virtual: self.is_virtual,
            modifier_slices_snapshot: crate::modifier::SlicesSnapshot::default(),
            modifier_slices_dirty: Cell::new(true),
            layout_state: Rc::clone(&self.layout_state),
            layout_runtime_state: Rc::clone(&self.layout_runtime_state),
            coordinator_geometry: Rc::clone(&self.coordinator_geometry),
        };
        node.sync_modifier_chain(None);
        node
    }
}

impl Node for LayoutNode {
    fn mount(&mut self) {
        self.modifier_chain.remount(self.id.get());
        crate::modal_nodes::reach_changed(self.id.get());
    }

    fn unmount(&mut self) {
        self.modifier_chain.chain_mut().detach_nodes();
    }

    fn set_node_id(&mut self, id: NodeId) {
        Self::set_node_id(self, id);
    }

    fn insert_child(&mut self, child: NodeId) -> bool {
        if self.children.contains(&child) {
            return false;
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

    fn for_each_child(&self, f: &mut dyn FnMut(NodeId)) {
        for &child in &self.children {
            f(child);
        }
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
        Self::mark_needs_layout(self);
    }

    fn needs_layout(&self) -> bool {
        Self::needs_layout(self)
    }

    fn mark_needs_measure(&self) {
        Self::mark_needs_measure(self);
    }

    fn needs_measure(&self) -> bool {
        Self::needs_measure(self)
    }

    fn mark_descendant_needs_layout(&self, measure: bool) {
        self.update_dirt(|dirt| LayoutDirt {
            descendant: dirt.descendant.marked(measure),
            ..dirt
        });
        if measure {
            self.cache.forget_intrinsics();
        }
    }

    fn descendant_needs_layout(&self) -> bool {
        self.cache.dirt().descendant.layout
    }

    fn is_virtual(&self) -> bool {
        self.is_virtual
    }

    fn descendant_needs_measure(&self) -> bool {
        self.cache.dirt().descendant.measure
    }

    fn mark_needs_semantics(&self) {
        self.needs_semantics.set(true);
        self.forget_semantics_reach();
    }

    fn mark_descendant_needs_semantics(&self) {
        self.descendant_needs_semantics.set(true);
    }

    fn needs_semantics(&self) -> bool {
        Self::needs_semantics(self)
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

    fn rehouse_for_recycle(&self) -> Option<Box<dyn Node>> {
        Some(Box::new(Self::new_recycled_shell(self.is_virtual)))
    }

    fn rehouse_for_live_compaction(&mut self) -> Option<Box<dyn Node>> {
        // The whole node moves into the new box. Its modifier nodes keep
        // what the user is doing with it, such as a focus, a held press or an
        // IME composition, and its caches and registry entry stay true for
        // it. Only its child list, which a spike of children may have grown,
        // is cut to size. The shell left behind has no id to unregister.
        let mut node = std::mem::replace(self, Self::new_recycled_shell(self.is_virtual));
        node.children.shrink_to_fit();
        Some(Box::new(node))
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
}

pub(crate) struct LayoutNodeRegistryState {
    entries: RefCell<HashMap<NodeId, LayoutNodeRegistryEntry>>,
}

impl LayoutNodeRegistryState {
    pub(crate) fn new() -> Self {
        Self {
            entries: RefCell::new(HashMap::default()),
        }
    }

    fn register(&self, id: NodeId, node: &LayoutNode) {
        self.entries.borrow_mut().insert(
            id,
            LayoutNodeRegistryEntry {
                parent: node.parent(),
                modifier_child_capabilities: node.modifier_child_capabilities(),
                modifier_locals: node.modifier_locals_handle(),
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
            let mut rebuilt = HashMap::default();
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
    crate::render_state::with_current_layout_node_registry(|registry| registry.register(id, node))
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
