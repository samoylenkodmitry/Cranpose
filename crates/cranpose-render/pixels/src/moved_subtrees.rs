// The subtrees of the children that moved in the update before, lowered
// whole, which a walk pushes again where their children now lie while
// nothing in them changes: a list that scrolls lowers each of its rows
// once.

use cranpose_core::NodeId;
use cranpose_render_common::scene_builder::MovedSubtree;

// Each subtree with whether the update that runs pushed it.
#[derive(Default)]
pub(crate) struct MovedSubtrees {
    kept: Vec<(MovedSubtree, bool)>,
}

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
        self.kept.push((subtree, true));
    }

    // Lets go the subtrees the update did not push, as it ends.
    pub(crate) fn end_update(&mut self) {
        for (subtree, _) in self.kept.extract_if(.., |(_, pushed)| !*pushed) {
            subtree.recycle();
        }
        for (_, pushed) in &mut self.kept {
            *pushed = false;
        }
    }

    // Lets every subtree go.
    pub(crate) fn clear(&mut self) {
        for (subtree, _) in self.kept.drain(..) {
            subtree.recycle();
        }
    }
}
