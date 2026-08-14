//! Bounded stage executor whose submitter works instead of waiting.
//!
//! The hot collection and replay paths fan pure per-entry maps out across
//! cores several times per frame. The previous `FrameWorkerPool` handled
//! this with hand-rolled lifetime-erased job pointers and a strict
//! one-job-at-a-time invariant, with the calling thread doubling as lane 0.
//! That invariant cannot survive the depth-one frame pipeline: the producer
//! stage (record/verify/lower) and the present stage (batch preparation)
//! will submit fan-outs concurrently from two threads. Its rayon successor
//! fixed concurrency but parked every submitter inside
//! [`rayon::ThreadPool::install`] — on the 4×A53 watch that latch wait was
//! 16.2% of render-thread wall (3.41 ms/frame), while `lanes` pool workers
//! plus the parked-but-spinning caller oversubscribed the cores.
//!
//! This executor keeps the rayon pool but makes the submitting thread one
//! of the lanes again:
//!
//! - The pool is one thread narrower than the lane budget; the submitter is
//!   the missing lane. A fan-out enters through
//!   [`rayon::ThreadPool::in_place_scope`], the caller pulls chunks like any
//!   worker, and a lone submission runs exactly `lanes` runnable threads.
//!   The scope's final wait holds the caller for at most the stragglers'
//!   chunk tails, not the whole fan-out.
//! - Work distributes through a per-submission atomic cursor, not rayon's
//!   parallel iterators: the closure `in_place_scope` runs on the caller
//!   executes outside the pool's registry, so a `par_iter` there would
//!   silently run on the GLOBAL rayon pool (wrong threads, wrong width).
//!   Explicit `scope.spawn` tasks target this pool by construction.
//! - Concurrent submissions stay independent and correct: each owns its
//!   cursor, its telemetry marks and its scope, and they share only the
//!   pool's workers. A contended pair briefly runs `lanes + 1` runnable
//!   threads (both submitters plus the pool) — no worse than what a single
//!   `install` submission cost before, and the steady state of one
//!   submission is now exactly on budget.
//! - Producer submissions split into `PRODUCER_CHUNK_FACTOR` times more
//!   chunks than lanes, bounding the submitter's straggler wait to one
//!   small chunk and letting workers move to a present submission as the
//!   producer's pull loops drain.
//! - Queue delay, execution time and contention counters are recorded per
//!   submission and exposed through [`StageExecutor::telemetry`] so the
//!   pipelined build can tune the lane budget from measurements.
//!
//! The spare-capacity fill behind [`StageExecutor::map_fill`] is this
//! module's only unsafe code, confined to the [`spare_fill`] helper the way
//! `run_entry` confines its `Sync` proof.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;

use web_time::Instant;

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

/// Decrements the active-submission count when dropped, unwind included —
/// a panicking fan-out must not pin the contention counter high forever.
struct ActiveSubmission<'a>(&'a AtomicUsize);

impl Drop for ActiveSubmission<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
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
        // The submitter is one of the lanes: a fan-out runs on `lanes - 1`
        // pool workers plus the submitting thread, so the pool is built one
        // thread narrower than the budget. `lanes == 1` never reaches the
        // pool (every entry point short-circuits to serial), so its single
        // mandatory rayon thread only ever parks.
        let workers = lanes.saturating_sub(1).max(1);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
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

    /// Distributes `units` work items across the submitting thread and the
    /// pool, recording queue delay, execution time and contention. Every
    /// runner — the caller inside the scope closure plus up to `lanes - 1`
    /// spawned pool tasks — pulls the next unit index from a shared cursor
    /// until none remain, so no unit runs twice and none is skipped;
    /// `in_place_scope` then holds the caller only for the last unit each
    /// straggler already pulled.
    ///
    /// Concurrent submissions are independent by construction: each call
    /// owns its cursor, its first-chunk mark and its scope, so two threads
    /// submitting simultaneously distribute their own units correctly and
    /// share only the pool's workers (covered by
    /// `simultaneous_submissions_from_two_threads_stay_correct`).
    fn run_pooled<F>(&self, units: usize, process: F)
    where
        F: Fn(usize) + Sync,
    {
        debug_assert!(self.lanes > 1, "serial short-circuits precede run_pooled");
        if units == 0 {
            return;
        }
        let previously_active = self.active_submissions.fetch_add(1, Ordering::Relaxed);
        let _active = ActiveSubmission(&self.active_submissions);
        self.telemetry.submissions.fetch_add(1, Ordering::Relaxed);
        if previously_active > 0 {
            self.telemetry
                .contended_submissions
                .fetch_add(1, Ordering::Relaxed);
        }
        let submitted = Instant::now();
        let first_chunk_delay_ns = AtomicU64::new(u64::MAX);
        let mark_first_chunk = || {
            // Invoked per unit: the relaxed load keeps the steady state a
            // shared read; only the winning first unit pays the CAS.
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
        let cursor = AtomicUsize::new(0);
        let pull = || loop {
            let unit = cursor.fetch_add(1, Ordering::Relaxed);
            if unit >= units {
                break;
            }
            mark_first_chunk();
            process(unit);
        };
        // The submitter takes one lane; more tasks than `units - 1` could
        // only ever pull an exhausted cursor.
        let helpers = (self.lanes - 1).min(units - 1);
        self.pool.in_place_scope(|scope| {
            for _ in 0..helpers {
                scope.spawn(|_| pull());
            }
            pull();
        });
        let exec = submitted.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        self.telemetry.exec_ns.fetch_add(exec, Ordering::Relaxed);
        let delay = first_chunk_delay_ns.load(Ordering::Relaxed);
        if delay != u64::MAX {
            self.telemetry
                .queue_delay_ns
                .fetch_add(delay, Ordering::Relaxed);
        }
    }

    /// `out` becomes `input.iter().map(f).collect()`, preserving `out`'s
    /// allocation, fanned out across the pool in deterministic order.
    ///
    /// Chunks write straight into `out`'s spare capacity by index — the
    /// pattern the old `FrameWorkerPool::map_fill` established — so the
    /// fan-out pays neither a placeholder fill nor a gather pass. If `f`
    /// panics, the scope propagates it after every task stops and
    /// [`spare_fill::SpareFill`]'s drop reverts the partial fill: `out`
    /// stays at len 0 and every already-produced value drops exactly once.
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
        let fill = spare_fill::SpareFill::new(out, len, self.chunk_len(stage, len));
        self.run_pooled(fill.chunks(), |chunk| {
            fill.fill_chunk(chunk, |index| f(&input[index]));
        });
        fill.commit();
    }
}

/// Command verification borrows the executor at record boundaries. A job is
/// a whole segment's verification — far heavier than a map item — so jobs
/// pull from the submission's cursor one at a time and that is the load
/// balancing; the submitting thread pulls alongside the pool. Verification
/// jobs write into disjoint per-segment result slots, so completion order
/// is free by contract.
impl cranpose_ui_graphics::VerifyExecutor for StageExecutor {
    fn for_each(&self, jobs: usize, run: &(dyn Fn(usize) + Sync)) {
        if self.lanes == 1 || jobs == 0 {
            for job in 0..jobs {
                run(job);
            }
            return;
        }
        self.run_pooled(jobs, run);
    }
}

/// Spare-capacity fill for [`StageExecutor::map_fill`].
///
/// This module is the crate's second exception to `deny(unsafe_code)`,
/// next to `run_entry`, and follows the same rule: one constructor
/// establishes every invariant the unsafe code rests on, and the module
/// stays small enough to audit alongside them. The old
/// `FrameWorkerPool::map_fill` wrote map results into the output vec's
/// spare capacity for the same reason — never pay a placeholder-fill pass
/// the map immediately overwrites — and this helper keeps that pattern,
/// adding the per-chunk accounting the executor's pull-cursor distribution
/// and its unwind path need.
mod spare_fill {
    #![allow(unsafe_code)]

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// An in-progress `map`-collect into a vec's spare capacity.
    ///
    /// [`SpareFill::new`] clears the vec and reserves room; runners fill
    /// disjoint chunks through a shared reference; [`SpareFill::commit`]
    /// publishes the length. Until commit the vec's len stays 0, so no
    /// panic path can expose uninitialized elements through the vec — the
    /// drop impl instead releases exactly the values written so far.
    pub(super) struct SpareFill<'v, O> {
        /// Held for `commit` and to keep the vec exclusively borrowed;
        /// between `new` and `commit` the buffer is touched only via
        /// `base`.
        out: &'v mut Vec<O>,
        /// `out`'s buffer, captured after the reserve (which may move it).
        base: *mut O,
        len: usize,
        chunk_len: usize,
        /// One claim per chunk: the claim makes `fill_chunk` a safe fn by
        /// turning a double-filled chunk into a panic instead of racing
        /// writes. The executor's pull cursor never yields a duplicate;
        /// this enforces that invariant rather than trusting it.
        claimed: Box<[AtomicBool]>,
        /// Per-chunk count of fully initialized slots, bumped only after a
        /// slot's write completes; the unwind path drops exactly this many.
        watermarks: Box<[AtomicUsize]>,
        committed: bool,
    }

    // SAFETY: a shared `SpareFill` exposes only `fill_chunk`, whose writes
    // land in claim-guarded disjoint slot ranges of a buffer this type
    // exclusively borrows; every written `O` later crosses back to the
    // vec-owning thread (commit) or is dropped on it (unwind), so `O: Send`
    // is exactly the bound that transfer needs. `commit` and `drop` take
    // the value or `&mut self`, so they cannot overlap any `fill_chunk`
    // borrow.
    unsafe impl<O: Send> Sync for SpareFill<'_, O> {}

    impl<'v, O> SpareFill<'v, O> {
        /// The only constructor. It leaves `out` cleared (len 0) with
        /// capacity for `len` values, which is what makes every panic path
        /// below sound: the vec never owns a slot until `commit`.
        pub(super) fn new(out: &'v mut Vec<O>, len: usize, chunk_len: usize) -> Self {
            assert!(chunk_len > 0, "chunk_len must be positive");
            out.clear();
            out.reserve(len);
            let base = out.as_mut_ptr();
            let chunks = len.div_ceil(chunk_len);
            Self {
                out,
                base,
                len,
                chunk_len,
                claimed: (0..chunks).map(|_| AtomicBool::new(false)).collect(),
                watermarks: (0..chunks).map(|_| AtomicUsize::new(0)).collect(),
                committed: false,
            }
        }

        /// How many chunks partition `0..len` — the unit count a
        /// distribution must cover exactly once each.
        pub(super) fn chunks(&self) -> usize {
            self.watermarks.len()
        }

        /// Fills chunk `chunk` in index order, writing `produce(index)`
        /// into slot `index` for every index the chunk covers.
        pub(super) fn fill_chunk(&self, chunk: usize, mut produce: impl FnMut(usize) -> O) {
            assert!(
                !self.claimed[chunk].swap(true, Ordering::Relaxed),
                "chunk {chunk} filled twice"
            );
            let start = chunk * self.chunk_len;
            let end = (start + self.chunk_len).min(self.len);
            let watermark = &self.watermarks[chunk];
            for index in start..end {
                let value = produce(index);
                // SAFETY: the claim above makes this call the only writer
                // of slots `start..end`, which lie inside the capacity
                // `new` reserved and beyond the vec's (zero) length —
                // uninitialized memory this type exclusively borrows, so a
                // plain write is correct and drops nothing.
                unsafe { self.base.add(index).write(value) };
                // Bumped after the write on purpose: the watermark counts
                // slots whose values fully exist — what drop may release
                // when `produce` panics on a later index.
                watermark.store(index - start + 1, Ordering::Release);
            }
        }

        /// Publishes the fill: the vec takes on length `len`. Called once
        /// every chunk has run to completion — for the executor that point
        /// is after the scope joined, which is also what synchronizes the
        /// workers' slot writes with this thread.
        pub(super) fn commit(mut self) {
            debug_assert!(
                self.watermarks
                    .iter()
                    .enumerate()
                    .all(|(chunk, watermark)| {
                        let start = chunk * self.chunk_len;
                        let span = (start + self.chunk_len).min(self.len) - start;
                        watermark.load(Ordering::Acquire) == span
                    }),
                "commit before every chunk finished"
            );
            // SAFETY: every chunk ran to completion, so slots `0..len` all
            // hold initialized values inside capacity `new` reserved.
            unsafe { self.out.set_len(self.len) };
            self.committed = true;
        }
    }

    impl<O> Drop for SpareFill<'_, O> {
        fn drop(&mut self) {
            if self.committed {
                return;
            }
            // The unwind path: `commit` never ran, the vec's len is still
            // 0, and the values produced so far would otherwise leak
            // without running their destructors. Release exactly each
            // chunk's initialized prefix. The executor drops this only
            // after its scope joined, so no `fill_chunk` is still running.
            for (chunk, watermark) in self.watermarks.iter().enumerate() {
                let initialized = watermark.load(Ordering::Acquire);
                let start = chunk * self.chunk_len;
                for index in start..start + initialized {
                    // SAFETY: `fill_chunk` fully wrote slots
                    // `start..start + initialized` (the watermark bumps
                    // only after a write), wrote each exactly once (the
                    // claim), and the vec never adopted them (len 0), so
                    // each value drops here exactly once.
                    unsafe { std::ptr::drop_in_place(self.base.add(index)) };
                }
            }
        }
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
    fn small_inputs_stay_serial_but_correct() {
        let executor = StageExecutor::new(4);
        let input: Vec<u32> = (0..10).collect();
        let mut output: Vec<u32> = Vec::new();
        executor.map_fill(Stage::Present, &input, &mut output, |value| value + 7);
        assert_eq!(output, (7..17).collect::<Vec<_>>());
        // Serial short-circuits never touch the pool.
        assert_eq!(executor.telemetry().submissions, 0);
    }

    #[test]
    fn map_fill_matches_serial_and_reuses_capacity() {
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
    fn map_fill_keeps_order_across_odd_chunk_boundaries() {
        // Serial below MIN_POOLED_ITEMS, pooled at and above it; the pooled
        // lens exercise ragged final chunks on both sides of the chunk
        // arithmetic, repeatedly, so a scheduling-dependent misplacement
        // would have many chances to show.
        let executor = StageExecutor::new(4);
        let mut out: Vec<usize> = Vec::new();
        let lens: Vec<usize> = (1..=257)
            .chain([511, 512, 513, 767, 1023, 1024, 1025, 4095, 4097])
            .collect();
        for &len in &lens {
            let input: Vec<usize> = (0..len).collect();
            for _ in 0..8 {
                executor.map_fill(Stage::Producer, &input, &mut out, |v| v * 31 + 7);
                assert_eq!(out.len(), len);
                assert!(
                    out.iter().enumerate().all(|(i, &v)| v == i * 31 + 7),
                    "misplaced result at len {len}"
                );
            }
        }
    }

    #[test]
    fn the_submitting_thread_participates_in_the_fan_out() {
        // The reason this executor exists: the submitter must pull chunks
        // instead of parking for the whole fan-out. A single run could in
        // principle be drained by the workers before the submitter's first
        // pull, so require participation only across a batch of runs.
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..50_000).collect();
        let caller = std::thread::current().id();
        let caller_items = AtomicUsize::new(0);
        let mut out: Vec<u64> = Vec::new();
        for _ in 0..20 {
            executor.map_fill(Stage::Producer, &input, &mut out, |v| {
                if std::thread::current().id() == caller {
                    caller_items.fetch_add(1, Ordering::Relaxed);
                }
                *v
            });
        }
        assert!(
            caller_items.load(Ordering::Relaxed) > 0,
            "the submitting thread never processed an item across 20 fan-outs"
        );
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
    fn verify_jobs_run_exactly_once_at_any_job_count() {
        // Fewer jobs than runners (some spawned helpers find an exhausted
        // cursor immediately) through far more jobs than runners.
        let executor = StageExecutor::new(4);
        for jobs in [0usize, 1, 2, 3, 4, 5, 7, 16, 193, 1024, 5000] {
            let hits: Vec<AtomicUsize> = (0..jobs).map(|_| AtomicUsize::new(0)).collect();
            executor.for_each(jobs, &|job| {
                hits[job].fetch_add(1, Ordering::Relaxed);
            });
            assert!(
                hits.iter().all(|hit| hit.load(Ordering::Relaxed) == 1),
                "job count {jobs}"
            );
        }
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
                let mut out: Vec<u64> = Vec::new();
                for round in 0..100u64 {
                    executor.map_fill(Stage::Present, &input, &mut out, |v| v * 5 + round);
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
    fn concurrent_map_fill_and_verify_jobs_stay_independent() {
        // A map fan-out and a verify fan-out submitted from two OS threads:
        // each submission owns its cursor, so neither can steal or skip the
        // other's units.
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..8_192).collect();
        let hits: Vec<AtomicUsize> = (0..193).map(|_| AtomicUsize::new(0)).collect();
        std::thread::scope(|scope| {
            let mapper = scope.spawn(|| {
                let mut out: Vec<u64> = Vec::new();
                for round in 0..60u64 {
                    executor.map_fill(Stage::Producer, &input, &mut out, |v| v ^ round);
                    assert!(out.iter().enumerate().all(|(i, &v)| v == i as u64 ^ round));
                }
            });
            let verifier = scope.spawn(|| {
                for _ in 0..60 {
                    executor.for_each(hits.len(), &|job| {
                        hits[job].fetch_add(1, Ordering::Relaxed);
                    });
                }
            });
            mapper.join().expect("mapper thread");
            verifier.join().expect("verifier thread");
        });
        assert!(hits.iter().all(|hit| hit.load(Ordering::Relaxed) == 60));
    }

    #[test]
    fn panicking_job_propagates_and_the_executor_survives() {
        let executor = StageExecutor::new(4);
        let input: Vec<u32> = (0..5_000).collect();
        let mut output: Vec<u32> = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.map_fill(Stage::Producer, &input, &mut output, |value| {
                assert!(*value != 2_500, "poisoned item");
                *value
            });
        }));
        assert!(result.is_err(), "the panic must reach the submitter");
        // The pool must keep serving submissions after a job panicked.
        executor.map_fill(Stage::Producer, &input, &mut output, |value| value + 1);
        assert!(output.iter().enumerate().all(|(i, &v)| v == i as u32 + 1));
    }

    #[test]
    fn panic_in_map_leaves_out_empty_and_drops_each_value_once() {
        struct Counted<'a>(&'a AtomicUsize);
        impl Drop for Counted<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let executor = StageExecutor::new(4);
        let input: Vec<u32> = (0..4_096).collect();
        let created = AtomicUsize::new(0);
        let dropped = AtomicUsize::new(0);
        let mut out: Vec<Counted> = Vec::new();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.map_fill(Stage::Producer, &input, &mut out, |value| {
                assert!(*value != 3_000, "poisoned item");
                created.fetch_add(1, Ordering::Relaxed);
                Counted(&dropped)
            });
        }));
        assert!(result.is_err(), "the panic must reach the submitter");
        assert_eq!(
            out.len(),
            0,
            "a failed fill must not expose partial results"
        );
        assert_eq!(
            created.load(Ordering::Relaxed),
            dropped.load(Ordering::Relaxed),
            "every produced value must drop exactly once, no more, no less"
        );
        // The same output vec must be reusable after the failed fill.
        executor.map_fill(Stage::Producer, &input, &mut out, |_| {
            created.fetch_add(1, Ordering::Relaxed);
            Counted(&dropped)
        });
        assert_eq!(out.len(), input.len());
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
        let mut out: Vec<u32> = Vec::new();
        executor.map_fill(Stage::Producer, &outer, &mut out, |v| v * 2);
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
