//! A parent's record of each child it measures, and the bind of the
//! records to the children before a pass over them.

use std::{
    cell::{Cell, Ref, RefCell},
    ops::Deref,
    rc::{Rc, Weak},
};

use cranpose_core::{MemoryApplier, Node, NodeError, NodeId, NodeIdCell, PackedU64};
use cranpose_ui_layout::{AlignmentLines, ChildPlacer, Constraints, MeasurePolicy, Placement};

use super::{
    LayoutBuilderState, LayoutRuntimeState, MeasuredChild, MeasuredNode, core::Measurable,
    layout_child_measurable::LayoutChildMeasurable,
};
use crate::{
    modifier::{Point, SharedParentData},
    render_state::AppContext,
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{IntrinsicKind, LayoutNode, LayoutParts, LayoutState, NodeCache},
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the bind of a parent's child records in their file"
)]
impl LayoutBuilderState {
    pub(super) fn bind_layout_children(
        self: &Rc<Self>,
        applier: &mut MemoryApplier,
        (node_id, parts): (NodeId, &Rc<LayoutParts>),
        child_ids: &[NodeId],
        pass: ChildPass,
    ) -> Result<(), NodeError> {
        let mut runtime_state = parts.runtime_state().borrow_mut();
        let Some(children) =
            runtime_state.bind_children(self, (node_id, parts), pass, child_ids.len())
        else {
            return Ok(());
        };
        let mut bound = 0;
        for &child_id in child_ids {
            if self.bind_layout_child(applier, children, bound, child_id)? {
                bound += 1;
            }
        }
        children.truncate(bound);
        Ok(())
    }

    fn bind_layout_child(
        &self,
        applier: &mut MemoryApplier,
        children: &mut LayoutChildren,
        position: usize,
        child_id: NodeId,
    ) -> Result<bool, NodeError> {
        if children.rebinds_unchanged(position, child_id, self) {
            return Ok(true);
        }
        let bound = applier.with_node::<LayoutNode, _>(child_id, |child| {
            children.bind_record(
                position,
                child_id,
                LayoutChildBinding {
                    parts: child.layout_parts(),
                    layout_node: true,
                    parent_data: child.parent_data(),
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
                    children.bind_record(
                        position,
                        child_id,
                        LayoutChildBinding {
                            parts: child.layout_parts(),
                            layout_node: false,
                            parent_data: child.parent_data(),
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
}

pub(super) struct LayoutChildFrame {
    pub(super) builder: RefCell<Option<Rc<LayoutBuilderState>>>,
    // The first error a child recorded in the pass, boxed: most passes
    // record none.
    pub(super) error: Cell<Option<Box<NodeError>>>,
    pass: Cell<ChildPass>,
    // The node whose children the frame binds, which places them.
    parent: NodeIdCell,
    // How many of the children have a z index other than 0, as their
    // records last placed or read it.
    placed_with_z_index: Cell<u32>,
    // The parts of the parent, whose layout state notes whether it placed
    // a child with a z index.
    parent_parts: Weak<LayoutParts>,
    // The parent's record of each child, in the children's order. A pass
    // reads and writes a record through its cells; only a bind adds, moves
    // or drops records.
    records: RefCell<Vec<LayoutChildMeasureState>>,
}

/// What a node binds its children for.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ChildPass {
    /// A measure of the node, which records how it read each child, for a
    /// later keep of its measurement.
    #[default]
    Measure,
    /// An intrinsic size of the node, which its parent asks: the record of
    /// the node's last measure stays as it was.
    Intrinsics,
}

impl LayoutChildFrame {
    pub(super) fn new(parent_parts: Weak<LayoutParts>) -> Self {
        Self {
            builder: RefCell::new(None),
            error: Cell::new(None),
            pass: Cell::new(ChildPass::default()),
            parent: NodeIdCell::new(None),
            placed_with_z_index: Cell::new(0),
            parent_parts,
            records: RefCell::default(),
        }
    }

    pub(super) fn records(&self) -> Ref<'_, Vec<LayoutChildMeasureState>> {
        self.records.borrow()
    }

    pub(super) fn bind(&self, builder: &Rc<LayoutBuilderState>, parent: NodeId, pass: ChildPass) {
        self.error.take();
        *self.builder.borrow_mut() = Some(Rc::clone(builder));
        self.pass.set(pass);
        self.parent.set(Some(parent));
    }

    // Counts a child whose z index went from `before` to `after`. The
    // parent's layout state notes when the count leaves or reaches 0.
    fn count_placed_z_index(&self, before: f32, after: f32) {
        let count = match (before != 0.0, after != 0.0) {
            (false, true) => self.placed_with_z_index.get() + 1,
            (true, false) => self.placed_with_z_index.get() - 1,
            _ => return,
        };
        self.placed_with_z_index.set(count);
        if count <= 1
            && let Some(parent) = self.parent_parts.upgrade()
        {
            parent
                .layout_state()
                .borrow_mut()
                .set_places_children_with_z_index(count > 0);
        }
    }

    pub(super) fn measures(&self) -> bool {
        self.pass.get() == ChildPass::Measure
    }

    pub(super) fn unbind(&self) {
        self.builder.borrow_mut().take();
    }

    fn record_error(&self, err: NodeError) {
        if self.builder.borrow().is_none() {
            return;
        }
        let first = self.error.take();
        self.error.set(first.or_else(|| Some(Box::new(err))));
    }
}

pub(super) struct LayoutChildBinding<'a> {
    pub(super) parts: &'a Rc<LayoutParts>,
    // Whether the child is a layout node, whose size and placed flag its
    // parent's measure writes: a subcompose node writes its own.
    pub(super) layout_node: bool,
    pub(super) parent_data: SharedParentData,
    /// The child's own layout or measure is dirty.
    pub(super) dirty: bool,
    /// Only nodes below the child are dirty.
    pub(super) descendant_dirty: bool,
}

// A placeable places its child through the record at its slot or, when a
// bind moved the records after its measure, through the child's record
// wherever it sits.
impl ChildPlacer for LayoutChildFrame {
    fn place(&self, slot: u32, child: NodeId, x: f32, y: f32) {
        let records = self.records.borrow();
        let record = records
            .get(slot as usize)
            .filter(|record| record.node_id == child)
            .or_else(|| records.iter().find(|record| record.node_id == child));
        if let Some(record) = record {
            record.place(self, x, y);
        }
    }
}

// A parent's record of one child, for each child of every parent, in the
// parent's frame: its shared parts sit in `Cell`s, read by take and put
// back, where a `RefCell`'s borrow flag would take a word of every record.
pub(super) struct LayoutChildMeasureState {
    pub(super) node_id: NodeId,
    // The child's shared parts, `None` until the record is first bound.
    pub(super) parts: Cell<Option<Rc<LayoutParts>>>,
    // Whether the record writes the child's size and placed flag: see
    // `LayoutChildBinding::layout_node`.
    pub(super) layout_node: Cell<bool>,
    // The revision of the child's cache when the record last read the child.
    revision: Cell<u32>,
    pub(super) cache_epoch: Cell<PackedU64>,
    pub(super) force_remeasure: Cell<bool>,
    pub(super) parent_data: Cell<SharedParentData>,
    /// The constraints the parent's last measure gave the child.
    pub(super) measured_constraints: Cell<Option<Constraints>>,
    /// Whether the parent's last measure read the child's intrinsic sizes.
    pub(super) read_intrinsics: Cell<bool>,
    pub(super) measured: Cell<Option<Rc<MeasuredNode>>>,
    pub(super) last_position: Cell<Option<Point>>,
    // The child's z index, as the record last placed it or, at a bind that
    // read the child, found it.
    placed_z_index: Cell<f32>,
}

/// Runs `read` on what `cell` holds and puts it back. `read` must not reach
/// the same cell again.
pub(super) fn read_cell<T: Default, R>(cell: &Cell<T>, read: impl FnOnce(&T) -> R) -> R {
    let value = cell.take();
    let result = read(&value);
    cell.set(value);
    result
}

impl LayoutChildMeasureState {
    pub(super) fn placement_position(&self, placement: Option<&Placement>) -> Point {
        placement
            .map(|placement| Point {
                x: placement.x,
                y: placement.y,
            })
            .or_else(|| self.last_position.get())
            .unwrap_or_default()
    }

    pub(super) fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            parts: Cell::new(None),
            layout_node: Cell::new(false),
            revision: Cell::new(0),
            cache_epoch: Cell::default(),
            force_remeasure: Cell::new(true),
            parent_data: Cell::default(),
            measured_constraints: Cell::new(None),
            read_intrinsics: Cell::new(false),
            measured: Cell::new(None),
            last_position: Cell::new(None),
            placed_z_index: Cell::new(0.0),
        }
    }

    /// Binds the child for a measure of its parent. A child with only dirty
    /// nodes below it keeps its cache: it measures again, but may keep its
    /// measurement.
    pub(super) fn bind(
        &self,
        frame: &LayoutChildFrame,
        binding: LayoutChildBinding<'_>,
        pass: &LayoutBuilderState,
    ) {
        let cache = binding.parts.cache();
        self.bind_cache(
            frame,
            cache,
            (binding.dirty, binding.descendant_dirty),
            pass,
        );
        self.revision.set(cache.revision());
        self.note_z_index(frame, binding.parts.layout_state().borrow().z_index());
        let parts = self.parts.take();
        let shared = parts
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, binding.parts));
        self.parts.set(if shared {
            parts
        } else {
            Some(Rc::clone(binding.parts))
        });
        self.layout_node.set(binding.layout_node);
        self.parent_data.set(binding.parent_data);
    }

    // Whether `policy` reads a change from the parent data of the record's
    // last bind to `data`. A record bound to none reads it as changed.
    pub(super) fn reads_parent_data_change(
        &self,
        policy: &dyn MeasurePolicy,
        data: &SharedParentData,
    ) -> bool {
        if read_cell(&self.parts, Option::is_none) {
            return true;
        }
        read_cell(&self.parent_data, |bound| {
            !bound.same(data)
                && bound
                    .read(|bound| data.read(|data| policy.reads_parent_data_change(bound, data)))
        })
    }

    // Binds the record again to the layout node it holds, without a look at
    // the node, while the node's cache has the revision of the record's last
    // read: the node's parts and parent data are then as the record holds
    // them, and the node's flags sit in its cache. `false` otherwise.
    fn rebind_unchanged(&self, frame: &LayoutChildFrame, pass: &LayoutBuilderState) -> bool {
        self.layout_node.get()
            && read_cell(&self.parts, |parts| {
                parts.as_deref().is_some_and(|parts| {
                    let cache = parts.cache();
                    let unchanged = cache.revision() == self.revision.get();
                    if unchanged {
                        let dirt = cache.dirt();
                        let dirty = dirt.measure || dirt.layout;
                        self.bind_cache(frame, cache, (dirty, dirt.descendant.layout), pass);
                    }
                    unchanged
                })
            })
    }

    fn bind_cache(
        &self,
        frame: &LayoutChildFrame,
        cache: &NodeCache,
        (dirty, descendant_dirty): (bool, bool),
        pass: &LayoutBuilderState,
    ) {
        let child_epoch = cache.epoch();
        let stale = dirty || child_epoch < pass.cache_floor;
        let cache_epoch = if stale { pass.cache_epoch } else { child_epoch };
        cache.activate(cache_epoch);
        self.cache_epoch.set(PackedU64::new(cache_epoch));
        self.force_remeasure.set(stale || descendant_dirty);
        if frame.measures() {
            self.measured.take();
            self.last_position.set(None);
            self.measured_constraints.set(None);
            self.read_intrinsics.set(false);
        }
    }

    /// Runs `f` on the child's layout state with the context of the pass it
    /// measures in; nothing while it is bound to no pass.
    pub(super) fn with_pass_layout_state(
        &self,
        frame: &LayoutChildFrame,
        f: impl FnOnce(&mut LayoutState, &AppContext),
    ) {
        if !self.layout_node.get() {
            return;
        }
        let builder = frame.builder.borrow();
        let Some(builder) = builder.as_ref() else {
            return;
        };
        read_cell(&self.parts, |parts| {
            if let Some(parts) = parts {
                f(&mut parts.layout_state().borrow_mut(), &builder.context);
            }
        });
    }

    // Places the child where its parent's placement says. A policy that
    // placed its placeable there in this pass, as one that places its
    // placeables does, left only the z index to set.
    pub(super) fn place_at(&self, frame: &LayoutChildFrame, position: Point, z_index: f32) {
        if self.last_position.get() != Some(position) {
            self.place_retained(frame, position);
        }
        if self.placed_z_index.get().to_bits() != z_index.to_bits() {
            self.note_z_index(frame, z_index);
            read_cell(&self.parts, |parts| {
                if let Some(parts) = parts {
                    parts
                        .layout_state()
                        .borrow_mut()
                        .set_z_index(Some(self.node_id), z_index);
                }
            });
        }
    }

    // Takes `z_index` as the child's, and counts the child among its
    // parent's children with a z index.
    fn note_z_index(&self, frame: &LayoutChildFrame, z_index: f32) {
        let before = self.placed_z_index.replace(z_index);
        frame.count_placed_z_index(before, z_index);
    }

    // Places the child at `(x, y)` as its parent's policy placed it, past
    // the offset its own measurement puts its content at.
    fn place(&self, frame: &LayoutChildFrame, x: f32, y: f32) {
        let internal_offset = read_cell(&self.measured, |measured| {
            measured
                .as_ref()
                .map(|measured| measured.offset)
                .unwrap_or_default()
        });
        self.place_retained(
            frame,
            Point {
                x: x + internal_offset.x,
                y: y + internal_offset.y,
            },
        );
    }

    pub(super) fn place_retained(&self, frame: &LayoutChildFrame, position: Point) {
        self.last_position.set(Some(position));
        let builder = frame.builder.borrow();
        let Some(builder) = builder.as_ref() else {
            return;
        };
        read_cell(&self.parts, |parts| {
            if let Some(parts) = parts {
                parts.layout_state().borrow_mut().place(
                    Some(self.node_id),
                    position,
                    frame.parent.get(),
                    &builder.context,
                );
            }
        });
    }

    /// The intrinsic size of a child that is a layout node, from its
    /// modifier chain and measure policy, without measuring it. `None` for
    /// another node type, which measures for it.
    pub(super) fn layout_intrinsic(
        &self,
        frame: &LayoutChildFrame,
        kind: IntrinsicKind,
    ) -> Option<f32> {
        let builder = frame.builder.borrow();
        let builder = builder.as_ref()?;
        match builder.layout_node_intrinsic(self.node_id, kind) {
            Ok(value) => value,
            Err(err) => {
                frame.record_error(err);
                Some(0.0)
            }
        }
    }

    fn perform_measure(
        &self,
        frame: &LayoutChildFrame,
        constraints: Constraints,
    ) -> Result<Rc<MeasuredNode>, NodeError> {
        let builder = frame.builder.borrow();
        let builder = builder.as_ref().ok_or(NodeError::MissingContext {
            id: self.node_id,
            reason: "layout child applier not configured",
        })?;
        builder.measure_node(self.node_id, constraints)
    }

    pub(super) fn measure_cached(
        &self,
        frame: &LayoutChildFrame,
        constraints: Constraints,
    ) -> Option<Rc<MeasuredNode>> {
        if frame.measures() {
            self.measured_constraints.set(Some(constraints));
        }
        let parts = read_cell(&self.parts, Clone::clone);
        let cache = parts.as_deref().map(LayoutParts::cache);
        if let Some(cache) = cache {
            cache.activate(self.cache_epoch.get().get());
        }
        if !self.force_remeasure.get()
            && let Some(cached) = cache.and_then(|cache| cache.get_measurement(constraints))
        {
            // A measure clears the placed flag the parent's placement sets
            // again; a child it no longer places then leaves the scene.
            if frame.measures() {
                self.with_pass_layout_state(frame, |state, context| {
                    state.clear_placed(Some(self.node_id), context);
                });
            }
            return Some(cached);
        }

        match self.perform_measure(frame, constraints) {
            Ok(measured) => {
                self.force_remeasure.set(false);
                if let Some(cache) = cache {
                    cache.store_measurement(constraints, Rc::clone(&measured));
                }
                Some(measured)
            }
            Err(err) => {
                frame.record_error(err);
                None
            }
        }
    }
}

// The records of a node's children, borrowed from their frame for a pass:
// none for a node that never bound a child.
pub(super) struct ChildRecords<'a>(Option<Ref<'a, Vec<LayoutChildMeasureState>>>);

impl Deref for ChildRecords<'_> {
    type Target = [LayoutChildMeasureState];

    fn deref(&self) -> &[LayoutChildMeasureState] {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "a node's children records in their file"
)]
impl LayoutRuntimeState {
    pub(super) fn child_states(&self) -> ChildRecords<'_> {
        ChildRecords(
            self.children
                .as_deref()
                .map(|children| children.frame.records()),
        )
    }

    pub(super) fn child_measurables(&self) -> &[Box<dyn Measurable>] {
        self.children
            .as_deref()
            .map_or(&[], |children| children.measurables.as_slice())
    }

    /// Binds the children's frame of the node `parent` to `builder` for a
    /// pass over `count` children. A node that never had one binds nothing.
    pub(super) fn bind_children(
        &mut self,
        builder: &Rc<LayoutBuilderState>,
        (parent, parts): (NodeId, &Rc<LayoutParts>),
        pass: ChildPass,
        count: usize,
    ) -> Option<&mut LayoutChildren> {
        if count == 0 && self.children.is_none() {
            return None;
        }
        let children = self
            .children
            .get_or_insert_with(|| Box::new(LayoutChildren::with_capacity(count, parts)));
        children.frame.bind(builder, parent, pass);
        Some(children)
    }

    pub(super) fn unbind_children(&self) {
        if let Some(children) = &self.children {
            children.frame.unbind();
        }
    }

    /// The first error a child recorded in the pass, taken.
    pub(super) fn take_child_error(&self) -> Option<NodeError> {
        self.children
            .as_ref()
            .and_then(|children| children.frame.error.take())
            .map(|err| *err)
    }

    /// Puts the children the last measure measured into `measured_children`.
    pub(super) fn measured_children(
        &self,
        placements: &[Placement],
        placement_indices: &[usize],
        content_offset: Point,
        measured_children: &mut Vec<MeasuredChild>,
    ) {
        let Some(children) = self.children.as_deref() else {
            return;
        };
        let child_states = children.frame.records();
        measured_children.reserve_exact(child_states.len());
        for (index, child_state) in child_states.iter().enumerate() {
            let Some(measured) = child_state.measured.take() else {
                continue;
            };
            let placement =
                placement_for_child(placements, placement_indices, index, child_state.node_id);
            let base_position = child_state.placement_position(placement);
            let position = Point {
                x: base_position.x + measured.offset.x,
                y: base_position.y + measured.offset.y,
            };
            if let Some(placement) = placement {
                child_state.place_at(&children.frame, position, placement.z_index);
            }
            measured_children.push(MeasuredChild::new(
                &measured,
                Point {
                    x: content_offset.x + base_position.x,
                    y: content_offset.y + base_position.y,
                },
            ));
        }
    }

    // Whether the node's last placement gave a child a z index other than 0.
}

pub(super) fn inherited_alignment_lines(
    child_states: &[LayoutChildMeasureState],
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
        read_cell(&child.measured, |measured| {
            if let Some(measured) = measured {
                lines.merge(
                    measured
                        .alignment_lines_for_parent()
                        .translated(child.placement_position(placement).y),
                );
            }
        });
    }
    lines
}

pub(super) fn placement_for_child<'a>(
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

// What a node keeps for measuring its children, in their order: their
// records in the frame, and a measurable for each that names its record by
// its place.
pub(super) struct LayoutChildren {
    measurables: Vec<Box<dyn Measurable>>,
    frame: Rc<LayoutChildFrame>,
}

impl LayoutChildren {
    /// Room for exactly `count` children, where growth would make room for
    /// four in each of the two lists. Children added later grow the lists
    /// as usual.
    fn with_capacity(count: usize, parent_parts: &Rc<LayoutParts>) -> Self {
        let frame = LayoutChildFrame::new(Rc::downgrade(parent_parts));
        frame.records.borrow_mut().reserve_exact(count);
        Self {
            measurables: Vec::with_capacity(count),
            frame: Rc::new(frame),
        }
    }

    // Whether the record at `position` holds `child_id` and binds it again
    // without a look at the node.
    fn rebinds_unchanged(
        &self,
        position: usize,
        child_id: NodeId,
        pass: &LayoutBuilderState,
    ) -> bool {
        let records = self.frame.records.borrow();
        records.get(position).is_some_and(|record| {
            record.node_id == child_id && record.rebind_unchanged(&self.frame, pass)
        })
    }

    // Binds the record of `child_id` at `position`, for a pass
    // `bind_children` bound. Inlined: the child is nearly always where it
    // was.
    #[inline]
    fn bind_record(
        &mut self,
        position: usize,
        child_id: NodeId,
        binding: LayoutChildBinding<'_>,
        pass: &LayoutBuilderState,
    ) {
        if self.id_at(position) != Some(child_id) {
            self.bind_at(position, child_id);
        }
        self.frame.records.borrow()[position].bind(&self.frame, binding, pass);
    }

    // The id of the child whose record sits at `position`.
    fn id_at(&self, position: usize) -> Option<NodeId> {
        self.frame
            .records
            .borrow()
            .get(position)
            .map(|record| record.node_id)
    }

    // Drops the records past `len`, each taking its child out of the
    // parent's count of children placed with a z index.
    fn truncate(&mut self, len: usize) {
        let mut records = self.frame.records.borrow_mut();
        for record in records.get(len..).unwrap_or_default() {
            self.frame
                .count_placed_z_index(record.placed_z_index.get(), 0.0);
        }
        records.truncate(len);
        self.measurables.truncate(len);
    }

    // Puts the record of `child_id` at `position`: the one it had further
    // on, or a new one. A measurable names the record at its own place, so
    // only the records move.
    #[inline(never)]
    fn bind_at(&mut self, position: usize, child_id: NodeId) {
        let mut records = self.frame.records.borrow_mut();
        let from = match records[position..]
            .iter()
            .position(|record| record.node_id == child_id)
        {
            Some(offset) => position + offset,
            None => {
                records.push(LayoutChildMeasureState::new(child_id));
                let slot = records.len() - 1;
                self.measurables.push(Box::new(LayoutChildMeasurable::new(
                    Rc::clone(&self.frame),
                    u32::try_from(slot).expect("a node has fewer than 2^32 children"),
                )));
                slot
            }
        };
        records.swap(position, from);
    }
}
