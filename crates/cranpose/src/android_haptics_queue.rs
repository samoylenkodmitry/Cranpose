use std::{
    collections::VecDeque,
    sync::{
        Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use cranpose_services::{HapticEffect, HapticFeedback};

pub(crate) enum HapticCommand {
    Perform(HapticFeedback),
    OneShot {
        duration_ms: u32,
        amplitude: u8,
    },
    Waveform {
        timings_ms: Vec<i64>,
        amplitudes: Vec<i32>,
        repeat: i32,
    },
    Effect(HapticEffect),
    Cancel,
}

const PARITY_LOG_EVERY: u64 = 512;

struct State {
    queue: VecDeque<HapticCommand>,
    shut_down: bool,
    enqueued: u64,
    coalesced: u64,
}

pub(crate) struct HapticQueue {
    capacity: usize,
    state: Mutex<State>,
    ready: Condvar,
    space: Condvar,
    delivered: AtomicU64,
}

fn lock(mutex: &Mutex<State>) -> MutexGuard<'_, State> {
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
            if matches!(command, HapticCommand::Waveform { .. })
                && let Some(tail @ HapticCommand::Waveform { .. }) = state.queue.back_mut()
            {
                *tail = command;
                state.coalesced += 1;
                break;
            }
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

    pub(crate) fn note_delivered(&self) {
        self.delivered.fetch_add(1, Ordering::Relaxed);
    }

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
#[path = "tests/android_haptics_queue_tests.rs"]
mod tests;
