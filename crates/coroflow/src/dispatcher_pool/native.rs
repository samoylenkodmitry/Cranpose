use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use crate::{Dispatch, Dispatcher, Runnable, SystemClock, sync::lock};

pub(super) fn dispatcher(name: &str, limit: usize, idle: Option<Duration>) -> Dispatcher {
    let pool = Arc::new(Pool {
        name: name.to_owned(),
        limit,
        idle,
        state: Mutex::new(State::default()),
        available: Condvar::new(),
    });
    Dispatcher::new(Executor { pool }, SystemClock::shared())
}

struct Executor {
    pool: Arc<Pool>,
}

impl Dispatch for Executor {
    fn dispatch(&self, runnable: Runnable) {
        self.pool.dispatch(runnable);
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        lock(&self.pool.state).closed = true;
        self.pool.available.notify_all();
    }
}

struct Pool {
    name: String,
    limit: usize,
    idle: Option<Duration>,
    state: Mutex<State>,
    available: Condvar,
}

#[derive(Default)]
struct State {
    queue: VecDeque<Runnable>,
    running: usize,
    workers: usize,
    next_worker: usize,
    closed: bool,
}

impl Pool {
    fn dispatch(self: &Arc<Self>, runnable: Runnable) {
        let mut state = lock(&self.state);
        state.queue.push_back(runnable);
        if state.workers < self.limit && state.workers < state.running + state.queue.len() {
            let worker = Arc::clone(self);
            let name = format!("{}-{}", self.name, state.next_worker);
            match std::thread::Builder::new()
                .name(name)
                .spawn(move || worker.run())
            {
                Ok(_) => {
                    state.workers += 1;
                    state.next_worker = state.next_worker.wrapping_add(1);
                }
                Err(error) => log::error!("coroflow: worker could not start: {error}"),
            }
        }
        let idle = state.workers > state.running;
        drop(state);
        if idle {
            self.available.notify_one();
        }
    }

    fn next(&self, completed: bool) -> Option<Runnable> {
        let mut state = lock(&self.state);
        if completed {
            state.running -= 1;
        }
        let mut idle_since = None;
        loop {
            if let Some(runnable) = state.queue.pop_front() {
                state.running += 1;
                return Some(runnable);
            }
            let remaining = self.idle.map(|idle| {
                idle.saturating_sub(idle_since.get_or_insert_with(Instant::now).elapsed())
            });
            if state.closed || remaining.is_some_and(|remaining| remaining.is_zero()) {
                state.workers -= 1;
                return None;
            }
            state = match remaining {
                Some(remaining) => {
                    self.available
                        .wait_timeout(state, remaining)
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .0
                }
                None => self
                    .available
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            };
        }
    }

    fn run(&self) {
        let mut completed = false;
        while let Some(runnable) = self.next(completed) {
            runnable.run_guarded();
            completed = true;
        }
    }
}
