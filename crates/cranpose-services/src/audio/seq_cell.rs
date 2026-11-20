//! A cell of words that one thread writes and another reads without waiting.
//!
//! The audio callback reads the pose of the head and the place of each source from cells like
//! this one. It takes no lock and never waits for the frame loop that writes them. A write that
//! overlaps a read makes the read fail. The reader then keeps what it read before.

use std::sync::atomic::{AtomicU32, Ordering, fence};

pub(crate) struct SeqCell<const N: usize> {
    // Odd while a write is in progress. Otherwise it is twice the number of writes done.
    sequence: AtomicU32,
    words: [AtomicU32; N],
}

impl<const N: usize> SeqCell<N> {
    pub(crate) fn new(words: [u32; N]) -> Self {
        Self {
            sequence: AtomicU32::new(0),
            words: words.map(AtomicU32::new),
        }
    }

    // Stores `words`. Returns `false` and stores nothing while another write is in progress. A
    // cell has one writer at a time. A second writer only loses its own write.
    pub(crate) fn write(&self, words: [u32; N]) -> bool {
        let before = self.sequence.load(Ordering::Relaxed);
        if before & 1 == 1
            || self
                .sequence
                .compare_exchange(
                    before,
                    before.wrapping_add(1),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_err()
        {
            return false;
        }
        // The odd sequence reaches a reader before any word of this write does
        fence(Ordering::Release);
        for (cell, word) in self.words.iter().zip(words) {
            cell.store(word, Ordering::Relaxed);
        }
        self.sequence
            .store(before.wrapping_add(2), Ordering::Release);
        true
    }

    // The words of the last finished write, or `None` while a write overlaps the read.
    pub(crate) fn read(&self) -> Option<[u32; N]> {
        let before = self.sequence.load(Ordering::Acquire);
        if before & 1 == 1 {
            return None;
        }
        let words = std::array::from_fn(|index| self.words[index].load(Ordering::Relaxed));
        // The words are read before the sequence is read again
        fence(Ordering::Acquire);
        (self.sequence.load(Ordering::Relaxed) == before).then_some(words)
    }
}

#[cfg(test)]
#[path = "tests/seq_cell_tests.rs"]
mod tests;
