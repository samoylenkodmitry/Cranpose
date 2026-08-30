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
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn ordering_and_coalescing_contract() {
        let queue = Arc::new(HapticQueue::new(4));

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

        assert!(queue.enqueue(HapticCommand::Cancel).is_ok());
        queue.shut_down();
        assert!(matches!(
            queue.enqueue(HapticCommand::Perform(HapticFeedback::Success)),
            Err(HapticCommand::Perform(HapticFeedback::Success))
        ));
        assert!(matches!(queue.dequeue(), Some(HapticCommand::Cancel)));
        queue.note_delivered();
        assert!(queue.dequeue().is_none());

        let (enqueued, coalesced, delivered) = queue.stats();
        assert_eq!(enqueued, 8);
        assert_eq!(coalesced, 2);
        assert_eq!(delivered, enqueued - coalesced);
    }
}
