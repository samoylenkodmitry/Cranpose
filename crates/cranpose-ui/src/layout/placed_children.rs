//! The children that a node's last measure placed, in its order, each with
//! what the measure kept of it in the child's record.

use std::rc::Rc;

use cranpose_core::{Applier, MemoryApplier, NodeError, NodeId};

use super::{
    LayoutBuilderState, MeasuredChild, MeasuredNode, live_layout_node::read_live_layout_node,
};
use crate::{
    modifier::Point,
    subcompose_layout::SubcomposeLayoutNode,
    widgets::nodes::{LayoutNode, LayoutParts},
};

// A layout node lists its children in its records; a subcompose node lists
// the children its last measure placed.
pub(super) enum PlacedChildren {
    Layout(Rc<LayoutParts>),
    Subcompose(NodeId),
    Leaf,
}

impl PlacedChildren {
    // Out of line, as each of the walks below reads children through one
    // copy of these.
    #[inline(never)]
    pub(super) fn of(applier: &mut MemoryApplier, node_id: NodeId) -> Self {
        let Ok(node) = applier.get_mut(node_id) else {
            return Self::Leaf;
        };
        let any = node.as_any_mut();
        if let Some(layout) = any.downcast_ref::<LayoutNode>() {
            return Self::Layout(Rc::clone(layout.layout_parts()));
        }
        if any.downcast_ref::<SubcomposeLayoutNode>().is_some() {
            return Self::Subcompose(node_id);
        }
        Self::Leaf
    }

    // How many children the list holds, placed or not.
    #[inline(never)]
    pub(super) fn len(&self, applier: &mut MemoryApplier) -> usize {
        match self {
            Self::Layout(parts) => parts.runtime_state().borrow().child_states().len(),
            Self::Subcompose(node_id) => applier
                .with_node::<SubcomposeLayoutNode, _>(*node_id, |node| {
                    node.with_active_children(<[NodeId]>::len)
                })
                .unwrap_or_default(),
            Self::Leaf => 0,
        }
    }

    // The child at `index`, with what the last measure kept of it: `None`
    // there for a child the measure did not place. A subcompose node placed
    // each of its children; one of another kind, which keeps no record,
    // reads as placed at the origin. `None` past the last child.
    #[inline(never)]
    pub(super) fn get(
        &self,
        applier: &mut MemoryApplier,
        index: usize,
    ) -> Result<Option<(NodeId, Option<MeasuredChild>)>, NodeError> {
        match self {
            Self::Layout(parts) => Ok(parts
                .runtime_state()
                .borrow()
                .child_states()
                .get(index)
                .map(|child| {
                    let record = child.record();
                    (record.child_id(), record.in_parent.get())
                })),
            Self::Subcompose(node_id) => {
                let child = applier
                    .with_node::<SubcomposeLayoutNode, _>(*node_id, |node| {
                        node.with_active_children(|children| children.get(index).copied())
                    })
                    .ok()
                    .flatten();
                let Some(child) = child else {
                    return Ok(None);
                };
                let kept = read_live_layout_node(applier, child, |node| node.in_parent())?;
                Ok(Some((child, Some(kept.flatten().unwrap_or_default()))))
            }
            Self::Leaf => Ok(None),
        }
    }

    // Runs `visit` on each placed child in order, with where the measure
    // placed it and the measurement it holds in its cache: a leaf of no size
    // for a child that holds none, as a node of another kind measures.
    #[inline(never)]
    pub(super) fn visit(
        &self,
        applier: &mut MemoryApplier,
        visit: &mut dyn FnMut(
            &mut MemoryApplier,
            NodeId,
            Point,
            &MeasuredNode,
        ) -> Result<(), NodeError>,
    ) -> Result<(), NodeError> {
        let mut index = 0;
        while let Some((child, kept)) = self.get(applier, index)? {
            index += 1;
            let Some(kept) = kept else {
                continue;
            };
            let held =
                read_live_layout_node(applier, child, |node| node.cache().held_measurement())
                    .ok()
                    .flatten()
                    .flatten();
            match held {
                Some(held) => visit(applier, child, kept.offset, &held)?,
                None => visit(applier, child, kept.offset, &MeasuredNode::default())?,
            }
        }
        Ok(())
    }
}

// How many children `node_id` lists, for the measure telemetry's log alone.
#[cold]
#[inline(never)]
pub(super) fn placed_child_count(builder: &LayoutBuilderState, node_id: NodeId) -> usize {
    builder
        .with_applier_result(|applier| Ok(PlacedChildren::of(applier, node_id).len(applier)))
        .unwrap_or_default()
}
