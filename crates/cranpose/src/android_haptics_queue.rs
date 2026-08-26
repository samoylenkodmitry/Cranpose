//! Bounded handoff between callers of [`cranpose_services::Haptics`] and the
//! dedicated Android haptics delivery thread.
//!
//! A vibrator call made on the calling thread costs that thread the JNI →
//! binder round trip to `VibratorManagerService`: a watch profile measured the
//! waveform path at 0.88 ms per frame on the render thread (946 calls in 975
//! frames). The queue moves delivery onto the "cranpose-haptics" thread, so
//! callers enqueue and return.
//!
//! The delivery contract, pinned down by the unit test:
//!
//! * Discrete effects (perform / one-shot / predefined / cancel) are never
//!   dropped and arrive in enqueue order; a saturated queue makes the caller
//!   wait for a slot rather than lose one.
//! * Waveforms are continuous state, so under saturation the newest wins: when
//!   the queue is full and its most recent entry is a waveform, the incoming
//!   waveform replaces it. Only the tail is ever replaced — replacing a
//!   waveform buried behind later commands would change which command the
//!   vibrator ends on, because every `Vibrator.vibrate` supersedes the one
//!   before it.
//! * After [`HapticQueue::shut_down`] the commands already accepted still
//!   drain; new ones are handed back for synchronous delivery on the caller.
//!
//! Built on the host as well so the ordering/coalescing test runs everywhere.

use std::{
    collections::VecDeque,
    sync::{
        Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use cranpose_services::{HapticEffect, HapticFeedback};

/// One `Haptics` call, carried from the calling thread to the delivery
/// thread. Waveform payloads are pre-converted to the JNI array element types
/// on the caller, exactly where the synchronous path converted them.
pub(crate) enum HapticCommand {
    /// `Haptics::perform` → `cranposeHaptic`.
    Perform(HapticFeedback),
    /// `Haptics::vibrate` → `cranposeHapticOneShot`.
    OneShot {
        /// Vibration length in milliseconds; never zero (callers filter).
        duration_ms: u32,
        /// 1..=255, or 0 for the device default strength.
        amplitude: u8,
    },
    /// `Haptics::play_pattern` → `cranposeHapticWaveform`.
    Waveform {
        /// Alternating off/on step lengths, `long[]` on the Java side.
        timings_ms: Vec<i64>,
        /// Per-step strengths, `int[]` on the Java side.
        amplitudes: Vec<i32>,
        /// Step index to repeat from, or -1 to play once.
        repeat: i32,
    },
    /// `Haptics::perform_effect` → `cranposeHapticPredefined`.
    Effect(HapticEffect),
    /// `Haptics::cancel` → `cranposeHapticCancel`.
    Cancel,
}

/// Every how many accepted commands the parity counters go to the log. At the
/// profiled rate of about one waveform per frame at 60 fps this is one debug
/// line every ~8.5 seconds — cheap enough to leave on, frequent enough to
/// prove on-watch that enqueued and delivered stay in lockstep.
const PARITY_LOG_EVERY: u64 = 512;

struct State {
    queue: VecDeque<HapticCommand>,
    shut_down: bool,
    /// Calls accepted, including waveforms that later coalesced away.
    enqueued: u64,
    /// Waveforms superseded in the queue by a newer waveform before delivery.
    coalesced: u64,
}

/// The bounded queue. `enqueued == delivered + coalesced + len` at any quiet
/// moment, which is what the parity log line lets an on-watch session check.
pub(crate) struct HapticQueue {
    capacity: usize,
    state: Mutex<State>,
    /// Signalled when a command lands or shutdown begins; the delivery thread
    /// waits here.
    ready: Condvar,
    /// Signalled when the delivery thread frees a slot; saturated callers
    /// wait here.
    space: Condvar,
    /// Commands the delivery thread has forwarded over JNI. An atomic rather
    /// than part of `State` so counting a delivery never contends with the
    /// render thread's enqueue lock.
    delivered: AtomicU64,
}

fn lock(mutex: &Mutex<State>) -> MutexGuard<'_, State> {
    // A poisoning panic cannot leave this simple state inconsistent; haptics
    // stay best-effort rather than vanishing for the rest of the session.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn wait<'a>(condvar: &Condvar, guard: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
    condvar.wait(guard).unwrap_or_else(PoisonError::into_inner)
}

impl HapticQueue {
    pub(crate) fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            state: Mutex::new(State {
                queue: VecDeque::with_capacity(capacity),
                shut_down: false,
                enqueued: 0,
                coalesced: 0,
            }),
            ready: Condvar::new(),
            space: Condvar::new(),
            delivered: AtomicU64::new(0),
        }
    }

    /// Accepts a command for delivery, returning it to the caller when the
    /// queue has shut down (the caller then delivers synchronously).
    ///
    /// Blocks only when the queue is saturated and the command cannot
    /// coalesce — eight undelivered discrete effects, which the profiled
    /// one-command-per-frame rate never approaches.
    pub(crate) fn enqueue(&self, command: HapticCommand) -> Result<(), HapticCommand> {
        let mut state = lock(&self.state);
        loop {
            if state.shut_down {
                return Err(command);
            }
            if state.queue.len() < self.capacity {
                state.queue.push_back(command);
                break;
            }
            if matches!(command, HapticCommand::Waveform { .. }) {
                if let Some(tail @ HapticCommand::Waveform { .. }) = state.queue.back_mut() {
                    *tail = command;
                    state.coalesced += 1;
                    break;
                }
            }
            // A discrete effect must not be dropped and a waveform must not
            // jump past one: wait for the delivery thread to free a slot.
            state = wait(&self.space, state);
        }
        state.enqueued += 1;
        if state.enqueued.is_multiple_of(PARITY_LOG_EVERY) {
            log::debug!(
                "[haptics] enqueued={} delivered={} coalesced={} queued={}",
                state.enqueued,
                self.delivered.load(Ordering::Relaxed),
                state.coalesced,
                state.queue.len(),
            );
        }
        drop(state);
        self.ready.notify_one();
        Ok(())
    }

    /// Next command to deliver, blocking while the queue is empty. `None`
    /// once the queue has shut down and drained: the delivery thread's cue
    /// to exit.
    pub(crate) fn dequeue(&self) -> Option<HapticCommand> {
        let mut state = lock(&self.state);
        loop {
            if let Some(command) = state.queue.pop_front() {
                drop(state);
                self.space.notify_one();
                return Some(command);
            }
            if state.shut_down {
                return None;
            }
            state = wait(&self.ready, state);
        }
    }

    /// Counts a command forwarded over JNI, for the parity log.
    pub(crate) fn note_delivered(&self) {
        self.delivered.fetch_add(1, Ordering::Relaxed);
    }

    /// Stops accepting commands and wakes both sides: the delivery thread
    /// drains what was accepted and exits, and a caller blocked on a full
    /// queue is released with its command handed back. Idempotent.
    pub(crate) fn shut_down(&self) {
        lock(&self.state).shut_down = true;
        self.ready.notify_all();
        self.space.notify_all();
    }

    #[cfg(test)]
    fn stats(&self) -> (u64, u64, u64) {
        let state = lock(&self.state);
        (
            state.enqueued,
            state.coalesced,
            self.delivered.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// The delivery contract in one pass: discrete effects survive saturation
    /// in order, a saturated tail waveform is last-write-wins, shutdown
    /// drains what was accepted and refuses the rest.
    #[test]
    fn ordering_and_coalescing_contract() {
        let queue = Arc::new(HapticQueue::new(4));

        // Fill to capacity: three discrete commands and a waveform tail.
        assert!(
            queue
                .enqueue(HapticCommand::Perform(HapticFeedback::ImpactLight))
                .is_ok()
        );
        assert!(
            queue
                .enqueue(HapticCommand::OneShot {
                    duration_ms: 10,
                    amplitude: 0,
                })
                .is_ok()
        );
        assert!(queue.enqueue(HapticCommand::Cancel).is_ok());
        assert!(
            queue
                .enqueue(HapticCommand::Waveform {
                    timings_ms: vec![1],
                    amplitudes: vec![1],
                    repeat: -1,
                })
                .is_ok()
        );

        // Saturated with a waveform at the tail: newer waveforms coalesce
        // into that slot without blocking; the last one written wins.
        for step in [2i64, 3] {
            assert!(
                queue
                    .enqueue(HapticCommand::Waveform {
                        timings_ms: vec![step],
                        amplitudes: vec![step as i32],
                        repeat: if step == 3 { 0 } else { -1 },
                    })
                    .is_ok()
            );
        }

        // A discrete effect never coalesces and never drops: it waits for the
        // delivery side to free a slot, then lands at the back.
        let enqueuer = std::thread::spawn({
            let queue = Arc::clone(&queue);
            move || {
                assert!(
                    queue
                        .enqueue(HapticCommand::Effect(HapticEffect::Tick))
                        .is_ok()
                );
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(50));

        let mut delivered = Vec::new();
        for _ in 0..5 {
            let command = queue.dequeue().expect("queue is not shut down");
            queue.note_delivered();
            delivered.push(command);
        }
        enqueuer.join().expect("blocked enqueue completes");

        assert!(matches!(
            delivered[0],
            HapticCommand::Perform(HapticFeedback::ImpactLight)
        ));
        assert!(matches!(
            delivered[1],
            HapticCommand::OneShot {
                duration_ms: 10,
                amplitude: 0,
            }
        ));
        assert!(matches!(delivered[2], HapticCommand::Cancel));
        // Of the three waveforms only the newest survived, payload intact.
        match &delivered[3] {
            HapticCommand::Waveform {
                timings_ms,
                amplitudes,
                repeat,
            } => {
                assert_eq!(timings_ms, &[3]);
                assert_eq!(amplitudes, &[3]);
                assert_eq!(*repeat, 0);
            }
            _ => panic!("expected the coalesced waveform"),
        }
        assert!(matches!(
            delivered[4],
            HapticCommand::Effect(HapticEffect::Tick)
        ));

        // Shutdown: what was accepted drains, what comes later is handed
        // back, and the drained queue reports the exit signal.
        assert!(queue.enqueue(HapticCommand::Cancel).is_ok());
        queue.shut_down();
        assert!(matches!(
            queue.enqueue(HapticCommand::Perform(HapticFeedback::Success)),
            Err(HapticCommand::Perform(HapticFeedback::Success))
        ));
        assert!(matches!(queue.dequeue(), Some(HapticCommand::Cancel)));
        queue.note_delivered();
        assert!(queue.dequeue().is_none());

        // Parity: every accepted command was delivered or coalesced.
        let (enqueued, coalesced, delivered) = queue.stats();
        assert_eq!(enqueued, 8);
        assert_eq!(coalesced, 2);
        assert_eq!(delivered, enqueued - coalesced);
    }
}
