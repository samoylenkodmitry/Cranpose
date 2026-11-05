// The subtrees of the children that moved in the updates before, lowered
// whole, which a walk pushes again where their children now lie while
// nothing in them changes: a list that scrolls lowers each of its rows
// once.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::scene_builder::MovedSubtree;
use cranpose_ui_graphics::MemoryHeadroom;

// Each subtree by its child, with the number of updates that ended since one
// pushed it. A walk takes a subtree for each child that moved, and a list
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

    // Lets go the subtrees no update pushed for longer than
    // `UNPUSHED_UPDATES`, or `TIGHT_UNPUSHED_UPDATES` at `headroom` Tight,
    // as an update ends.
    pub(crate) fn end_update(&mut self, headroom: MemoryHeadroom) {
        if self.kept.is_empty() {
            return;
        }
        let unpushed = match headroom {
            MemoryHeadroom::Tight => TIGHT_UNPUSHED_UPDATES,
            MemoryHeadroom::Ample => UNPUSHED_UPDATES,
        };
        let stale = self.kept.extract_if(|_, (_, ended)| {
            *ended += 1;
            *ended > unpushed + 1
        });
        for (_, (subtree, _)) in stale {
            subtree.recycle();
        }
    }

    // Lets go the subtrees whose nodes are not `clean` as an update starts:
    // the node or a node beneath it changed, so what the subtree draws may
    // be old.
    pub(crate) fn drop_changed(&mut self, clean: impl Fn(NodeId) -> bool) {
        if self.kept.is_empty() {
            return;
        }
        for (_, (subtree, _)) in self.kept.extract_if(|node, _| !clean(*node)) {
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
