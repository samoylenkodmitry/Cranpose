// The subtrees of the children that moved in the updates before, lowered
// whole, which a walk pushes again where their children now lie while
// nothing in them changes: a list that scrolls lowers each of its rows
// once.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::scene_builder::MovedSubtree;
use cranpose_ui_graphics::MemoryHeadroom;

// Each subtree by its child, with the number of updates that started since
// one pushed it. A walk takes a subtree for each child that moved, and a list
// that scrolls moves many: they are found by the child.
#[derive(Default)]
pub(crate) struct MovedSubtrees {
    kept: HashMap<NodeId, (MovedSubtree, u8)>,
}

// The updates in a row that may end without pushing a subtree it keeps. A
// list that a fling slows, or that scrolls by less than a pixel a frame,
// moves its rows in some updates and not in others: half a second at 60
// frames a second keeps its rows lowered across its pauses. With little
// memory left, one update.
const UNPUSHED_UPDATES: u8 = 30;
const TIGHT_UNPUSHED_UPDATES: u8 = 1;

impl MovedSubtrees {
    // Takes the subtree of `node` out, when it is kept.
    pub(crate) fn take(&mut self, node: NodeId) -> Option<MovedSubtree> {
        if self.kept.is_empty() {
            return None;
        }
        self.kept.remove(&node).map(|(subtree, _)| subtree)
    }

    // Keeps `subtree`, which the update that runs pushed.
    pub(crate) fn keep(&mut self, subtree: MovedSubtree) {
        if let Some((replaced, _)) = self.kept.insert(subtree.node(), (subtree, 0)) {
            replaced.recycle();
        }
    }

    // Lets go, as an update starts, in one pass, the subtrees no update
    // pushed in the last `UNPUSHED_UPDATES`, or `TIGHT_UNPUSHED_UPDATES` at
    // `headroom` Tight, and those whose nodes are not `clean`: the node or a
    // node beneath it changed, so what the subtree draws may be old.
    pub(crate) fn begin_update(
        &mut self,
        clean: impl Fn(NodeId) -> bool,
        headroom: MemoryHeadroom,
    ) {
        if self.kept.is_empty() {
            return;
        }
        let unpushed = match headroom {
            MemoryHeadroom::Tight => TIGHT_UNPUSHED_UPDATES,
            MemoryHeadroom::Ample => UNPUSHED_UPDATES,
        };
        let stale = self.kept.extract_if(|node, (_, waited)| {
            *waited += 1;
            *waited > unpushed + 1 || !clean(*node)
        });
        for (_, (subtree, _)) in stale {
            subtree.recycle();
        }
    }

    // Lets every subtree go.
    pub(crate) fn clear(&mut self) {
        for (_, (subtree, _)) in self.kept.drain() {
            subtree.recycle();
        }
    }
}
