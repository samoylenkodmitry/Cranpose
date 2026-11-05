// The subtrees of the children that moved in the updates before, lowered
// whole, which a walk pushes again where their children now lie while
// nothing in them changes: a list that scrolls lowers each of its rows
// once.

use cranpose_core::NodeId;
use cranpose_render_common::scene_builder::MovedSubtree;

// Each subtree with the number of updates that ended since one pushed it.
#[derive(Default)]
pub(crate) struct MovedSubtrees {
    kept: Vec<(MovedSubtree, u8)>,
}

// The updates in a row that may end without pushing a subtree it keeps: a
// list that scrolls by less than a pixel a frame moves its rows in most
// updates, not in each.
const UNPUSHED_UPDATES: u8 = 1;

impl MovedSubtrees {
    // Takes the subtree of `node` out, when it is kept.
    pub(crate) fn take(&mut self, node: NodeId) -> Option<MovedSubtree> {
        let at = self
            .kept
            .iter()
            .position(|(subtree, _)| subtree.node() == node)?;
        Some(self.kept.swap_remove(at).0)
    }

    // Keeps `subtree`, which the update that runs pushed.
    pub(crate) fn keep(&mut self, subtree: MovedSubtree) {
        self.kept.push((subtree, 0));
    }

    // Lets go the subtrees no update pushed for longer than
    // `UNPUSHED_UPDATES`, as an update ends.
    pub(crate) fn end_update(&mut self) {
        let stale = self.kept.extract_if(.., |(_, ended)| {
            *ended += 1;
            *ended > UNPUSHED_UPDATES + 1
        });
        for (subtree, _) in stale {
            subtree.recycle();
        }
    }

    // Lets go the subtrees whose nodes are not `clean` as an update starts:
    // the node or a node beneath it changed, so what the subtree draws may
    // be old.
    pub(crate) fn drop_changed(&mut self, clean: impl Fn(NodeId) -> bool) {
        for (subtree, _) in self
            .kept
            .extract_if(.., |(subtree, _)| !clean(subtree.node()))
        {
            subtree.recycle();
        }
    }

    // Lets every subtree go.
    pub(crate) fn clear(&mut self) {
        for (subtree, _) in self.kept.drain(..) {
            subtree.recycle();
        }
    }
}
