//! Spacer widget implementation

#![allow(non_snake_case)]

use super::nodes::SpacerNode;
use crate::composable;
use crate::layout::mark_measure_dirty;
use crate::modifier::Size;
use compose_core::NodeId;

#[composable]
pub fn Spacer(size: Size) -> NodeId {
    let id =
        compose_core::with_current_composer(|composer| composer.emit_node(|| SpacerNode { size }));
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut SpacerNode| {
        if node.size != size {
            node.size = size;
            mark_measure_dirty(id);
        }
    }) {
        debug_assert!(false, "failed to update Spacer node: {err}");
    }
    id
}
