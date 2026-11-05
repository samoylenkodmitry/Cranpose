// What the walk keeps of nodes it pushed in the updates before, lowered:
// the subtrees of children that moved, lowered whole, which a walk pushes
// again where their children now lie while nothing in them changes, and the
// layers of nodes that hold changed nodes, lowered alone, which a walk
// pushes again while the node's own draws stay. A list that scrolls lowers
// each of its rows once, and a row that holds a changing bar lowers its
// containers once.

use cranpose_core::{MemoryApplier, NodeId, collections::map::HashMap};
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

// What the walk keeps of a node, in its slot.
struct Slot {
    node: NodeId,
    // What is kept: absent while a walk pushes it.
    kept: Option<Kept>,
    // The updates that started since one pushed it.
    waited: u8,
}

// What the walk keeps, by node. A walk takes a subtree for each child that
// moved, and a list that scrolls moves many: they are found by the node.
// Each node keeps its slot across the updates that push it, so a walk lends
// what it keeps out of the slot and back with no change to the map, and an
// update's sweep runs over the slots in order.
#[derive(Default)]
pub(crate) struct MovedSubtrees {
    slot_of: HashMap<NodeId, u32>,
    // The slots, each free or in use.
    slots: Vec<Option<Slot>>,
    free: Vec<u32>,
}

// The updates in a row that may end without pushing what the walk keeps of
// a node. A list that a fling slows, or that scrolls by less than a pixel a
// frame, moves its rows in some updates and not in others: half a second at
// 60 frames a second keeps its rows lowered across its pauses. With little
// memory left, one update.
const UNPUSHED_UPDATES: u8 = 30;
const TIGHT_UNPUSHED_UPDATES: u8 = 1;

impl MovedSubtrees {
    // The slot of `node`, when the walk keeps something of it.
    fn slot(&mut self, node: NodeId) -> Option<&mut Slot> {
        if self.slot_of.is_empty() {
            return None;
        }
        let at = *self.slot_of.get(&node)?;
        self.slots[at as usize].as_mut()
    }

    // Takes the subtree of `node` out, when it is kept whole.
    pub(crate) fn take(&mut self, node: NodeId) -> Option<MovedSubtree> {
        let slot = self.slot(node)?;
        match slot.kept.take()? {
            Kept::Subtree(subtree) => Some(subtree),
            head @ Kept::Head(_) => {
                slot.kept = Some(head);
                None
            }
        }
    }

    // Takes the layer of `node` out, when it is kept alone.
    pub(crate) fn take_head(&mut self, node: NodeId) -> Option<KeptHead> {
        let slot = self.slot(node)?;
        match slot.kept.take()? {
            Kept::Head(head) => Some(head),
            subtree @ Kept::Subtree(_) => {
                slot.kept = Some(subtree);
                None
            }
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
        if let Some(slot) = self.slot(node) {
            if let Some(replaced) = slot.kept.replace(kept) {
                replaced.recycle();
            }
            slot.waited = 0;
            return;
        }
        let slot = Slot {
            node,
            kept: Some(kept),
            waited: 0,
        };
        let at = match self.free.pop() {
            Some(at) => {
                self.slots[at as usize] = Some(slot);
                at
            }
            None => {
                self.slots.push(Some(slot));
                (self.slots.len() - 1) as u32
            }
        };
        self.slot_of.insert(node, at);
    }

    // Lets go, as an update starts, in one pass over the slots, what no
    // update pushed in the last `UNPUSHED_UPDATES`, or
    // `TIGHT_UNPUSHED_UPDATES` at `headroom` Tight, and what the update's
    // `marks` make old: a subtree whose node or a node beneath it changed,
    // and a layer whose node changed.
    pub(crate) fn begin_update(
        &mut self,
        (applier, marks): (&MemoryApplier, &DirtyMarks),
        headroom: MemoryHeadroom,
    ) {
        if self.slot_of.is_empty() {
            return;
        }
        let unpushed = match headroom {
            MemoryHeadroom::Tight => TIGHT_UNPUSHED_UPDATES,
            MemoryHeadroom::Ample => UNPUSHED_UPDATES,
        };
        for (at, place) in self.slots.iter_mut().enumerate() {
            let Some(slot) = place else {
                continue;
            };
            slot.waited += 1;
            let stale = slot.waited > unpushed + 1
                || match &slot.kept {
                    Some(Kept::Subtree(_)) => !marks.is_clean(applier, slot.node),
                    Some(Kept::Head(_)) => marks.changed(applier, slot.node),
                    None => true,
                };
            if stale && let Some(slot) = place.take() {
                self.slot_of.remove(&slot.node);
                self.free.push(at as u32);
                if let Some(kept) = slot.kept {
                    kept.recycle();
                }
            }
        }
    }

    // Lets everything go.
    pub(crate) fn clear(&mut self) {
        for slot in self.slots.drain(..).flatten() {
            if let Some(kept) = slot.kept {
                kept.recycle();
            }
        }
        self.slot_of.clear();
        self.free.clear();
    }
}
