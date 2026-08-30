use std::sync::{
    OnceLock,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};

use web_time::Instant;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Stage {
    Producer,
    #[allow(dead_code)]
    Present,
}

const MIN_POOLED_ITEMS: usize = 256;

const PRODUCER_CHUNK_FACTOR: usize = 4;

#[derive(Default)]
struct TelemetryCounters {
    submissions: AtomicU64,
    contended_submissions: AtomicU64,
    queue_delay_ns: AtomicU64,
    exec_ns: AtomicU64,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExecutorTelemetry {
    pub submissions: u64,
    pub contended_submissions: u64,
    pub queue_delay_ns: u64,
    pub exec_ns: u64,
}

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

    #[allow(dead_code)]
    pub(crate) fn lanes(&self) -> usize {
        self.lanes
    }

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

mod spare_fill {
    #![allow(unsafe_code)]

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    pub(super) struct SpareFill<'v, O> {
        out: &'v mut Vec<O>,
        base: *mut O,
        len: usize,
        chunk_len: usize,
        claimed: Box<[AtomicBool]>,
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

        pub(super) fn chunks(&self) -> usize {
            self.watermarks.len()
        }

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
                watermark.store(index - start + 1, Ordering::Release);
            }
        }

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

pub(crate) fn stage_executor() -> &'static StageExecutor {
    static EXECUTOR: OnceLock<StageExecutor> = OnceLock::new();
    EXECUTOR.get_or_init(|| StageExecutor::new(crate::render::shape_convert_worker_count().max(1)))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use cranpose_ui_graphics::VerifyExecutor;

    use super::*;

    #[test]
    fn small_inputs_stay_serial_but_correct() {
        let executor = StageExecutor::new(4);
        let input: Vec<u32> = (0..10).collect();
        let mut output: Vec<u32> = Vec::new();
        executor.map_fill(Stage::Present, &input, &mut output, |value| value + 7);
        assert_eq!(output, (7..17).collect::<Vec<_>>());
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
        let executor = StageExecutor::new(4);
        let input: Vec<u64> = (0..20_000).collect();
        std::thread::scope(|scope| {
            let producer = scope.spawn(|| {
                let mut out: Vec<u64> = Vec::new();
                for round in 0..100u64 {
                    executor.map_fill(Stage::Producer, &input, &mut out, |v| v * 2 + round);
                    assert!(
                        out.iter()
                            .enumerate()
                            .all(|(i, &v)| v == i as u64 * 2 + round)
                    );
                }
            });
            let present = scope.spawn(|| {
                let mut out: Vec<u64> = Vec::new();
                for round in 0..100u64 {
                    executor.map_fill(Stage::Present, &input, &mut out, |v| v * 5 + round);
                    assert!(
                        out.iter()
                            .enumerate()
                            .all(|(i, &v)| v == i as u64 * 5 + round)
                    );
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
        drop(executor);
    }

    #[test]
    fn nested_submissions_do_not_deadlock() {
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
