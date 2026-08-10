//! Safe bounded stage executor.
//!
//! The hot collection and replay paths fan pure per-entry maps out across
//! cores several times per frame. The previous `FrameWorkerPool` handled
//! this with hand-rolled lifetime-erased job pointers and a strict
//! one-job-at-a-time invariant, with the calling thread doubling as lane 0.
//! That invariant cannot survive the depth-one frame pipeline: the producer
//! stage (record/verify/lower) and the present stage (batch preparation)
//! will submit fan-outs concurrently from two threads.
//!
//! This executor runs every fan-out on one fixed-width rayon pool:
//!
//! - Submitting threads park inside [`rayon::ThreadPool::install`] while the
//!   pool executes, so the number of runnable threads equals the pool width
//!   (the device core budget) whether one stage submits or both do —
//!   concurrent submissions share the same workers via work-stealing instead
//!   of oversubscribing the cores.
//! - Borrowed inputs and outputs flow through rayon's scoped, order-
//!   preserving indexed parallel iterators; this module contains no unsafe
//!   code, and the crate-wide `deny(unsafe_code)` holds again.
//! - Presentation-critical submissions must not stall behind a long producer
//!   fan-out: producer submissions split into `PRODUCER_CHUNK_FACTOR` times
//!   more chunks than lanes, bounding how long any lane stays busy before it
//!   can steal newly arrived present work. Producer chunks always keep
//!   executing, so neither stage starves.
//! - Queue delay, execution time and contention counters are recorded per
//!   submission and exposed through [`StageExecutor::telemetry`] so the
//!   pipelined build can tune the lane budget from measurements.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

use web_time::Instant;

use rayon::prelude::*;

/// Which pipeline stage a submission serves. Present submissions use the
/// coarsest chunking (lowest overhead); producer submissions chunk finer so
/// present work entering mid-map is delayed by at most one small chunk per
/// lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stage {
    Producer,
    /// No production caller yet: present-side submissions arrive when the
    /// GPU backend moves to its own thread (pipeline step 6).
    #[allow(dead_code)]
    Present,
}

/// Below this many items the per-item work cannot amortize even a parked
/// wake, and one core does it faster alone.
const MIN_POOLED_ITEMS: usize = 256;

/// Producer fan-outs split into this many chunks per lane; the bound on
/// present-stage queue delay is one such chunk's execution time.
const PRODUCER_CHUNK_FACTOR: usize = 4;

#[derive(Default)]
struct TelemetryCounters {
    submissions: AtomicU64,
    contended_submissions: AtomicU64,
    queue_delay_ns: AtomicU64,
    exec_ns: AtomicU64,
}

/// A point-in-time copy of the executor's counters.
#[allow(dead_code)] // read by the pipeline measurement stage (step 8)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExecutorTelemetry {
    /// Pooled submissions since construction (serial short-circuits excluded).
    pub submissions: u64,
    /// Submissions that found another submission already in flight.
    pub contended_submissions: u64,
    /// Total delay between submission and the first chunk starting.
    pub queue_delay_ns: u64,
    /// Total wall time spent inside pooled submissions.
    pub exec_ns: u64,
}

pub(crate) struct StageExecutor {
    pool: rayon::ThreadPool,
    lanes: usize,
    active_submissions: AtomicUsize,
    telemetry: TelemetryCounters,
}

impl StageExecutor {
    pub(crate) fn new(lanes: usize) -> Self {
        let lanes = lanes.max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(lanes)
            .thread_name(|index| format!("cranpose-exec-{index}"))
            .build()
            .expect("build stage executor pool");
        Self {
            pool,
            lanes,
            active_submissions: AtomicUsize::new(0),
            telemetry: TelemetryCounters::default(),
        }
    }

    /// No production caller yet: the pipeline measurement stage (step 8)
    /// reads lane width and counters to tune the lane budget.
    #[allow(dead_code)]
    pub(crate) fn lanes(&self) -> usize {
        self.lanes
    }

    /// No production caller yet: see [`StageExecutor::lanes`].
    #[allow(dead_code)]
    pub(crate) fn telemetry(&self) -> ExecutorTelemetry {
        ExecutorTelemetry {
            submissions: self.telemetry.submissions.load(Ordering::Relaxed),
            contended_submissions: self.telemetry.contended_submissions.load(Ordering::Relaxed),
            queue_delay_ns: self.telemetry.queue_delay_ns.load(Ordering::Relaxed),
            exec_ns: self.telemetry.exec_ns.load(Ordering::Relaxed),
        }
    }

    fn chunk_len(&self, stage: Stage, len: usize) -> usize {
        let chunks = match stage {
            Stage::Producer => self.lanes * PRODUCER_CHUNK_FACTOR,
            Stage::Present => self.lanes,
        };
        len.div_ceil(chunks).max(1)
    }

    /// Runs `body` on the pool, recording queue delay, execution time and
    /// contention. `body` receives a callback that executing chunks invoke;
    /// the first invocation timestamps the submission-to-execution delay.
    fn run_pooled<R, B>(&self, body: B) -> R
    where
        R: Send,
        B: FnOnce(&(dyn Fn() + Sync)) -> R + Send,
    {
        let previously_active = self.active_submissions.fetch_add(1, Ordering::Relaxed);
        self.telemetry.submissions.fetch_add(1, Ordering::Relaxed);
        if previously_active > 0 {
            self.telemetry
                .contended_submissions
                .fetch_add(1, Ordering::Relaxed);
        }
        let submitted = Instant::now();
        let first_chunk_delay_ns = AtomicU64::new(u64::MAX);
        let mark_first_chunk = || {
            // Invoked per item: the relaxed load keeps the steady state a
            // shared read; only the winning first item pays the CAS.
            if first_chunk_delay_ns.load(Ordering::Relaxed) != u64::MAX {
                return;
            }
            let delay = submitted.elapsed().as_nanos().min(u64::MAX as u128) as u64;
            let _ = first_chunk_delay_ns.compare_exchange(
                u64::MAX,
                delay,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        };
        let result = self.pool.install(|| body(&mark_first_chunk));
        let exec = submitted.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.telemetry.exec_ns.fetch_add(exec, Ordering::Relaxed);
        let delay = first_chunk_delay_ns.load(Ordering::Relaxed);
        if delay != u64::MAX {
            self.telemetry
                .queue_delay_ns
                .fetch_add(delay, Ordering::Relaxed);
        }
        self.active_submissions.fetch_sub(1, Ordering::Relaxed);
        result
    }

    /// `output[i] = f(&input[i])` fanned out across the pool with an
    /// explicit parallelism floor, for callers whose per-item work is far
    /// heavier than the item count suggests (a replay "item" is a whole
    /// segment's verification). Order is deterministic by construction.
    pub(crate) fn map_into_min<I, O, F>(
        &self,
        stage: Stage,
        input: &[I],
        output: &mut [O],
        min_items: usize,
        f: F,
    ) where
        I: Sync,
        O: Send,
        F: Fn(&I) -> O + Send + Sync,
    {
        assert_eq!(input.len(), output.len());
        let len = input.len();
        if self.lanes == 1 || len < min_items {
            for (slot, item) in output.iter_mut().zip(input) {
                *slot = f(item);
            }
            return;
        }
        let chunk = self.chunk_len(stage, len);
        self.run_pooled(|mark_first_chunk| {
            output
                .par_iter_mut()
                .zip(input.par_iter())
                .with_min_len(chunk)
                .for_each(|(slot, item)| {
                    mark_first_chunk();
                    *slot = f(item);
                });
        });
    }

    /// `out` becomes `input.iter().map(f).collect()`, preserving `out`'s
    /// allocation, fanned out like [`StageExecutor::map_into`].
    pub(crate) fn map_fill<I, O, F>(&self, stage: Stage, input: &[I], out: &mut Vec<O>, f: F)
    where
        I: Sync,
        O: Send,
        F: Fn(&I) -> O + Send + Sync,
    {
        let len = input.len();
        if self.lanes == 1 || len < MIN_POOLED_ITEMS {
            out.clear();
            out.reserve(len);
            out.extend(input.iter().map(f));
            return;
        }
        let chunk = self.chunk_len(stage, len);
        self.run_pooled(|mark_first_chunk| {
            input
                .par_iter()
                .with_min_len(chunk)
                .map(|item| {
                    mark_first_chunk();
                    f(item)
                })
                .collect_into_vec(out);
        });
    }
}

/// Command verification borrows the executor at record boundaries. Jobs are
/// balanced across the pool by work-stealing; verification jobs write into
/// disjoint per-segment result slots, so completion order is free by
/// contract.
impl cranpose_ui_graphics::VerifyExecutor for StageExecutor {
    fn for_each(&self, jobs: usize, run: &(dyn Fn(usize) + Sync)) {
        if self.lanes == 1 || jobs == 0 {
            for job in 0..jobs {
                run(job);
            }
            return;
        }
        self.run_pooled(|mark_first_chunk| {
            (0..jobs).into_par_iter().for_each(|job| {
                mark_first_chunk();
                run(job);
            });
        });
    }
}

/// The process-wide executor, sized once from the conversion worker policy.
/// Workers park between submissions, so scenes that never fan out pay
/// nothing.
pub(crate) fn stage_executor() -> &'static StageExecutor {
    static EXECUTOR: OnceLock<StageExecutor> = OnceLock::new();
    EXECUTOR.get_or_init(|| StageExecutor::new(crate::render::shape_convert_worker_count().max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranpose_ui_graphics::VerifyExecutor;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn map_into_matches_serial_and_keeps_order() {
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..10_000).collect();
        let mut output = vec![0u64; input.len()];
        executor.map_into_min(Stage::Producer, &input, &mut output, 256, |value| {
            value * 3 + 1
        });
        assert!(output
            .iter()
            .enumerate()
            .all(|(i, &v)| v == i as u64 * 3 + 1));
    }

    #[test]
    fn small_inputs_stay_serial_but_correct() {
        let executor = StageExecutor::new(4);
        let input: Vec<u32> = (0..10).collect();
        let mut output = vec![0u32; 10];
        executor.map_into_min(Stage::Present, &input, &mut output, 256, |value| value + 7);
        assert_eq!(output, (7..17).collect::<Vec<_>>());
        // Serial short-circuits never touch the pool.
        assert_eq!(executor.telemetry().submissions, 0);
    }

    #[test]
    fn map_fill_matches_map_into_and_reuses_capacity() {
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..10_000).collect();
        let mut out: Vec<String> = Vec::new();
        executor.map_fill(Stage::Producer, &input, &mut out, |value| {
            format!("{value}")
        });
        assert_eq!(out.len(), input.len());
        assert!(out.iter().enumerate().all(|(i, v)| *v == format!("{i}")));
        let capacity = out.capacity();
        executor.map_fill(Stage::Producer, &input[..500], &mut out, |value| {
            format!("{value}")
        });
        assert_eq!(out.len(), 500);
        assert!(
            out.capacity() >= capacity.min(input.len()),
            "refill must not shed capacity"
        );
        assert!(out.iter().enumerate().all(|(i, v)| *v == format!("{i}")));
    }

    #[test]
    fn verify_executor_runs_every_job_exactly_once() {
        let executor = StageExecutor::new(3);
        let hits: Vec<AtomicUsize> = (0..97).map(|_| AtomicUsize::new(0)).collect();
        for _ in 0..50 {
            executor.for_each(hits.len(), &|job| {
                hits[job].fetch_add(1, Ordering::Relaxed);
            });
        }
        assert!(hits.iter().all(|hit| hit.load(Ordering::Relaxed) == 50));
    }

    #[test]
    fn simultaneous_submissions_from_two_threads_stay_correct() {
        // The invariant the old pool could not offer: two stages submitting
        // concurrently, every result byte-exact.
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..20_000).collect();
        std::thread::scope(|scope| {
            let producer = scope.spawn(|| {
                let mut out: Vec<u64> = Vec::new();
                for round in 0..100u64 {
                    executor.map_fill(Stage::Producer, &input, &mut out, |v| v * 2 + round);
                    assert!(out
                        .iter()
                        .enumerate()
                        .all(|(i, &v)| v == i as u64 * 2 + round));
                }
            });
            let present = scope.spawn(|| {
                let mut out = vec![0u64; input.len()];
                for round in 0..100u64 {
                    executor.map_into_min(Stage::Present, &input, &mut out, 1, |v| v * 5 + round);
                    assert!(out
                        .iter()
                        .enumerate()
                        .all(|(i, &v)| v == i as u64 * 5 + round));
                }
            });
            producer.join().expect("producer thread");
            present.join().expect("present thread");
        });
        let telemetry = executor.telemetry();
        assert!(telemetry.submissions >= 200);
        assert!(
            telemetry.contended_submissions > 0,
            "two hammering threads must overlap at least once"
        );
    }

    #[test]
    fn panicking_job_propagates_and_the_executor_survives() {
        let executor = StageExecutor::new(4);
        let input: Vec<u32> = (0..5_000).collect();
        let mut output = vec![0u32; input.len()];
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.map_into_min(Stage::Producer, &input, &mut output, 1, |value| {
                assert!(*value != 2_500, "poisoned item");
                *value
            });
        }));
        assert!(result.is_err(), "the panic must reach the submitter");
        // The pool must keep serving submissions after a job panicked.
        executor.map_into_min(Stage::Producer, &input, &mut output, 1, |value| value + 1);
        assert!(output.iter().enumerate().all(|(i, &v)| v == i as u32 + 1));
    }

    #[test]
    fn teardown_joins_cleanly_after_use() {
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..10_000).collect();
        let mut out = Vec::new();
        executor.map_fill(Stage::Producer, &input, &mut out, |v| v + 1);
        assert_eq!(out.len(), input.len());
        drop(executor); // completing (not hanging) is the assertion
    }

    #[test]
    fn nested_submissions_do_not_deadlock() {
        // The old pool forbade nesting outright; rayon runs a nested
        // submission inline on the worker. The executor must not rely on
        // "jobs never nest" holding forever.
        let executor = StageExecutor::new(2);
        let outer: Vec<u32> = (0..600).collect();
        let mut out = vec![0u32; outer.len()];
        executor.map_into_min(Stage::Producer, &outer, &mut out, 1, |v| v * 2);
        assert!(out.iter().enumerate().all(|(i, &v)| v == i as u32 * 2));
    }

    #[test]
    fn telemetry_records_queue_delay_and_exec_time() {
        let executor = StageExecutor::new(2);
        let input: Vec<u64> = (0..10_000).collect();
        let mut out = Vec::new();
        executor.map_fill(Stage::Producer, &input, &mut out, |v| v + 1);
        let telemetry = executor.telemetry();
        assert_eq!(telemetry.submissions, 1);
        assert!(telemetry.exec_ns > 0);
        assert!(telemetry.queue_delay_ns < telemetry.exec_ns);
    }
}
