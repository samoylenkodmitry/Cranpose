use std::{
    collections::VecDeque,
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use crate::{Dispatch, Dispatcher, Runnable, SystemClock, sync::lock};

pub(super) struct Owner {
    pool: Arc<Pool>,
}

impl Owner {
    pub(super) fn new(name: &str, limits: [usize; 2], idle: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            pool: Arc::new(Pool {
                name: name.to_owned(),
                limits,
                idle,
                state: Mutex::new(State::default()),
                available: Condvar::new(),
            }),
        })
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        lock(&self.pool.state).closed = true;
        self.pool.available.notify_all();
    }
}

pub(super) fn dispatcher(owner: Arc<Owner>, lane: usize) -> Dispatcher {
    Dispatcher::new(Executor { owner, lane }, SystemClock::shared())
}

struct Executor {
    owner: Arc<Owner>,
    lane: usize,
}

impl Dispatch for Executor {
    fn dispatch(&self, runnable: Runnable) {
        self.owner.pool.dispatch(self.lane, runnable);
    }
}

struct Pool {
    name: String,
    limits: [usize; 2],
    idle: Option<Duration>,
    state: Mutex<State>,
    available: Condvar,
}

#[derive(Default)]
struct State {
    queues: [VecDeque<Runnable>; 2],
    running: [usize; 2],
    workers: usize,
    next_worker: usize,
    next_lane: usize,
    closed: bool,
}

impl Pool {
    fn dispatch(self: &Arc<Self>, lane: usize, runnable: Runnable) {
        let mut state = lock(&self.state);
        state.queues[lane].push_back(runnable);
        let demand: usize = (0..2)
            .map(|lane| {
                state.running[lane]
                    + state.queues[lane]
                        .len()
                        .min(self.limits[lane] - state.running[lane])
            })
            .sum();
        if state.workers < demand {
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
        drop(state);
        self.available.notify_one();
    }

    fn next(&self) -> Option<(usize, Runnable)> {
        let mut state = lock(&self.state);
        let idle_since = Instant::now();
        loop {
            for offset in 0..2 {
                let lane = (state.next_lane + offset) % 2;
                if state.running[lane] < self.limits[lane]
                    && let Some(runnable) = state.queues[lane].pop_front()
                {
                    state.running[lane] += 1;
                    state.next_lane = 1 - lane;
                    return Some((lane, runnable));
                }
            }
            if state.closed || self.idle.is_some_and(|idle| idle_since.elapsed() >= idle) {
                state.workers -= 1;
                return None;
            }
            state = match self.idle {
                Some(idle) => {
                    self.available
                        .wait_timeout(state, idle.saturating_sub(idle_since.elapsed()))
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
        while let Some((lane, runnable)) = self.next() {
            runnable.run_guarded();
            lock(&self.state).running[lane] -= 1;
            self.available.notify_one();
        }
    }
}
