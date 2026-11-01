//! A parent's record of each child it measures, and the bind of the
//! records to the children before a pass over them.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MemoryApplier, Node, NodeError, NodeId, PackedU64};
use cranpose_ui_layout::{AlignmentLines, Constraints, MeasurePolicy, ParentData, Placement};

use super::{LayoutBuilderState, LayoutRuntimeState, MeasuredNode};
use crate::{
    modifier::Point,
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
        (node_id, runtime_state): (NodeId, &RefCell<LayoutRuntimeState>),
        child_ids: &[NodeId],
        pass: ChildPass,
    ) -> Result<(), NodeError> {
        let mut runtime_state = runtime_state.borrow_mut();
        runtime_state.bind_children(self, node_id, pass, child_ids.len());
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
        let unchanged = runtime_state.children.as_deref().is_some_and(|children| {
            children.ids.get(position) == Some(&child_id)
                && children.states[position].rebind_unchanged(self)
        });
        if unchanged {
            return Ok(true);
        }
        let bound = applier.with_node::<LayoutNode, _>(child_id, |child| {
            runtime_state.child_state_at(position, child_id).bind(
                LayoutChildBinding {
                    parts: child.layout_parts(),
                    layout_node: true,
                    parent_data: Some(child.parent_data()),
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
                            parts: child.layout_parts(),
                            layout_node: false,
                            parent_data: Some(child.parent_data()),
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

#[derive(Default)]
pub(super) struct LayoutChildFrame {
    pub(super) builder: RefCell<Option<Rc<LayoutBuilderState>>>,
    pub(super) error: RefCell<Option<NodeError>>,
    pass: Cell<ChildPass>,
    /// The node whose children the frame binds, which places them.
    parent: Cell<Option<NodeId>>,
    // How many of the children the parent placed with a z index other
    // than 0, as their records last placed them.
    placed_with_z_index: Cell<u32>,
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
    pub(super) fn bind(&self, builder: &Rc<LayoutBuilderState>, parent: NodeId, pass: ChildPass) {
        self.error.borrow_mut().take();
        *self.builder.borrow_mut() = Some(Rc::clone(builder));
        self.pass.set(pass);
        self.parent.set(Some(parent));
    }

    // Counts a child whose placed z index went from `before` to `after`.
    fn count_placed_z_index(&self, before: Option<f32>, after: Option<f32>) {
        let with_z = |z_index: Option<f32>| z_index.is_some_and(|z_index| z_index != 0.0);
        match (with_z(before), with_z(after)) {
            (false, true) => self
                .placed_with_z_index
                .set(self.placed_with_z_index.get() + 1),
            (true, false) => self
                .placed_with_z_index
                .set(self.placed_with_z_index.get() - 1),
            _ => {}
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
        let mut slot = self.error.borrow_mut();
        if slot.is_none() {
            *slot = Some(err);
        }
    }
}

pub(super) struct LayoutChildBinding<'a> {
    pub(super) parts: &'a Rc<LayoutParts>,
    // Whether the child is a layout node, whose size and placed flag its
    // parent's measure writes: a subcompose node writes its own.
    pub(super) layout_node: bool,
    pub(super) parent_data: Option<ParentData>,
    /// The child's own layout or measure is dirty.
    pub(super) dirty: bool,
    /// Only nodes below the child are dirty.
    pub(super) descendant_dirty: bool,
}

/// A parent's record of one child, for each child of every parent: its
/// shared parts sit in `Cell`s, read by take and put back, where a
/// `RefCell`'s borrow flag would take a word of every record.
pub(super) struct LayoutChildMeasureState {
    pub(super) node_id: NodeId,
    pub(super) frame: Rc<LayoutChildFrame>,
    // The child's shared parts, `None` until the record is first bound.
    pub(super) parts: Cell<Option<Rc<LayoutParts>>>,
    // Whether the record writes the child's size and placed flag: see
    // `LayoutChildBinding::layout_node`.
    pub(super) layout_node: Cell<bool>,
    // The revision of the child's cache when the record last read the child.
    revision: Cell<u32>,
    pub(super) cache_epoch: Cell<PackedU64>,
    pub(super) force_remeasure: Cell<bool>,
    pub(super) parent_data: RefCell<Option<ParentData>>,
    /// The constraints the parent's last measure gave the child.
    pub(super) measured_constraints: Cell<Option<Constraints>>,
    /// Whether the parent's last measure read the child's intrinsic sizes.
    pub(super) read_intrinsics: Cell<bool>,
    pub(super) measured: Cell<Option<Rc<MeasuredNode>>>,
    pub(super) last_position: Cell<Option<Point>>,
    // The z index the record last placed the child with, `None` before it
    // placed the child.
    placed_z_index: Cell<Option<f32>>,
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

    pub(super) fn new(node_id: NodeId, frame: Rc<LayoutChildFrame>) -> Rc<Self> {
        Rc::new(Self {
            node_id,
            frame,
            parts: Cell::new(None),
            layout_node: Cell::new(false),
            revision: Cell::new(0),
            cache_epoch: Cell::default(),
            force_remeasure: Cell::new(true),
            parent_data: RefCell::new(None),
            measured_constraints: Cell::new(None),
            read_intrinsics: Cell::new(false),
            measured: Cell::new(None),
            last_position: Cell::new(None),
            placed_z_index: Cell::new(None),
        })
    }

    /// Binds the child for a measure of its parent. A child with only dirty
    /// nodes below it keeps its cache: it measures again, but may keep its
    /// measurement.
    pub(super) fn bind(&self, binding: LayoutChildBinding<'_>, pass: &LayoutBuilderState) {
        let cache = binding.parts.cache();
        self.bind_cache(cache, binding.dirty, binding.descendant_dirty, pass);
        self.revision.set(cache.revision());
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
        *self.parent_data.borrow_mut() = binding.parent_data;
    }

    // Whether `policy` reads a change from the parent data of the record's
    // last bind to `data`. A record bound to none reads it as changed.
    pub(super) fn reads_parent_data_change(
        &self,
        policy: &dyn MeasurePolicy,
        data: &ParentData,
    ) -> bool {
        self.parent_data
            .borrow()
            .as_ref()
            .is_none_or(|bound| policy.reads_parent_data_change(bound, data))
    }

    // Binds the record again to the layout node it holds, without a look at
    // the node, while the node's cache has the revision of the record's last
    // read: the node's parts and parent data are then as the record holds
    // them, and the node's flags sit in its cache. `false` otherwise.
    fn rebind_unchanged(&self, pass: &LayoutBuilderState) -> bool {
        self.layout_node.get()
            && read_cell(&self.parts, |parts| {
                parts.as_deref().is_some_and(|parts| {
                    let cache = parts.cache();
                    let unchanged = cache.revision() == self.revision.get();
                    if unchanged {
                        let dirt = cache.dirt();
                        let dirty = dirt.measure || dirt.layout;
                        self.bind_cache(cache, dirty, dirt.descendant.layout, pass);
                    }
                    unchanged
                })
            })
    }

    fn bind_cache(
        &self,
        cache: &NodeCache,
        dirty: bool,
        descendant_dirty: bool,
        pass: &LayoutBuilderState,
    ) {
        let child_epoch = cache.epoch();
        let stale = dirty || child_epoch < pass.cache_floor;
        let cache_epoch = if stale { pass.cache_epoch } else { child_epoch };
        cache.activate(cache_epoch);
        self.cache_epoch.set(PackedU64::new(cache_epoch));
        self.force_remeasure.set(stale || descendant_dirty);
        if self.frame.measures() {
            self.measured.take();
            self.last_position.set(None);
            self.measured_constraints.set(None);
            self.read_intrinsics.set(false);
        }
    }

    /// Runs `f` on the child's layout state with the context of the pass it
    /// measures in; nothing while it is bound to no pass.
    pub(super) fn with_pass_layout_state(&self, f: impl FnOnce(&mut LayoutState, &AppContext)) {
        if !self.layout_node.get() {
            return;
        }
        let builder = self.frame.builder.borrow();
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
    pub(super) fn place_at(&self, position: Point, z_index: f32) {
        if self.last_position.get() != Some(position) {
            self.place_retained(position);
        }
        let placed = self.placed_z_index.replace(Some(z_index));
        if placed != Some(z_index) {
            self.frame.count_placed_z_index(placed, Some(z_index));
            read_cell(&self.parts, |parts| {
                if let Some(parts) = parts {
                    parts.layout_state().borrow_mut().set_z_index(z_index);
                }
            });
        }
    }

    pub(super) fn place_retained(&self, position: Point) {
        self.last_position.set(Some(position));
        let builder = self.frame.builder.borrow();
        let Some(builder) = builder.as_ref() else {
            return;
        };
        read_cell(&self.parts, |parts| {
            if let Some(parts) = parts {
                parts.layout_state().borrow_mut().place(
                    position,
                    self.frame.parent.get(),
                    &builder.context,
                );
            }
        });
    }

    /// The intrinsic size of a child that is a layout node, from its
    /// modifier chain and measure policy, without measuring it. `None` for
    /// another node type, which measures for it.
    pub(super) fn layout_intrinsic(&self, kind: IntrinsicKind) -> Option<f32> {
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

    pub(super) fn measure_cached(&self, constraints: Constraints) -> Option<Rc<MeasuredNode>> {
        if self.frame.measures() {
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
            if self.frame.measures() {
                self.with_pass_layout_state(LayoutState::clear_placed);
            }
            return Some(cached);
        }

        match self.perform_measure(constraints) {
            Ok(measured) => {
                self.force_remeasure.set(false);
                if let Some(cache) = cache {
                    cache.store_measurement(constraints, Rc::clone(&measured));
                }
                Some(measured)
            }
            Err(err) => {
                self.frame.record_error(err);
                None
            }
        }
    }
}

// A record that leaves its parent takes its child out of the parent's count
// of children placed with a z index.
impl Drop for LayoutChildMeasureState {
    fn drop(&mut self) {
        self.frame
            .count_placed_z_index(self.placed_z_index.get(), None);
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "a node's children records in their file"
)]
impl LayoutRuntimeState {
    // Whether the node's last placement gave a child a z index other than 0.
    pub(super) fn places_children_with_z_index(&self) -> bool {
        self.children
            .as_deref()
            .is_some_and(|children| children.frame.placed_with_z_index.get() > 0)
    }
}

pub(super) fn inherited_alignment_lines(
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
