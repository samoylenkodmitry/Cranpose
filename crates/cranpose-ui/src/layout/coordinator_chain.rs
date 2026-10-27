// A layout node's chain of coordinators: its layout modifiers, each
// measuring the next and the last its measure policy, with the sizes and
// offsets each measure leaves on them.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::{InvalidationKind, ModifierNodeContext, NodeCapabilities};
use cranpose_ui_layout::{AlignmentLines, Constraints, MeasurePolicy, Placement};

use super::{
    LayoutChildMeasureState, LayoutNodeContext, ModifierChainInputs,
    core::{Measurable, Placeable},
    inherited_alignment_lines, policy_intrinsic,
};
use crate::{
    modifier::{Point, Rect as GeometryRect, Size},
    widgets::nodes::{IntrinsicKind, LayoutNode},
};

/// The intrinsic size `kind` names, of a measurable.
fn measurable_intrinsic(measurable: &dyn Measurable, kind: IntrinsicKind) -> f32 {
    match kind {
        IntrinsicKind::MinWidth(height) => measurable.min_intrinsic_width(height),
        IntrinsicKind::MaxWidth(height) => measurable.max_intrinsic_width(height),
        IntrinsicKind::MinHeight(width) => measurable.min_intrinsic_height(width),
        IntrinsicKind::MaxHeight(width) => measurable.max_intrinsic_height(width),
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

pub(super) struct CoordinatorFrame<'a> {
    measure_policy: &'a Rc<dyn MeasurePolicy>,
    scope: &'a dyn cranpose_ui_layout::MeasureScope,
    measurables: &'a [Box<dyn Measurable>],
    child_states: &'a [Rc<LayoutChildMeasureState>],
    placements: RefCell<&'a mut Vec<Placement>>,
    placement_indices: RefCell<&'a mut Vec<usize>>,
    context: RefCell<LayoutNodeContext>,
}

impl<'a> CoordinatorFrame<'a> {
    pub(super) fn new(
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

    pub(super) fn take_invalidations(&self) -> Vec<InvalidationKind> {
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
    /// The constraints this node last measured under.
    measured_constraints: Cell<Constraints>,
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
            measured_constraints: Cell::new(Constraints::tight(0.0, 0.0)),
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
pub(super) struct CoordinatorChain {
    nodes: Vec<CoordinatorNode>,
    /// The size the node's own measure policy measured its content at.
    inner_size: Cell<Size>,
    /// The constraints the node's own measure policy last measured under.
    inner_constraints: Cell<Option<Constraints>>,
    /// The chain revision the nodes were last synced at and the inputs it
    /// gave: a node whose modifiers did not change since measures again
    /// without walking its chain.
    synced: Option<(u64, ModifierChainInputs)>,
    // Whether a measure placed the coordinators, whose rects the nodes'
    // sizes and offsets give, and whether one of them moved since the scene
    // last heard of the node.
    placed: Cell<bool>,
    moved: Cell<bool>,
}

impl CoordinatorChain {
    pub(super) fn sync(&mut self, node: &LayoutNode) -> ModifierChainInputs {
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
        // Other modifiers may place other coordinators.
        self.moved.set(true);
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
        // Exactly: most chains hold one layout modifier, where growth would
        // make room for four.
        self.nodes.reserve_exact(len);
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

    /// What the last measure through this chain holds for: the policy's own
    /// range, then each layout modifier from the innermost out turns its
    /// content's range into its own; any other node passes its constraints
    /// through.
    pub(super) fn measured_hold(
        &self,
        density: f32,
        policy: &dyn MeasurePolicy,
        measurables: &[Box<dyn Measurable>],
    ) -> Option<cranpose_ui_layout::ConstraintsHold> {
        let inner_size = self.inner_size.get();
        let mut wrapped = cranpose_ui_layout::WrappedHold {
            size: inner_size,
            hold: self
                .inner_constraints
                .get()
                .and_then(|constraints| policy.measure_hold(measurables, constraints, inner_size)),
        };
        for node in self.nodes.iter().rev() {
            let size = node.measured_size.get();
            let hold = match node.node.borrow().as_layout_node() {
                Some(layout) => {
                    layout.measure_hold(density, node.measured_constraints.get(), size, wrapped)
                }
                None => wrapped.hold,
            };
            wrapped = cranpose_ui_layout::WrappedHold { size, hold };
        }
        wrapped.hold
    }

    /// Whether the policy's last measure through this chain ignored its
    /// children's sizes. The layout modifiers around it see only its size,
    /// which then stays as it is.
    pub(super) fn ignores_child_sizes(
        &self,
        policy: &dyn MeasurePolicy,
        measurables: &[Box<dyn Measurable>],
    ) -> bool {
        self.inner_constraints
            .get()
            .is_some_and(|constraints| policy.ignores_child_sizes(measurables, constraints))
    }

    pub(super) fn measure_from(
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
            self.place(&self.inner_size, measurement.size);
            self.inner_constraints.set(Some(constraints));
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
        node.measured_constraints.set(constraints);

        let Some(layout_node) = node_borrow.as_layout_node() else {
            let placeable = wrapped.measure(constraints);
            self.place(
                &node.measured_size,
                Size {
                    width: placeable.width(),
                    height: placeable.height(),
                },
            );
            let child_accumulated = self.total_content_offset_from(index + 1);
            self.place(&node.accumulated_offset, child_accumulated);
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

        self.place(&node.measured_size, result.size);
        let local_offset = Point {
            x: result.placement_offset_x,
            y: result.placement_offset_y,
        };
        let child_accumulated = self.total_content_offset_from(index + 1);
        let accumulated = Point {
            x: local_offset.x + child_accumulated.x,
            y: local_offset.y + child_accumulated.y,
        };
        self.place(&node.accumulated_offset, accumulated);

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

    pub(super) fn intrinsic_from(
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

    // Sets a size or an offset the coordinators' rects come from, noting a
    // coordinator that moved: a layer bounded by an inner coordinator
    // changes when that coordinator does, though the node's size may not.
    fn place<T: Copy + PartialEq>(&self, cell: &Cell<T>, value: T) {
        if cell.replace(value) != value {
            self.moved.set(true);
        }
    }

    // Notes that a measure placed the coordinators, and says whether one of
    // them moved since the last note.
    pub(super) fn take_moved(&self) -> bool {
        self.placed.set(true);
        self.moved.take()
    }

    // Where the last measure put the coordinator `ordinal`, relative to the
    // node drawn at its offset: for each layout modifier, from the outermost
    // in, where it was placed and its size, then the node's content. A node
    // of `node_size` without layout modifiers has its content alone, which
    // fills it. `None` before a measure placed them.
    pub(super) fn placed_rect(&self, ordinal: usize, node_size: Size) -> Option<GeometryRect> {
        let (_, inputs) = self.synced.filter(|_| self.placed.get())?;
        if !inputs.uses_chain {
            return (ordinal == 0).then(|| GeometryRect::from_size(node_size));
        }
        let content = self.total_content_offset_from(0);
        let placed = |inner_offset: Point, size: Size| GeometryRect {
            x: content.x - inner_offset.x - inputs.offset.x,
            y: content.y - inner_offset.y - inputs.offset.y,
            width: size.width,
            height: size.height,
        };
        match self.nodes.get(ordinal) {
            Some(node) => Some(placed(
                node.accumulated_offset.get(),
                node.measured_size.get(),
            )),
            None => (ordinal == self.nodes.len())
                .then(|| placed(Point::default(), self.inner_size.get())),
        }
    }

    fn total_content_offset_from(&self, index: usize) -> Point {
        self.nodes
            .get(index)
            .map(|node| node.accumulated_offset.get())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn debug_ptrs(&self) -> Vec<usize> {
        self.nodes.iter().map(CoordinatorNode::ptr).collect()
    }
}
