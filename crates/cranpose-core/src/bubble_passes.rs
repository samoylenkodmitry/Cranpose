//! Which ancestors the dirty bubbles of one batch of commands passed.
//!
//! A bubble walks from a node to the root and marks every ancestor as having
//! a dirty descendant. It cannot stop at an ancestor that is marked already:
//! a scoped layout repass can leave a dirty node under a clean ancestor. Within
//! a batch no layout pass runs, so a walk may stop at an ancestor an earlier
//! walk of the batch passed with the same flags: that walk marked every
//! ancestor above it. A node that changes its parent, or code the batch runs,
//! makes the batch forget what its walks passed.

/// The low bits of a pass hold the bubble's flags, the rest its round.
const FLAG_BITS: u32 = 3;
const FLAG_MASK: u32 = (1 << FLAG_BITS) - 1;
const MAX_ROUND: u32 = u32::MAX >> FLAG_BITS;

#[derive(Default)]
pub(crate) struct BubblePasses {
    /// How many batches are open; walks note passes only inside one.
    open_batches: u32,
    round: u32,
    /// Per node index, the round of the last pass and the flags the round's
    /// passes carried.
    passes: Vec<u32>,
}

impl BubblePasses {
    pub(crate) fn begin_batch(&mut self) {
        self.open_batches += 1;
        self.next_round();
    }

    pub(crate) fn end_batch(&mut self) {
        self.open_batches = self.open_batches.saturating_sub(1);
    }

    pub(crate) fn forget(&mut self) {
        if self.open_batches > 0 {
            self.next_round();
        }
    }

    fn next_round(&mut self) {
        if self.round >= MAX_ROUND {
            self.passes.fill(0);
            self.round = 0;
        }
        self.round += 1;
    }

    /// Notes that a bubble carrying `flags` passed the node at `index`;
    /// `true` when one carrying all of them passed it earlier in the round.
    pub(crate) fn note(&mut self, index: usize, flags: u8) -> bool {
        if self.open_batches == 0 {
            return false;
        }
        if index >= self.passes.len() {
            self.passes.resize(index + 1, 0);
        }
        let flags = u32::from(flags) & FLAG_MASK;
        let stamp = self.round << FLAG_BITS;
        let pass = &mut self.passes[index];
        if *pass & !FLAG_MASK != stamp {
            *pass = stamp | flags;
            return false;
        }
        if *pass & flags == flags {
            return true;
        }
        *pass |= flags;
        false
    }
}
