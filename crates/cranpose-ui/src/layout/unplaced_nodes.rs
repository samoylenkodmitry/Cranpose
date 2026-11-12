//! The nodes a layout pass left unplaced.

use cranpose_core::MemoryApplier;

use super::read_live_layout_node;
use crate::render_state::AppContext;

// Keeps for the scene phase every node `pass` found placed and left
// unplaced: what the scene drew for it leaves the scene. The pass kept the
// nodes it cleared, so only those are read. Out of line: a pass rarely
// unplaces nodes.
#[inline(never)]
pub(super) fn record_unplaced_nodes(applier: &mut MemoryApplier, pass: u64, context: &AppContext) {
    let placement = context.placement_pass();
    let mut cleared = placement.take_cleared();
    cleared.retain(|&node| {
        let unplaced = read_live_layout_node(applier, node, |node| node.state().unplaced_in(pass));
        matches!(unplaced, Ok(Some(true)))
    });
    placement.keep_unplaced(&cleared);
    placement.keep_cleared(cleared);
}
