// The room the node lists a surface hands from frame to frame keep: a
// screen's first frame names every node, while later frames name a few.

use cranpose_core::{NodeId, collections::recent_peak::RecentPeak};
use cranpose_render_common::NodeUpdate;

// The room each list keeps whatever recent frames named.
const LIST_ROOM_KEPT: usize = 64;

#[derive(Default)]
pub(crate) struct ListRoom {
    nodes: [RecentPeak; 5],
    updates: RecentPeak,
}

impl ListRoom {
    // Notes what the frame left in the surface's lists and gives back the
    // room recent frames left unused, keeping each list's entries.
    pub(crate) fn trim(
        &mut self,
        nodes: [&mut Vec<NodeId>; 5],
        updates: &mut Vec<(NodeId, NodeUpdate)>,
    ) {
        // Most frames name a few nodes into small lists.
        for (peak, list) in self.nodes.iter_mut().zip(nodes) {
            peak.trim_past_floor(list, LIST_ROOM_KEPT);
        }
        self.updates.trim_past_floor(updates, LIST_ROOM_KEPT);
    }
}
