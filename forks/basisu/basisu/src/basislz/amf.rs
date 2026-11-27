//! `approx_move_to_front`: the selector-history buffer used by the ETC1S slice
//! decoder. A rover-based ring with an approximate move-to-front (`index/2`
//! swap) on access, so frequently reused selector indices migrate toward the
//! front of the buffer.

use alloc::vec::Vec;

/// Approximate move-to-front buffer of `i32` values.
pub struct ApproxMoveToFront {
    values: Vec<i32>,
    rover: usize,
}

impl ApproxMoveToFront {
    /// Buffer of `n` slots, all zeroed, with the rover seeded at the midpoint
    /// `n / 2` (so the first `add` writes into the second half).
    pub fn new(n: usize) -> Self {
        Self {
            values: vec![0; n],
            rover: n / 2,
        }
    }

    /// Number of slots in the buffer.
    pub fn size(&self) -> usize {
        self.values.len()
    }

    /// Value currently held at `index`.
    pub fn get(&self, index: usize) -> i32 {
        self.values[index]
    }

    /// Write `new_value` at the rover and advance it. On reaching the end the
    /// rover wraps to the midpoint, not to 0: new values only ever land in the
    /// second half, leaving the front half to entries promoted by `use_index`.
    pub fn add(&mut self, new_value: i32) {
        self.values[self.rover] = new_value;
        self.rover += 1;
        if self.rover == self.values.len() {
            self.rover = self.values.len() / 2;
        }
    }

    /// Mark the value at `index` as used: swap it halfway toward the front
    /// (into slot `index/2`). Index 0 stays where it is.
    pub fn use_index(&mut self, index: usize) {
        if index != 0 {
            self.values.swap(index / 2, index);
        }
    }
}
