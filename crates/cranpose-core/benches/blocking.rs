use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

use cranpose_core::{Composition, MemoryApplier};

#[path = "support/blocking.rs"]
mod support;
use support::Failure;

type Gate = Arc<(Mutex<bool>, Condvar)>;

fn wait(gate: &Gate) {
    let (lock, wake) = &**gate;
    let guard = lock.lock().expect("gate");
    drop(
        wake.wait_while(guard, |released| !*released)
            .expect("gate wait"),
    );
}

fn release(gate: &Gate) {
    *gate.0.lock().expect("gate") = true;
    gate.1.notify_all();
}

struct Ready {
    runnable: AtomicBool,
    waiter: std::thread::Thread,
}
impl Wake for Ready {
    fn wake(self: Arc<Self>) {
        self.runnable.store(true, Ordering::Release);
        self.waiter.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.runnable.store(true, Ordering::Release);
        self.waiter.unpark();
    }
}

struct Slot {
    future: Pin<Box<dyn Future<Output = Result<u64, Failure>>>>,
    ready: Arc<Ready>,
}
impl Slot {
    fn new(work: impl FnOnce() -> u64 + Send + 'static) -> Self {
        Self {
            future: Box::pin(support::run(work)),
            ready: Arc::new(Ready {
                runnable: AtomicBool::new(true),
                waiter: std::thread::current(),
            }),
        }
    }
    fn poll(&mut self) -> Option<Result<u64, Failure>> {
        if !self.ready.runnable.swap(false, Ordering::AcqRel) {
            return None;
        }
        let waker = Waker::from(Arc::clone(&self.ready));
        match self.future.as_mut().poll(&mut Context::from_waker(&waker)) {
            Poll::Ready(value) => Some(value),
            Poll::Pending => None,
        }
    }
}

fn process() -> String {
    let text = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or("null")
    };
    format!(
        "{{\"rss_kib\":{},\"peak_rss_kib\":{},\"threads\":{}}}",
        value("VmRSS:"),
        value("VmHWM:"),
        value("Threads:")
    )
}

fn compute(mut value: u64) -> u64 {
    for _ in 0..1_000_000 {
        value = std::hint::black_box(
            value
                .wrapping_mul(6_364_136_223_846_793_005)
                .rotate_left(13)
                ^ 1_442_695_040_888_963_407,
        );
    }
    value
}

#[derive(Default)]
struct Jobs {
    slots: Vec<Slot>,
    next: usize,
    completed: usize,
    rejected: usize,
    checksum: u64,
}
impl Jobs {
    fn fill(&mut self, total: usize, gate: &Gate) {
        while self.next < total {
            let value = self.next as u64;
            let gate = Arc::clone(gate);
            let mut slot = Slot::new(move || {
                wait(&gate);
                compute(value)
            });
            match slot.poll() {
                Some(Err(Failure::Saturated)) => {
                    self.rejected += 1;
                    return;
                }
                Some(Err(Failure::Other)) => panic!("worker failed"),
                Some(Ok(value)) => self.complete(value),
                None => self.slots.push(slot),
            }
            self.next += 1;
        }
    }
    fn complete(&mut self, value: u64) {
        self.completed += 1;
        self.checksum = self.checksum.wrapping_add(value);
    }
    fn collect(&mut self) -> bool {
        let mut changed = false;
        for index in (0..self.slots.len()).rev() {
            if let Some(result) = self.slots[index].poll() {
                self.complete(result.expect("admitted work succeeds"));
                self.slots.swap_remove(index);
                changed = true;
            }
        }
        changed
    }
}

const GAP_BUCKETS_US: [u128; 12] = [
    1,
    4,
    16,
    64,
    100,
    250,
    500,
    1_000,
    4_000,
    16_000,
    64_000,
    u128::MAX,
];
struct Gaps {
    counts: [u64; 12],
    maximum_ns: u128,
}
impl Gaps {
    fn record(&mut self, elapsed: Duration) {
        let index = GAP_BUCKETS_US
            .iter()
            .position(|bound| elapsed.as_micros() <= *bound)
            .expect("last bucket");
        self.counts[index] += 1;
        self.maximum_ns = self.maximum_ns.max(elapsed.as_nanos());
    }
    fn percentile(&self) -> u128 {
        let target = self
            .counts
            .iter()
            .sum::<u64>()
            .saturating_mul(99)
            .div_ceil(100);
        let mut count = 0;
        for (index, amount) in self.counts.iter().enumerate() {
            count += amount;
            if count >= target {
                return GAP_BUCKETS_US[index];
            }
        }
        self.maximum_ns.div_ceil(1_000)
    }
}

fn cpu(busy_ui: bool) {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let mut jobs = Jobs::default();
    let composition = busy_ui.then(|| Composition::new(MemoryApplier::new()));
    let runtime = composition.as_ref().map(Composition::runtime_handle);
    let mut gaps = Gaps {
        counts: [0; 12],
        maximum_ns: 0,
    };
    let started = Instant::now();
    jobs.fill(96, &gate);
    let admission_ns = started.elapsed().as_nanos();
    let submitted = jobs.next;
    let loaded = process();
    release(&gate);
    let mut previous = Instant::now();
    while jobs.completed < 96 {
        let now = Instant::now();
        gaps.record(now.duration_since(previous));
        previous = now;
        if let Some(runtime) = &runtime {
            runtime.drain_ui();
        }
        if jobs.collect() {
            jobs.fill(96, &gate);
        } else if !busy_ui {
            std::thread::park_timeout(Duration::from_secs(60).saturating_sub(started.elapsed()));
        }
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "CPU jobs stalled"
        );
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let phase = if busy_ui { "contended" } else { "cpu" };
    let p99 = if busy_ui {
        gaps.percentile().to_string()
    } else {
        "null".into()
    };
    let maximum = if busy_ui {
        gaps.maximum_ns.to_string()
    } else {
        "null".into()
    };
    println!(
        "{{\"phase\":\"{phase}\",\"jobs\":{},\"checksum\":{},\"elapsed_ns\":{elapsed_ns},\"initial_admission_ns\":{admission_ns},\"initial_submitted\":{submitted},\"rejected_attempts\":{},\"main_p99_gap_us_upper\":{p99},\"main_max_gap_ns\":{maximum},\"loaded\":{loaded},\"finished\":{}}}",
        jobs.completed,
        jobs.checksum,
        jobs.rejected,
        process()
    );
}

struct Payload {
    bytes: Vec<u8>,
    live: Arc<AtomicUsize>,
}
impl Payload {
    fn new(live: &Arc<AtomicUsize>) -> Self {
        let bytes = vec![17; 128 * 1024];
        live.fetch_add(bytes.len(), Ordering::SeqCst);
        Self {
            bytes,
            live: Arc::clone(live),
        }
    }
}
impl Drop for Payload {
    fn drop(&mut self) {
        self.live.fetch_sub(self.bytes.len(), Ordering::SeqCst);
    }
}

fn cancel() {
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let live = Arc::new(AtomicUsize::new(0));
    let executed = Arc::new(AtomicUsize::new(0));
    let mut slots = Vec::new();
    let mut rejected = 0;
    for _ in 0..128 {
        let payload = Payload::new(&live);
        let gate = Arc::clone(&gate);
        let executed = Arc::clone(&executed);
        let mut slot = Slot::new(move || {
            wait(&gate);
            executed.fetch_add(1, Ordering::SeqCst);
            std::hint::black_box(payload.bytes.len() as u64)
        });
        match slot.poll() {
            None => slots.push(slot),
            Some(Err(Failure::Saturated)) => rejected += 1,
            Some(_) => panic!("a gated job cannot finish"),
        }
    }
    let pending = slots.len();
    let before_cancel = live.load(Ordering::SeqCst);
    let loaded = process();
    let started = Instant::now();
    drop(slots);
    let cancellation_ns = started.elapsed().as_nanos();
    let after_cancel = live.load(Ordering::SeqCst);
    release(&gate);
    while live.load(Ordering::SeqCst) != 0 {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "cancelled payloads retained"
        );
        std::thread::yield_now();
    }
    println!(
        "{{\"phase\":\"cancel\",\"submitted\":128,\"pending_futures\":{pending},\"rejected\":{rejected},\"executed\":{},\"payload_before_cancel\":{before_cancel},\"payload_after_cancel\":{after_cancel},\"cancellation_ns\":{cancellation_ns},\"loaded\":{loaded},\"finished\":{}}}",
        executed.load(Ordering::SeqCst),
        process()
    );
}

fn main() {
    let mode = std::env::args()
        .skip(1)
        .find(|argument| !argument.starts_with('-'))
        .unwrap_or_else(|| "serial".into());
    match mode.as_str() {
        "cpu" => cpu(false),
        "contended" => cpu(true),
        "cancel" => cancel(),
        "serial" => {
            support::serial(1);
            let started = Instant::now();
            let checksum = support::serial(1_000);
            let elapsed_ns = started.elapsed().as_nanos();
            println!(
                "{{\"phase\":\"serial\",\"jobs\":1000,\"checksum\":{checksum},\"elapsed_ns\":{elapsed_ns},\"finished\":{}}}",
                process()
            );
        }
        _ => panic!("expected serial, cpu, contended or cancel"),
    }
}
