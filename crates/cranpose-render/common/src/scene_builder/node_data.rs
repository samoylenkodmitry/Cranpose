// What the scene builder reads of a layout node in the applier: its layout,
// its modifier slices and its children in the order they draw.

use std::any::Any;

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_ui::{LayoutNode, Point, Size, SubcomposeLayoutNode, widgets::LayoutState};

use super::{Level, SnapshotNodeData};

pub(super) fn read_placed_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    level: Level,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    read_node_data(applier, node_id, |mut data| {
        if data.window_root {
            if level == Level::Child {
                return None;
            }
            data.layout_state = data.layout_state.at_origin();
        }
        if !data.layout_state.is_placed() {
            return None;
        }
        Some(read(data))
    })?
}

#[derive(Clone, Copy)]
pub(super) struct PlacedGeometry {
    pub(super) position: Point,
    pub(super) size: Size,
}

impl PlacedGeometry {
    fn of_placed(state: &LayoutState) -> Option<Self> {
        state.is_placed().then(|| Self {
            position: state.position(),
            size: state.size(),
        })
    }
}

pub(super) fn placed_child_geometry(
    applier: &MemoryApplier,
    node_id: NodeId,
) -> Option<PlacedGeometry> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        if node.is_window_root() {
            return None;
        }
        return node.with_layout_state(PlacedGeometry::of_placed);
    }
    PlacedGeometry::of_placed(&node.downcast_ref::<SubcomposeLayoutNode>()?.layout_state())
}

pub(super) fn read_node_data<R>(
    applier: &MemoryApplier,
    node_id: NodeId,
    read: impl FnOnce(SnapshotNodeData<'_>) -> R,
) -> Option<R> {
    let node: &dyn Any = applier.get_ref(node_id).ok()?;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        let z_order = z_ordered_children(applier, node_id, node.children());
        return Some(read(SnapshotNodeData {
            layout_state: node.layout_state(),
            modifier_slices: node.modifier_slices_snapshot(),
            children: z_order.as_deref().unwrap_or_else(|| node.children()),
            window_root: node.is_window_root(),
        }));
    }
    let node = node.downcast_ref::<SubcomposeLayoutNode>()?;
    let layout_state = node.layout_state();
    let modifier_slices = node.modifier_slices_snapshot();
    Some(node.with_active_children(|children| {
        let z_order = z_ordered_children(applier, node_id, children);
        read(SnapshotNodeData {
            layout_state,
            modifier_slices,
            children: z_order.as_deref().unwrap_or(children),
            window_root: false,
        })
    }))
}

#[inline]
fn z_ordered_children(
    applier: &MemoryApplier,
    parent: NodeId,
    children: &[NodeId],
) -> Option<Vec<NodeId>> {
    if children.len() < 2 || !cranpose_ui::z_index_in_use() {
        return None;
    }
    z_sorted_children(applier, parent, children)
}

#[inline(never)]
fn z_sorted_children(
    applier: &MemoryApplier,
    parent: NodeId,
    children: &[NodeId],
) -> Option<Vec<NodeId>> {
    if !cranpose_ui::z_index_modifier_in_use() && !places_children_with_z_index(applier, parent) {
        return None;
    }
    let mut previous = f32::NEG_INFINITY;
    let in_order = children.iter().all(|&child| {
        let z_index = node_z_index(applier, child);
        let ordered = z_index >= previous;
        previous = z_index;
        ordered
    });
    if in_order {
        return None;
    }
    let mut ordered: Vec<(f32, NodeId)> = children
        .iter()
        .map(|&child| (node_z_index(applier, child), child))
        .collect();
    ordered.sort_by(|a, b| a.0.total_cmp(&b.0));
    Some(ordered.into_iter().map(|(_, child)| child).collect())
}

fn places_children_with_z_index(applier: &MemoryApplier, node_id: NodeId) -> bool {
    let Ok(node) = applier.get_ref(node_id) else {
        return false;
    };
    let node: &dyn Any = node;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return node.with_layout_state(LayoutState::places_children_with_z_index);
    }
    node.downcast_ref::<SubcomposeLayoutNode>()
        .is_some_and(|node| node.layout_state().places_children_with_z_index())
}

fn node_z_index(applier: &MemoryApplier, node_id: NodeId) -> f32 {
    let Ok(node) = applier.get_ref(node_id) else {
        return 0.0;
    };
    let node: &dyn Any = node;
    if let Some(node) = node.downcast_ref::<LayoutNode>() {
        return node.modifier_slices_snapshot().z_index()
            + node.with_layout_state(LayoutState::z_index);
    }
    node.downcast_ref::<SubcomposeLayoutNode>()
        .map_or(0.0, |node| {
            node.modifier_slices_snapshot().z_index() + node.layout_state().z_index()
        })
}
