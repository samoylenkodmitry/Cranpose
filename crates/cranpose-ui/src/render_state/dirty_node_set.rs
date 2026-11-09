use cranpose_core::{
    NodeId,
    collections::{map::HashSet, recent_peak::RecentPeak},
};

// The room a set keeps whatever recent takes held.
const ROOM_KEPT: usize = 64;

pub(super) struct DirtyNodeSet {
    pub(super) dirty_nodes: HashSet<NodeId>,
    // The most nodes a take found lately: a screen's first frame names every
    // node, while later frames name a few.
    recent: RecentPeak,
}

impl DirtyNodeSet {
    pub(super) fn new() -> Self {
        Self {
            dirty_nodes: HashSet::default(),
            recent: RecentPeak::default(),
        }
    }

    // Notes a take of the set and gives back the room recent takes left
    // unused, once the set is empty.
    fn note_take(&mut self, len: usize) {
        self.recent.note(len);
        if let Some(room) = self
            .recent
            .shrink_to(self.dirty_nodes.capacity(), ROOM_KEPT)
        {
            self.dirty_nodes.shrink_to(room);
        }
    }

    pub(super) fn schedule_repass(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    pub(super) fn has_pending_repass(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    // A drain clears every control byte of the table when it ends, even of
    // an empty one: most frames take sets that are empty.
    pub(super) fn take_dirty_nodes(&mut self) -> Vec<NodeId> {
        let len = self.dirty_nodes.len();
        let nodes = if len == 0 {
            Vec::new()
        } else {
            self.dirty_nodes.drain().collect()
        };
        self.note_take(len);
        nodes
    }

    pub(super) fn take_dirty_nodes_into(&mut self, output: &mut Vec<NodeId>) {
        output.clear();
        let len = self.dirty_nodes.len();
        if len > 0 {
            output.extend(self.dirty_nodes.drain());
        }
        self.note_take(len);
    }

    pub(super) fn dirty_nodes_snapshot(&self) -> Vec<NodeId> {
        let mut nodes = self.dirty_nodes.iter().copied().collect::<Vec<_>>();
        nodes.sort_unstable();
        nodes
    }
}
