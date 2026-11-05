// What the walk keeps of nodes it pushed in the updates before, lowered:
// the subtrees of children that moved, lowered whole, which a walk pushes
// again where their children now lie while nothing in them changes, and the
// layers of nodes that hold changed nodes, lowered alone, which a walk
// pushes again while the node's own draws stay. A list that scrolls lowers
// each of its rows once, and a row that holds a changing bar lowers its
// containers once.

use cranpose_core::{
    MemoryApplier, NodeId,
    collections::map::{Entry, HashMap},
};
use cranpose_render_common::scene_builder::{KeptHead, MovedSubtree};
use cranpose_ui_graphics::MemoryHeadroom;

use crate::layout_walk::DirtyMarks;

// What the walk keeps of a node.
enum Kept {
    // Its subtree lowered whole: the node and every node beneath it did not
    // change.
    Subtree(MovedSubtree),
    // Its own layer lowered alone: the node did not change, nodes beneath
    // it may have.
    Head(KeptHead),
}

impl Kept {
    fn recycle(self) {
        match self {
            Self::Subtree(subtree) => subtree.recycle(),
            Self::Head(head) => head.recycle(),
        }
    }
}

// What the walk keeps by node, with the number of updates that started
// since one pushed it. A walk takes a subtree for each child that moved, and
// a list that scrolls moves many: they are found by the node.
#[derive(Default)]
pub(crate) struct MovedSubtrees {
    kept: HashMap<NodeId, (Kept, u8)>,
}

// The updates in a row that may end without pushing what the walk keeps of
// a node. A list that a fling slows, or that scrolls by less than a pixel a
// frame, moves its rows in some updates and not in others: half a second at
// 60 frames a second keeps its rows lowered across its pauses. With little
// memory left, one update.
const UNPUSHED_UPDATES: u8 = 30;
const TIGHT_UNPUSHED_UPDATES: u8 = 1;

impl MovedSubtrees {
    // Takes the subtree of `node` out, when it is kept whole.
    pub(crate) fn take(&mut self, node: NodeId) -> Option<MovedSubtree> {
        if self.kept.is_empty() {
            return None;
        }
        let Entry::Occupied(entry) = self.kept.entry(node) else {
            return None;
        };
        match entry.get().0 {
            Kept::Subtree(_) => match entry.remove().0 {
                Kept::Subtree(subtree) => Some(subtree),
                Kept::Head(_) => None,
            },
            Kept::Head(_) => None,
        }
    }

    // Takes the layer of `node` out, when it is kept alone.
    pub(crate) fn take_head(&mut self, node: NodeId) -> Option<KeptHead> {
        if self.kept.is_empty() {
            return None;
        }
        let Entry::Occupied(entry) = self.kept.entry(node) else {
            return None;
        };
        match entry.get().0 {
            Kept::Head(_) => match entry.remove().0 {
                Kept::Head(head) => Some(head),
                Kept::Subtree(_) => None,
            },
            Kept::Subtree(_) => None,
        }
    }

    // Keeps `subtree`, which the update that runs pushed.
    pub(crate) fn keep(&mut self, subtree: MovedSubtree) {
        self.keep_as(subtree.node(), Kept::Subtree(subtree));
    }

    // Keeps `head`, which the update that runs pushed.
    pub(crate) fn keep_head(&mut self, head: KeptHead) {
        self.keep_as(head.node(), Kept::Head(head));
    }

    fn keep_as(&mut self, node: NodeId, kept: Kept) {
        if let Some((replaced, _)) = self.kept.insert(node, (kept, 0)) {
            replaced.recycle();
        }
    }

    // Lets go, as an update starts, in one pass, what no update pushed in
    // the last `UNPUSHED_UPDATES`, or `TIGHT_UNPUSHED_UPDATES` at `headroom`
    // Tight, and what the update's `marks` make old: a subtree whose node or
    // a node beneath it changed, and a layer whose node changed.
    pub(crate) fn begin_update(
        &mut self,
        (applier, marks): (&MemoryApplier, &DirtyMarks),
        headroom: MemoryHeadroom,
    ) {
        if self.kept.is_empty() {
            return;
        }
        let unpushed = match headroom {
            MemoryHeadroom::Tight => TIGHT_UNPUSHED_UPDATES,
            MemoryHeadroom::Ample => UNPUSHED_UPDATES,
        };
        let stale = self.kept.extract_if(|&node, (kept, waited)| {
            *waited += 1;
            *waited > unpushed + 1
                || match kept {
                    Kept::Subtree(_) => !marks.is_clean(applier, node),
                    Kept::Head(_) => marks.changed(applier, node),
                }
        });
        for (_, (kept, _)) in stale {
            kept.recycle();
        }
    }

    // Lets everything go.
    pub(crate) fn clear(&mut self) {
        for (_, (kept, _)) in self.kept.drain() {
            kept.recycle();
        }
    }
}
