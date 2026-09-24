//! A bounded lock-free single-producer/single-consumer queue.
//!
//! This is the workspace's only real-time queue. The mixer feeds its audio
//! callback through it and so does [`cranpose_media`](https://docs.rs/cranpose-media),
//! which is why it is exported rather than kept private: a second implementation
//! would be a second place reaching past the borrow checker.
//!
//! It is the whole channel between a producer thread and an audio callback:
//! the engine's UI thread pushes mixer commands, the media player's decode
//! thread pushes samples, and in both cases the callback is the only reader.
//! Both ends are wait-free: [`Producer::push`] and [`Consumer::pop`] are a pair of
//! relaxed loads, one acquire load, one move and one release store. Nothing
//! allocates, nothing locks, and neither side can block the other — which is
//! what lets the audio callback stay inside its real-time budget.
//!
//! Ownership of the two halves enforces the "single" in SPSC: `push` and `pop`
//! take `&mut self`, and [`Producer`]/[`Consumer`] are not [`Clone`], so the
//! type system rules out a second writer or reader.
//!
//! The crate root denies unsafe code and this module opts back in by name; it
//! is the one place in the engine that reaches past the borrow checker.
#![expect(unsafe_code)]

use std::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Ring<T> {
    slots: Box<[UnsafeCell<MaybeUninit<T>>]>,
    mask: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
}

// SAFETY: the ring hands out exactly one Producer and one Consumer, each of
// which requires `&mut self` to touch a slot. The producer only writes slots in
// `[tail, head + capacity)` and publishes them with a release store to `tail`;
// the consumer only reads slots in `[head, tail)` after an acquire load of
// `tail`, and releases them by advancing `head`. The two index ranges are
// disjoint at all times, so no slot is ever aliased. `T: Send` is required
// because values cross the thread boundary.
unsafe impl<T: Send> Send for Ring<T> {}
// SAFETY: see the `Send` invariant above. `&Ring<T>` exposes no way to read or
// write a slot on its own; the halves do, and each lives on one thread.
unsafe impl<T: Send> Sync for Ring<T> {}

impl<T> Drop for Ring<T> {
    fn drop(&mut self) {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let mut index = head;
        while index != tail {
            let slot = &self.slots[index & self.mask];
            // SAFETY: `index` is in `[head, tail)`, the range the producer has
            // published and the consumer has not yet taken, so this slot holds
            // an initialized `T` that nothing else will read.
            unsafe { slot.get().read().assume_init() };
            index = index.wrapping_add(1);
        }
    }
}

/// The writing half. Lives on the thread that enqueues work (the UI thread).
pub struct Producer<T> {
    ring: Arc<Ring<T>>,
}

/// The reading half. Lives on the thread that consumes work (the audio thread).
pub struct Consumer<T> {
    ring: Arc<Ring<T>>,
}

/// Creates a queue holding at least `capacity` elements.
///
/// The real capacity is `capacity` rounded up to a power of two (minimum 2), so
/// the index wrap is a mask rather than a division.
pub fn channel<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
    let capacity = capacity.max(2).next_power_of_two();
    let mut slots = Vec::with_capacity(capacity);
    slots.resize_with(capacity, || UnsafeCell::new(MaybeUninit::uninit()));
    let ring = Arc::new(Ring {
        slots: slots.into_boxed_slice(),
        mask: capacity - 1,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
    });
    (
        Producer {
            ring: Arc::clone(&ring),
        },
        Consumer { ring },
    )
}

impl<T> Producer<T> {
    /// Enqueues `value`, returning it untouched when the queue is full.
    pub fn push(&mut self, value: T) -> Result<(), T> {
        let ring = &*self.ring;
        let tail = ring.tail.load(Ordering::Relaxed);
        let head = ring.head.load(Ordering::Acquire);
        if tail.wrapping_sub(head) == ring.slots.len() {
            return Err(value);
        }
        let slot = &ring.slots[tail & ring.mask];
        // SAFETY: the slot at `tail` is outside `[head, tail)`, so the consumer
        // never touches it, and this `&mut self` method is the only writer.
        // Writing before the release store below is what publishes the value.
        unsafe { slot.get().write(MaybeUninit::new(value)) };
        ring.tail.store(tail.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    /// How many elements the queue holds.
    pub fn capacity(&self) -> usize {
        self.ring.slots.len()
    }
}

impl<T> Consumer<T> {
    /// Dequeues the oldest element, or `None` when the queue is empty.
    pub fn pop(&mut self) -> Option<T> {
        let ring = &*self.ring;
        let head = ring.head.load(Ordering::Relaxed);
        let tail = ring.tail.load(Ordering::Acquire);
        if head == tail {
            return None;
        }
        let slot = &ring.slots[head & ring.mask];
        // SAFETY: `head != tail` means the producer published this slot with a
        // release store that the acquire load above synchronizes with, so the
        // slot holds an initialized `T` that no one else reads.
        let value = unsafe { slot.get().read().assume_init() };
        ring.head.store(head.wrapping_add(1), Ordering::Release);
        Some(value)
    }

    /// Whether the queue currently holds no elements.
    pub fn is_empty(&self) -> bool {
        let ring = &*self.ring;
        ring.head.load(Ordering::Relaxed) == ring.tail.load(Ordering::Acquire)
    }
}

#[cfg(test)]
#[path = "tests/ring_tests.rs"]
mod tests;
