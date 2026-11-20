use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[test]
fn a_finished_write_reads_back() {
    let cell = SeqCell::new([1, 2, 3]);
    assert_eq!(cell.read(), Some([1, 2, 3]));
    assert!(cell.write([4, 5, 6]));
    assert_eq!(cell.read(), Some([4, 5, 6]));
}

#[test]
fn a_write_that_overlaps_another_loses_and_changes_nothing() {
    let cell = SeqCell::new([7, 7]);
    // Starts a write by hand and leaves it unfinished, as a writer on another thread would
    cell.sequence.store(1, Ordering::Relaxed);
    assert!(!cell.write([8, 8]));
    assert_eq!(cell.read(), None, "a read during a write is refused");
    cell.sequence.store(2, Ordering::Release);
    assert_eq!(cell.read(), Some([7, 7]));
}

// A reader on another thread sees the words of one write or the words of the one before, and
// never a mix of two: each write puts one value in every word.
#[test]
fn a_reader_never_sees_a_mix_of_two_writes() {
    let cell = SeqCell::new([0; 8]);
    let done = AtomicBool::new(false);
    let mut seen = 0_u32;
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for value in 1..=200_000_u32 {
                while !cell.write([value; 8]) {}
            }
            done.store(true, Ordering::Release);
        });
        while !done.load(Ordering::Acquire) {
            if let Some(words) = cell.read() {
                assert!(
                    words.iter().all(|word| *word == words[0]),
                    "a torn read gave {words:?}"
                );
                seen += 1;
            }
        }
    });
    assert!(seen > 0, "the reader never got a clean read");
    assert_eq!(cell.read(), Some([200_000; 8]));
}
