//! Fixed frame worker pool.
//!
//! The hot collection and replay paths fan pure per-entry maps out across
//! cores several times per frame. Scoped threads spawn and join real threads
//! each time, and on a watch-class core `pthread_create` alone costs a few
//! hundred microseconds — multiplied by several fan-outs per frame it ate
//! most of what the parallelism won. This pool creates its workers once and
//! parks them on a condvar between jobs; steady-state dispatch is one lock,
//! one notify, and no allocation.
//!
//! The pool is deliberately tiny: one job at a time, dispatched from the
//! render thread, which always participates as worker 0 — the barrier at the
//! end of [`FrameWorkerPool::run`] is what makes lending stack borrows to the
//! workers sound.
//!
//! This module is the crate's one exception to `deny(unsafe_code)`: lending
//! stack borrows to persistent threads cannot be expressed safely in std
//! (it is the problem rayon exists to solve), and the unsafety is confined
//! to two pointer wrappers whose invariants the completion barrier enforces.
#![allow(unsafe_code)]

use std::sync::{Condvar, Mutex, OnceLock};

/// A pointer to the caller's borrowed job, valid strictly between publish
/// and the completion barrier inside `run`.
#[derive(Clone, Copy)]
struct JobPtr(*const (dyn Fn(usize) + Sync));

// SAFETY: the pointee outlives every dereference — `run` does not return
// (and therefore the borrow does not end) until every worker has finished
// the generation and decremented `remaining`, panics included.
unsafe impl Send for JobPtr {}

struct PoolState {
    job: Option<JobPtr>,
    generation: u64,
    remaining: usize,
    panicked: bool,
}

struct Shared {
    state: Mutex<PoolState>,
    work_ready: Condvar,
    work_done: Condvar,
}

pub(crate) struct FrameWorkerPool {
    shared: std::sync::Arc<Shared>,
    /// Total lanes including the calling thread's lane 0.
    lanes: usize,
}

impl FrameWorkerPool {
    fn new(lanes: usize) -> Self {
        let lanes = lanes.max(1);
        let shared = std::sync::Arc::new(Shared {
            state: Mutex::new(PoolState {
                job: None,
                generation: 0,
                remaining: 0,
                panicked: false,
            }),
            work_ready: Condvar::new(),
            work_done: Condvar::new(),
        });
        for lane in 1..lanes {
            let shared = std::sync::Arc::clone(&shared);
            std::thread::Builder::new()
                .name(format!("cranpose-fanout-{lane}"))
                .spawn(move || worker_loop(&shared, lane))
                .expect("spawn frame worker");
        }
        Self { shared, lanes }
    }

    /// Runs `job(lane)` once per lane, the calling thread taking lane 0.
    /// Returns only after every lane finished — the barrier that lets `job`
    /// borrow from the caller's stack.
    pub(crate) fn run<F: Fn(usize) + Sync>(&self, job: F) {
        if self.lanes == 1 {
            job(0);
            return;
        }
        let wide: &(dyn Fn(usize) + Sync) = &job;
        // SAFETY: erases the borrow's lifetime so the trait-object pointer
        // can cross into the workers; the completion barrier below keeps the
        // pointee alive for every dereference (see JobPtr).
        let wide: &'static (dyn Fn(usize) + Sync) = unsafe { std::mem::transmute(wide) };
        let ptr = JobPtr(wide as *const (dyn Fn(usize) + Sync));
        {
            let mut state = self.shared.state.lock().expect("pool poisoned");
            debug_assert!(state.job.is_none(), "worker pool jobs cannot nest");
            state.job = Some(ptr);
            state.generation = state.generation.wrapping_add(1);
            state.remaining = self.lanes - 1;
            state.panicked = false;
            self.shared.work_ready.notify_all();
        }
        let caller_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(0)));
        let mut state = self.shared.state.lock().expect("pool poisoned");
        while state.remaining > 0 {
            state = self.shared.work_done.wait(state).expect("pool poisoned");
        }
        state.job = None;
        let worker_panicked = state.panicked;
        drop(state);
        if let Err(payload) = caller_result {
            std::panic::resume_unwind(payload);
        }
        if worker_panicked {
            panic!("frame worker panicked during pooled job");
        }
    }

    /// `output[i] = f(&input[i])` fanned out across the lanes in contiguous
    /// chunks. Order is deterministic by construction.
    pub(crate) fn map_into<I, O, F>(&self, input: &[I], output: &mut [O], f: F)
    where
        I: Sync,
        O: Send,
        F: Fn(&I) -> O + Sync,
    {
        self.map_into_min(input, output, MIN_POOLED_ITEMS, f);
    }

    /// [`FrameWorkerPool::map_into`] with an explicit parallelism floor, for
    /// callers whose per-item work is far heavier than the item count
    /// suggests (a replay "item" is a whole segment's verification).
    pub(crate) fn map_into_min<I, O, F>(&self, input: &[I], output: &mut [O], min_items: usize, f: F)
    where
        I: Sync,
        O: Send,
        F: Fn(&I) -> O + Sync,
    {
        assert_eq!(input.len(), output.len());
        let len = input.len();
        if self.lanes == 1 || len < min_items {
            for (slot, item) in output.iter_mut().zip(input) {
                *slot = f(item);
            }
            return;
        }
        let chunk = len.div_ceil(self.lanes);
        let out_base = SendMutPtr(output.as_mut_ptr());
        self.run(|lane| {
            let start = lane * chunk;
            if start >= len {
                return;
            }
            let end = (start + chunk).min(len);
            // SAFETY: lanes own disjoint index ranges of `output`, and the
            // barrier in `run` ends all use before `output`'s borrow does.
            let out =
                unsafe { std::slice::from_raw_parts_mut(out_base.get().add(start), end - start) };
            for (slot, item) in out.iter_mut().zip(&input[start..end]) {
                *slot = f(item);
            }
        });
    }

    /// `out` becomes `input.iter().map(f).collect()`, fanned out like
    /// [`FrameWorkerPool::map_into`] but writing straight into the vec's
    /// spare capacity — a watch-scale run's output buffer never pays a
    /// placeholder-fill pass that the map immediately overwrites.
    pub(crate) fn map_fill<I, O, F>(&self, input: &[I], out: &mut Vec<O>, f: F)
    where
        I: Sync,
        O: Send,
        F: Fn(&I) -> O + Sync,
    {
        use std::mem::MaybeUninit;
        out.clear();
        out.reserve(input.len());
        let spare = &mut out.spare_capacity_mut()[..input.len()];
        self.map_into(input, spare, |item| MaybeUninit::new(f(item)));
        // SAFETY: `map_into` wrote every slot of `spare` — the serial branch
        // zips the full range and the lane chunks partition `0..len` — so the
        // first `input.len()` elements are initialized. If `f` panics this
        // line is never reached: the vec stays empty and already-written
        // slots leak rather than drop uninitialized memory.
        unsafe { out.set_len(input.len()) };
    }
}

/// Command verification borrows the frame pool at record boundaries: jobs
/// stride across lanes (`lane, lane + lanes, ...`) so a frame's segments
/// spread evenly however many there are. Runs strictly during scene build,
/// before any [`FrameWorkerPool::run`] the renderer issues — jobs never
/// nest.
impl cranpose_ui_graphics::VerifyExecutor for FrameWorkerPool {
    fn for_each(&self, jobs: usize, run: &(dyn Fn(usize) + Sync)) {
        self.run(|lane| {
            let mut i = lane;
            while i < jobs {
                run(i);
                i += self.lanes;
            }
        });
    }
}

/// Below this many items the per-item work cannot amortize even a parked
/// wake, and one core does it faster alone.
const MIN_POOLED_ITEMS: usize = 256;

struct SendMutPtr<T>(*mut T);

impl<T> SendMutPtr<T> {
    /// Accessor rather than field access on purpose: closures touching the
    /// raw field directly would disjoint-capture it WITHOUT this wrapper's
    /// Send/Sync, defeating the point.
    fn get(&self) -> *mut T {
        self.0
    }
}
// SAFETY: lanes access disjoint ranges only; see map_into.
unsafe impl<T: Send> Send for SendMutPtr<T> {}
unsafe impl<T: Send> Sync for SendMutPtr<T> {}

fn worker_loop(shared: &Shared, lane: usize) {
    let mut seen_generation = 0u64;
    loop {
        let job = {
            let mut state = shared.state.lock().expect("pool poisoned");
            loop {
                if state.generation != seen_generation {
                    if let Some(job) = state.job {
                        seen_generation = state.generation;
                        break job;
                    }
                }
                state = shared.work_ready.wait(state).expect("pool poisoned");
            }
        };
        // SAFETY: valid until this generation's barrier releases; we
        // decrement `remaining` only after the call returns.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*job.0)(lane) }));
        let mut state = shared.state.lock().expect("pool poisoned");
        if result.is_err() {
            state.panicked = true;
        }
        state.remaining -= 1;
        if state.remaining == 0 {
            shared.work_done.notify_one();
        }
    }
}

/// The process-wide pool, sized once from the conversion worker policy.
/// Workers park between jobs, so scenes that never fan out pay nothing.
pub(crate) fn frame_pool() -> &'static FrameWorkerPool {
    static POOL: OnceLock<FrameWorkerPool> = OnceLock::new();
    POOL.get_or_init(|| FrameWorkerPool::new(crate::render::shape_convert_worker_count().max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_into_matches_serial_and_keeps_order() {
        let pool = FrameWorkerPool::new(4);
        let input: Vec<u64> = (0..10_000).collect();
        let mut output = vec![0u64; input.len()];
        pool.map_into(&input, &mut output, |value| value * 3 + 1);
        assert!(output.iter().enumerate().all(|(i, &v)| v == i as u64 * 3 + 1));
    }

    #[test]
    fn run_reaches_every_lane() {
        let pool = FrameWorkerPool::new(3);
        let hits: Vec<std::sync::atomic::AtomicUsize> =
            (0..3).map(|_| std::sync::atomic::AtomicUsize::new(0)).collect();
        for _ in 0..50 {
            pool.run(|lane| {
                hits[lane].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            });
        }
        assert!(hits
            .iter()
            .all(|hit| hit.load(std::sync::atomic::Ordering::Relaxed) == 50));
    }

    #[test]
    fn small_inputs_stay_serial_but_correct() {
        let pool = FrameWorkerPool::new(4);
        let input: Vec<u32> = (0..10).collect();
        let mut output = vec![0u32; 10];
        pool.map_into(&input, &mut output, |value| value + 7);
        assert_eq!(output, (7..17).collect::<Vec<_>>());
    }

    #[test]
    fn map_fill_matches_map_into_and_reuses_capacity() {
        let pool = FrameWorkerPool::new(4);
        let input: Vec<u64> = (0..10_000).collect();
        let mut out: Vec<String> = Vec::new();
        pool.map_fill(&input, &mut out, |value| format!("{value}"));
        assert_eq!(out.len(), input.len());
        assert!(out.iter().enumerate().all(|(i, v)| *v == format!("{i}")));
        let capacity = out.capacity();
        pool.map_fill(&input[..500], &mut out, |value| format!("{value}"));
        assert_eq!(out.len(), 500);
        assert_eq!(out.capacity(), capacity, "refill must not shed capacity");
        assert!(out.iter().enumerate().all(|(i, v)| *v == format!("{i}")));
    }
}
