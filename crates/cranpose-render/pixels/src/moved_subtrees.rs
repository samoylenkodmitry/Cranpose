// The subtrees of children that did not change, lowered whole, which a walk
// pushes into the scene of their parent's own surface: that scene holds
// none of their old records, so a walk keeps what it lowered of them by
// node, and a layer that turns each frame lowers its children once.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::scene_builder::MovedSubtree;
use cranpose_ui_graphics::MemoryHeadroom;

// What the walk keeps of a node, in its slot.
struct Slot {
    node: NodeId,
    // What is kept: absent while a walk pushes it.
    kept: Option<MovedSubtree>,
    // The updates that started since one pushed it.
    waited: u8,
}

// The slot a walk took a kept subtree out of, which the subtree goes back
// into. It stays the node's slot until the next update starts.
#[derive(Clone, Copy)]
pub(crate) struct SlotIndex(u32);

// What the walk keeps, by node, in a box made when it first keeps one:
// only the children of a layer drawn on its own surface are kept, and most
// windows have none.
#[derive(Default)]
pub(crate) struct MovedSubtrees(Option<Box<Slots>>);

// Each node keeps its slot across the updates that push it, so a walk lends
// what it keeps out of the slot and back into it with no lookup, and an
// update's sweep runs over the slots in order.
#[derive(Default)]
struct Slots {
    slot_of: HashMap<NodeId, u32>,
    // The slots, each free or in use.
    slots: Vec<Option<Slot>>,
    free: Vec<u32>,
}

// The updates in a row that may end without pushing what the walk keeps of
// a node. A layer that turns in some updates and rests in others keeps its
// children lowered across half a second of pauses at 60 frames a second.
// With little memory left, one update.
const UNPUSHED_UPDATES: u8 = 30;
const TIGHT_UNPUSHED_UPDATES: u8 = 1;

impl Slots {
    // The slot of `node`, when the walk keeps its subtree.
    fn slot_index(&self, node: NodeId) -> Option<u32> {
        if self.slot_of.is_empty() {
            return None;
        }
        self.slot_of.get(&node).copied()
    }

    // Takes the subtree of `node` out, with its slot.
    fn take(&mut self, node: NodeId) -> Option<(SlotIndex, MovedSubtree)> {
        let at = self.slot_index(node)?;
        let kept = self.slots[at as usize].as_mut()?.kept.take()?;
        Some((SlotIndex(at), kept))
    }

    // Keeps `subtree`, which the update that runs pushed, in the slot `at`
    // it was taken out of, if any.
    fn keep(&mut self, at: Option<SlotIndex>, subtree: MovedSubtree) {
        let node = subtree.node();
        let at = at.map(|SlotIndex(at)| at).or_else(|| self.slot_index(node));
        if let Some(slot) = at.and_then(|at| self.slots[at as usize].as_mut()) {
            if let Some(replaced) = slot.kept.replace(subtree) {
                replaced.recycle();
            }
            slot.waited = 0;
            return;
        }
        let slot = Slot {
            node,
            kept: Some(subtree),
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

    // Lets go the subtree the walk keeps of `node`, which an update changed
    // or holds a node an update changed.
    fn let_go(&mut self, node: NodeId) {
        if let Some(at) = self.slot_index(node) {
            self.free_slot(at);
        }
    }

    fn free_slot(&mut self, at: u32) {
        if let Some(slot) = self.slots[at as usize].take() {
            self.slot_of.remove(&slot.node);
            self.free.push(at);
            if let Some(kept) = slot.kept {
                kept.recycle();
            }
        }
    }

    // Lets go, as an update starts, in one pass over the slots, what no
    // update pushed in the last `UNPUSHED_UPDATES`, or
    // `TIGHT_UNPUSHED_UPDATES` at `headroom` Tight. The marks of the update
    // let go before what it made old.
    fn begin_update(&mut self, headroom: MemoryHeadroom) {
        if self.slot_of.is_empty() {
            return;
        }
        let unpushed = match headroom {
            MemoryHeadroom::Tight => TIGHT_UNPUSHED_UPDATES,
            MemoryHeadroom::Ample => UNPUSHED_UPDATES,
        };
        for at in 0..self.slots.len() {
            let Some(slot) = &mut self.slots[at] else {
                continue;
            };
            slot.waited += 1;
            if slot.waited > unpushed + 1 || slot.kept.is_none() {
                self.free_slot(at as u32);
            }
        }
    }

    // Lets everything go.
    fn clear(&mut self) {
        for slot in self.slots.drain(..).flatten() {
            if let Some(kept) = slot.kept {
                kept.recycle();
            }
        }
        self.slot_of.clear();
        self.free.clear();
    }
}

impl MovedSubtrees {
    // Takes the subtree of `node` out, with its slot.
    pub(crate) fn take(&mut self, node: NodeId) -> Option<(SlotIndex, MovedSubtree)> {
        self.0.as_mut()?.take(node)
    }

    // Keeps `subtree`, which the update that runs pushed, in the slot `at`
    // it was taken out of, if any.
    pub(crate) fn keep(&mut self, at: Option<SlotIndex>, subtree: MovedSubtree) {
        self.0.get_or_insert_default().keep(at, subtree);
    }

    // Lets go the subtree the walk keeps of `node`, which an update changed
    // or holds a node an update changed.
    pub(crate) fn let_go(&mut self, node: NodeId) {
        if let Some(slots) = self.0.as_mut() {
            slots.let_go(node);
        }
    }

    // Lets go what no update pushed lately, as `Slots::begin_update` says.
    pub(crate) fn begin_update(&mut self, headroom: MemoryHeadroom) {
        if let Some(slots) = self.0.as_mut() {
            slots.begin_update(headroom);
        }
    }

    // Lets everything go.
    pub(crate) fn clear(&mut self) {
        if let Some(slots) = self.0.as_mut() {
            slots.clear();
        }
    }
}
