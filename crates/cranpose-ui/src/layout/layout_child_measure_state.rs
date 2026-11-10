// A parent's record of each child it measures, and the bind of the
// records to the children before a pass over them.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use cranpose_core::{MemoryApplier, Node, NodeError, NodeId, NodeIdCell, PackedU64};
use cranpose_ui_graphics::shared_sort;
use cranpose_ui_layout::{AlignmentLines, Constraints, MeasurePolicy, Placement};

use super::{
    LayoutBuilderState, LayoutRuntimeState, MeasuredChild, MeasuredNode, core::Measurable,
    layout_child_measurable::LayoutChildMeasurable,
};
use crate::{
    modifier::{Point, SharedParentData},
    render_state::AppContext,
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{IntrinsicKind, LayoutCells, LayoutNode, LayoutParts, NodeCache},
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
        if children.rebind_unchanged(position, child_id, self) {
            return Ok(true);
        }
        let bound = applier.with_node::<LayoutNode, _>(child_id, |child| {
            children.bind(
                (position, child_id),
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
                    children.bind(
                        (position, child_id),
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
}

// What a node binds its children for.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ChildPass {
    // A measure of the node, which records how it read each child, for a
    // later keep of its measurement.
    #[default]
    Measure,
    // An intrinsic size of the node, which its parent asks: the record of
    // the node's last measure stays as it was.
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
        }
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
    // The child's own layout or measure is dirty.
    pub(super) dirty: bool,
    // Only nodes below the child are dirty.
    pub(super) descendant_dirty: bool,
}

// A parent's record of one child. It sits in the child's parts, as a node
// has one parent: the parent lists the parts of its children, and reads
// and writes each child's record there. Its shared values sit in `Cell`s,
// read by take and put back, where a `RefCell`'s borrow flag would take a
// word of every record.
pub(crate) struct LayoutChildMeasureState {
    // The child, `None` until a parent binds the record.
    node_id: NodeIdCell,
    // The frame of the parent that bound the record last, `None` once that
    // parent let the child go.
    frame: RefCell<Option<Rc<LayoutChildFrame>>>,
    // Whether the record writes the child's size and placed flag: see
    // `LayoutChildBinding::layout_node`.
    pub(super) layout_node: Cell<bool>,
    // The revision of the child's cache when the record last read the child.
    revision: Cell<u32>,
    pub(super) cache_epoch: Cell<PackedU64>,
    pub(super) force_remeasure: Cell<bool>,
    pub(super) parent_data: Cell<SharedParentData>,
    // The constraints the parent's last measure gave the child.
    pub(super) measured_constraints: Cell<Option<Constraints>>,
    // Whether the parent's last measure read the child's intrinsic sizes.
    pub(super) read_intrinsics: Cell<bool>,
    pub(super) measured: Cell<Option<Rc<MeasuredNode>>>,
    pub(super) last_position: Cell<Option<Point>>,
    // The child's z index, as the record last placed it or, at a bind that
    // read the child, found it.
    placed_z_index: Cell<f32>,
}

impl Default for LayoutChildMeasureState {
    fn default() -> Self {
        Self {
            node_id: NodeIdCell::new(None),
            frame: RefCell::new(None),
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
}

// Runs `read` on what `cell` holds and puts it back. `read` must not reach
// the same cell again.
pub(super) fn read_cell<T: Default, R>(cell: &Cell<T>, read: impl FnOnce(&T) -> R) -> R {
    let value = cell.take();
    let result = read(&value);
    cell.set(value);
    result
}

impl LayoutChildMeasureState {
    // The child the record is of.
    pub(super) fn child_id(&self) -> NodeId {
        self.node_id.get().unwrap_or_default()
    }

    // Whether `frame`'s parent holds the record.
    fn is_of(&self, frame: &Rc<LayoutChildFrame>) -> bool {
        self.frame
            .borrow()
            .as_ref()
            .is_some_and(|held| Rc::ptr_eq(held, frame))
    }

    // Whether the pass that binds the record measures its parent.
    pub(super) fn measures(&self) -> bool {
        self.frame
            .borrow()
            .as_deref()
            .is_some_and(LayoutChildFrame::measures)
    }

    // The builder of the pass the record's parent is bound to.
    pub(super) fn builder(&self) -> Option<Rc<LayoutBuilderState>> {
        self.frame.borrow().as_ref()?.builder.borrow().clone()
    }

    // Runs `read` on the builder of the pass the record's parent is bound
    // to; `None` while it is bound to none.
    pub(super) fn with_builder<R>(&self, read: impl FnOnce(&LayoutBuilderState) -> R) -> Option<R> {
        let frame = self.frame.borrow();
        let builder = frame.as_ref()?.builder.borrow();
        builder.as_deref().map(read)
    }

    fn record_error(&self, err: NodeError) {
        if let Some(frame) = self.frame.borrow().as_ref() {
            frame.record_error(err);
        }
    }

    pub(super) fn placement_position(&self, placement: Option<&Placement>) -> Point {
        placement
            .map(|placement| Point {
                x: placement.x,
                y: placement.y,
            })
            .or_else(|| self.last_position.get())
            .unwrap_or_default()
    }

    // Whether `policy` reads a change from the parent data of the record's
    // last bind to `data`. A record no parent holds reads it as changed.
    pub(super) fn reads_parent_data_change(
        &self,
        policy: &dyn MeasurePolicy,
        data: &SharedParentData,
    ) -> bool {
        if self.frame.borrow().is_none() {
            return true;
        }
        read_cell(&self.parent_data, |bound| {
            !bound.same(data)
                && bound
                    .read(|bound| data.read(|data| policy.reads_parent_data_change(bound, data)))
        })
    }

    // Takes `z_index` as the child's, and counts the child among its
    // parent's children with a z index.
    fn note_z_index(&self, z_index: f32) {
        let before = self.placed_z_index.replace(z_index);
        if let Some(frame) = self.frame.borrow().as_ref() {
            frame.count_placed_z_index(before, z_index);
        }
    }

    // Takes the record from the parent that holds it: the child leaves
    // that parent's count of children placed with a z index, and the record
    // starts over.
    fn release(&self) {
        self.note_z_index(0.0);
        self.frame.borrow_mut().take();
        self.measured.take();
        self.last_position.set(None);
        self.measured_constraints.set(None);
        self.read_intrinsics.set(false);
        self.force_remeasure.set(true);
    }

    // `measures` tells whether the pass measures the record's parent.
    fn bind_cache(
        &self,
        cache: &NodeCache,
        (dirty, descendant_dirty): (bool, bool),
        measures: bool,
        pass: &LayoutBuilderState,
    ) {
        let child_epoch = cache.epoch();
        let stale = dirty || child_epoch < pass.cache_floor;
        let cache_epoch = if stale { pass.cache_epoch } else { child_epoch };
        cache.activate(cache_epoch);
        self.cache_epoch.set(PackedU64::new(cache_epoch));
        self.force_remeasure.set(stale || descendant_dirty);
        if measures {
            self.measured.take();
            self.last_position.set(None);
            self.measured_constraints.set(None);
            self.read_intrinsics.set(false);
        }
    }
}

// A child as its parent's measure reaches it: the child's parts, with the
// parent's record of the child in them.
#[expect(
    clippy::multiple_inherent_impl,
    reason = "what a parent does with a child's parts, beside its record"
)]
impl LayoutParts {
    // Binds the child `child_id`, whose parts these are, for a measure of
    // the parent whose frame is `frame`. A child with only dirty nodes
    // below it keeps its cache: it measures again, but may keep its
    // measurement. A record another parent held leaves that parent first.
    pub(super) fn bind_child(
        &self,
        child_id: NodeId,
        binding: &LayoutChildBinding<'_>,
        frame: &Rc<LayoutChildFrame>,
        pass: &LayoutBuilderState,
    ) {
        let record = self.record();
        if !record.is_of(frame) {
            record.release();
            *record.frame.borrow_mut() = Some(Rc::clone(frame));
        }
        record.node_id.set(Some(child_id));
        let cache = self.cache();
        let dirt = (binding.dirty, binding.descendant_dirty);
        record.bind_cache(cache, dirt, frame.measures(), pass);
        record.revision.set(cache.revision());
        record.note_z_index(self.layout_state().z_index());
        record.layout_node.set(binding.layout_node);
        record.parent_data.set(binding.parent_data.clone());
    }

    // Binds the record again to the layout node it holds, for the parent
    // whose frame is `frame`, without a look at the node, while the node's
    // cache has the revision of the record's last read: the node's parent
    // data is then as the record holds it, and the node's flags sit in its
    // cache. `false` otherwise.
    fn rebind_unchanged(&self, frame: &Rc<LayoutChildFrame>, pass: &LayoutBuilderState) -> bool {
        let record = self.record();
        let cache = self.cache();
        let unchanged = record.layout_node.get()
            && cache.revision() == record.revision.get()
            && record.is_of(frame);
        if unchanged {
            let dirt = cache.dirt();
            let dirty = dirt.measure || dirt.layout;
            record.bind_cache(
                cache,
                (dirty, dirt.descendant.layout),
                frame.measures(),
                pass,
            );
        }
        unchanged
    }

    // Runs `f` on the child's layout state with the context of the pass it
    // measures in; nothing while it is bound to no pass.
    pub(super) fn with_pass_layout_state(&self, f: impl FnOnce(&LayoutCells, &AppContext)) {
        let record = self.record();
        if !record.layout_node.get() {
            return;
        }
        record.with_builder(|builder| f(self.layout_state(), &builder.context));
    }

    // Places the child where its parent's placement says. A policy that
    // placed its placeable there in this pass, as one that places its
    // placeables does, left only the z index to set.
    pub(super) fn place_at(&self, position: Point, z_index: f32) {
        let record = self.record();
        if record.last_position.get() != Some(position) {
            self.place_retained(position);
        }
        if record.placed_z_index.get().to_bits() != z_index.to_bits() {
            record.note_z_index(z_index);
            self.layout_state()
                .set_z_index(Some(record.child_id()), z_index);
        }
    }

    pub(super) fn place_retained(&self, position: Point) {
        let record = self.record();
        record.last_position.set(Some(position));
        let frame = record.frame.borrow();
        let Some(frame) = frame.as_ref() else {
            return;
        };
        let builder = frame.builder.borrow();
        let Some(builder) = builder.as_ref() else {
            return;
        };
        self.layout_state().place(
            Some(record.child_id()),
            position,
            frame.parent.get(),
            &builder.context,
        );
    }

    // The intrinsic size of a child that is a layout node, from its
    // modifier chain and measure policy, without measuring it. `None` for
    // another node type, which measures for it.
    pub(super) fn layout_intrinsic(&self, kind: IntrinsicKind) -> Option<f32> {
        let record = self.record();
        let builder = record.builder()?;
        match builder.layout_node_intrinsic(record.child_id(), kind) {
            Ok(value) => value,
            Err(err) => {
                record.record_error(err);
                Some(0.0)
            }
        }
    }

    fn perform_measure(&self, constraints: Constraints) -> Result<Rc<MeasuredNode>, NodeError> {
        let record = self.record();
        let builder = record.builder().ok_or_else(|| NodeError::MissingContext {
            id: record.child_id(),
            reason: "layout child applier not configured",
        })?;
        builder.measure_node(record.child_id(), constraints)
    }

    pub(super) fn measure_cached(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        let record = self.record();
        let measures = record.measures();
        if measures {
            record.measured_constraints.set(Some(constraints));
        }
        let cache = self.cache();
        cache.activate(record.cache_epoch.get().get());
        if !record.force_remeasure.get()
            && let Some(cached) = cache.get_measurement(constraints)
        {
            // A measure clears the placed flag the parent's placement sets
            // again; a child it no longer places then leaves the scene.
            if measures {
                self.with_pass_layout_state(|state, context| {
                    state.clear_placed(Some(record.child_id()), context);
                });
            }
            return Some(cached);
        }

        match self.perform_measure(constraints) {
            Ok(measured) => {
                record.force_remeasure.set(false);
                cache.store_measurement(constraints, Rc::clone(&measured));
                Some(measured)
            }
            Err(err) => {
                record.record_error(err);
                None
            }
        }
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "a node's children records in their file"
)]
impl LayoutRuntimeState {
    pub(super) fn child_states(&self) -> &[Rc<LayoutParts>] {
        self.children
            .as_deref()
            .map_or(&[], |children| children.states.as_slice())
    }

    pub(super) fn child_measurables(&self) -> &[Box<dyn Measurable>] {
        self.children
            .as_deref()
            .map_or(&[], |children| children.measurables.as_slice())
    }

    // Binds the children's frame of the node `parent` to `builder` for a
    // pass over `count` children. A node that never had one binds nothing.
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

    // The first error a child recorded in the pass, taken.
    pub(super) fn take_child_error(&self) -> Option<NodeError> {
        self.children
            .as_ref()
            .and_then(|children| children.frame.error.take())
            .map(|err| *err)
    }

    // Puts the children the last measure measured into `measured_children`.
    pub(super) fn measured_children(
        &self,
        placements: &[Placement],
        placement_indices: &[usize],
        content_offset: Point,
        measured_children: &mut Vec<MeasuredChild>,
    ) {
        let child_states = self.child_states();
        measured_children.reserve_exact(child_states.len());
        for (index, child) in child_states.iter().enumerate() {
            let record = child.record();
            let Some(measured) = record.measured.take() else {
                continue;
            };
            let child_id = record.child_id();
            let placement = placement_for_child(placements, placement_indices, index, child_id);
            let base_position = record.placement_position(placement);
            let position = Point {
                x: base_position.x + measured.offset.x,
                y: base_position.y + measured.offset.y,
            };
            if let Some(placement) = placement {
                child.place_at(position, placement.z_index);
            }
            measured_children.push(MeasuredChild::new(
                child_id,
                &measured,
                Point {
                    x: content_offset.x + base_position.x,
                    y: content_offset.y + base_position.y,
                },
            ));
        }
    }
}

pub(super) fn inherited_alignment_lines(
    child_states: &[Rc<LayoutParts>],
    placements: &[Placement],
    placement_indices: &mut Vec<usize>,
) -> AlignmentLines {
    placement_indices.clear();
    let in_child_order = placements.len() == child_states.len()
        && placements
            .iter()
            .zip(child_states)
            .all(|(placement, child)| placement.node_id == child.record().child_id());
    if !in_child_order {
        placement_indices.extend(0..placements.len());
        shared_sort::sort_positions(placement_indices, &mut |a, b| {
            placements[a].node_id.cmp(&placements[b].node_id)
        });
    }
    let mut lines = AlignmentLines::default();
    for (index, child) in child_states.iter().enumerate() {
        let record = child.record();
        let placement =
            placement_for_child(placements, placement_indices, index, record.child_id());
        read_cell(&record.measured, |measured| {
            if let Some(measured) = measured {
                lines.merge(
                    measured
                        .alignment_lines_for_parent()
                        .translated(record.placement_position(placement).y),
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

// What a node keeps for measuring its children, in their order: the parts
// of each child, which hold the node's record of it.
pub(super) struct LayoutChildren {
    states: Vec<Rc<LayoutParts>>,
    measurables: Vec<Box<dyn Measurable>>,
    frame: Rc<LayoutChildFrame>,
}

impl LayoutChildren {
    // Room for exactly `count` children, where growth would make room for
    // four in each of the two lists. Children added later grow the lists
    // as usual.
    fn with_capacity(count: usize, parent_parts: &Rc<LayoutParts>) -> Self {
        Self {
            states: Vec::with_capacity(count),
            measurables: Vec::with_capacity(count),
            frame: Rc::new(LayoutChildFrame::new(Rc::downgrade(parent_parts))),
        }
    }

    // Binds again the child at `position`, when it is `child_id` and its
    // record holds: see `LayoutParts::rebind_unchanged`.
    #[inline]
    fn rebind_unchanged(
        &self,
        position: usize,
        child_id: NodeId,
        pass: &LayoutBuilderState,
    ) -> bool {
        self.states.get(position).is_some_and(|child| {
            child.record().node_id.get() == Some(child_id)
                && child.rebind_unchanged(&self.frame, pass)
        })
    }

    // Binds the child `child_id`, as `binding` reads it, at `position`,
    // for a pass `bind_children` bound.
    fn bind(
        &mut self,
        (position, child_id): (usize, NodeId),
        binding: LayoutChildBinding<'_>,
        pass: &LayoutBuilderState,
    ) {
        let here = self
            .states
            .get(position)
            .is_some_and(|child| Rc::ptr_eq(child, binding.parts));
        if !here {
            self.bind_at(position, binding.parts);
        }
        binding
            .parts
            .bind_child(child_id, &binding, &self.frame, pass);
    }

    // Lets go of the children from `len` on. Inlined: a bind nearly always
    // keeps every child.
    #[inline]
    fn truncate(&mut self, len: usize) {
        if len < self.states.len() {
            self.release_from(len);
        }
    }

    // Lets go of the children from `len` on: each record this node still
    // holds starts over.
    #[inline(never)]
    fn release_from(&mut self, len: usize) {
        for child in &self.states[len..] {
            if child.record().is_of(&self.frame) {
                child.record().release();
            }
        }
        self.states.truncate(len);
        self.measurables.truncate(len);
    }

    // Puts `parts`, a child's, at `position`: from further on, where the
    // child was, or as a new entry.
    #[inline(never)]
    fn bind_at(&mut self, position: usize, parts: &Rc<LayoutParts>) {
        let from = match self.states[position..]
            .iter()
            .position(|child| Rc::ptr_eq(child, parts))
        {
            Some(offset) => position + offset,
            None => {
                self.states.push(Rc::clone(parts));
                self.measurables
                    .push(Box::new(LayoutChildMeasurable::new(Rc::clone(parts))));
                self.states.len() - 1
            }
        };
        self.states.swap(position, from);
        self.measurables.swap(position, from);
    }
}

// A node that goes lets its children go.
impl Drop for LayoutChildren {
    fn drop(&mut self) {
        self.truncate(0);
    }
}
