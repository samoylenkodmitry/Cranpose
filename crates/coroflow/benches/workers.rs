use std::{
    sync::{Arc, Condvar, Mutex, mpsc},
    time::{Duration, Instant},
};

use coroflow::{CoroutineScope, Dispatchers, JobOutcome, yield_now};
use cranpose_core::{Composition, MemoryApplier};

#[path = "support/workers.rs"]
mod support;

fn resources() -> [usize; 3] {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    ["Threads:", "VmRSS:", "VmHWM:"].map(|field| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(field))
            .and_then(|tail| tail.split_whitespace().next())
            .and_then(|number| number.parse().ok())
            .unwrap_or(0)
    })
}

fn compute(mut value: u64) -> u64 {
    for _ in 0..1_000_000 {
        value = std::hint::black_box(value.wrapping_mul(0xd134_2543_de82_ef95));
        value ^= value >> 19;
    }
    value
}

type Gate = Arc<(Mutex<bool>, Condvar)>;

fn work(gate: Gate, value: u64) -> u64 {
    let held = gate.0.lock().expect("gate");
    drop(gate.1.wait_while(held, |go| !*go).expect("gate released"));
    compute(value)
}

struct Measurement {
    checksum: u64,
    jobs: usize,
    admission_ns: u128,
    elapsed_ns: u128,
    max_gap_ns: u128,
    loaded: [usize; 3],
}

fn mixed(busy_main: bool, wakes: bool) -> Measurement {
    let cpu = CoroutineScope::new(Dispatchers::default_pool());
    let io = CoroutineScope::new(Dispatchers::io());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let composition = busy_main.then(|| Composition::new(MemoryApplier::new()));
    let runtime = composition.as_ref().map(Composition::runtime_handle);
    let (completed, receive) = mpsc::channel();
    let mut jobs = Vec::new();
    let started = Instant::now();
    for lane in 0..3 {
        for index in 0..32 {
            let value = (lane * 32 + index) as u64;
            let gate = Arc::clone(&gate);
            let completed = completed.clone();
            let scope = if lane == 1 { &io } else { &cpu };
            jobs.push(scope.launch(async move {
                let result = if wakes {
                    for _ in 0..128 {
                        yield_now().await;
                    }
                    value
                } else if lane == 0 {
                    support::blocking(move || work(gate, value)).await
                } else {
                    work(gate, value)
                };
                completed
                    .send(result)
                    .expect("driver receives every result");
            }));
        }
    }
    let admission_ns = started.elapsed().as_nanos();
    let loaded = resources();
    *gate.0.lock().expect("gate") = true;
    gate.1.notify_all();
    let mut checksum = 0_u64;
    let mut remaining = jobs.len();
    let mut previous = Instant::now();
    let mut max_gap_ns = 0;
    while remaining > 0 {
        let now = Instant::now();
        max_gap_ns = max_gap_ns.max(now.duration_since(previous).as_nanos());
        previous = now;
        let value = if let Some(runtime) = &runtime {
            runtime.drain_ui();
            receive.try_recv().ok()
        } else {
            Some(
                receive
                    .recv_timeout(Duration::from_secs(10))
                    .expect("work progresses"),
            )
        };
        if let Some(value) = value {
            checksum = checksum.wrapping_add(value);
            remaining -= 1;
        }
        assert!(started.elapsed() < Duration::from_secs(30), "work stalled");
    }
    pollster::block_on(async {
        for job in jobs {
            assert_eq!(job.join().await, JobOutcome::Completed);
        }
    });
    Measurement {
        checksum,
        jobs: 96,
        admission_ns,
        elapsed_ns: started.elapsed().as_nanos(),
        max_gap_ns,
        loaded,
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "mixed".into());
    let initial = resources();
    let initialized_at = Instant::now();
    let _dispatchers = [Dispatchers::default_pool(), Dispatchers::io()];
    let initialization_ns = initialized_at.elapsed().as_nanos();
    let initialized = resources();
    let result = if mode == "serial" {
        let started = Instant::now();
        let checksum = support::serial(1_000);
        assert_eq!(checksum, 499_500);
        Measurement {
            checksum,
            jobs: 1_000,
            admission_ns: 0,
            elapsed_ns: started.elapsed().as_nanos(),
            max_gap_ns: 0,
            loaded: resources(),
        }
    } else {
        assert!(["cpu", "mixed", "wakes", "idle"].contains(&mode.as_str()));
        mixed(mode == "mixed", mode == "wakes")
    };
    let finished = resources();
    if mode == "idle" {
        std::thread::sleep(Duration::from_secs(31));
    }
    let retained = resources();
    println!(
        "{{\"mode\":\"{mode}\",\"jobs\":{},\"checksum\":{},\"initialization_ns\":{initialization_ns},\"admission_ns\":{},\"elapsed_ns\":{},\"main_max_gap_ns\":{},\"initial\":{initial:?},\"initialized\":{initialized:?},\"loaded\":{:?},\"finished\":{finished:?},\"retained\":{retained:?}}}",
        result.jobs,
        result.checksum,
        result.admission_ns,
        result.elapsed_ns,
        result.max_gap_ns,
        result.loaded
    );
}
