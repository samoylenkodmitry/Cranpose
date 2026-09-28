use std::sync::mpsc;

use web_time::{Duration, Instant};

use super::{CompileLane, PipelineCompiler};

#[test]
fn each_lane_runs_its_jobs_in_queue_order_off_the_calling_thread() {
    let compiler = PipelineCompiler::spawn();
    assert!(compiler.is_active());
    let caller = std::thread::current().id();
    for lane in [CompileLane::Demanded, CompileLane::WarmUp] {
        let (finished, observed) = mpsc::channel();
        for index in 0..3 {
            let finished = finished.clone();
            compiler.enqueue(lane, move || {
                finished.send((index, std::thread::current().id())).unwrap();
            });
        }
        for expected in 0..3 {
            let (index, thread) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(index, expected);
            assert_ne!(thread, caller);
        }
    }
}

#[test]
fn a_demanded_job_runs_while_a_warm_up_is_still_compiling() {
    let compiler = PipelineCompiler::spawn();
    let (started, warm_up_started) = mpsc::channel();
    let (release, blocked) = mpsc::channel::<()>();
    compiler.enqueue(CompileLane::WarmUp, move || {
        started.send(()).unwrap();
        blocked.recv().unwrap();
    });
    warm_up_started
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let (ran, demanded_ran) = mpsc::channel();
    compiler.enqueue(CompileLane::Demanded, move || ran.send(()).unwrap());
    assert_eq!(
        demanded_ran.recv_timeout(Duration::from_secs(5)),
        Ok(()),
        "a demanded job must not wait behind a warm-up"
    );
    release.send(()).unwrap();
}

#[test]
fn dropping_every_handle_skips_the_jobs_not_started() {
    let compiler = PipelineCompiler::spawn();
    let handle = compiler.clone();
    let (started, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel::<()>();
    compiler.enqueue(CompileLane::WarmUp, move || {
        started.send("first").unwrap();
        blocked.recv().unwrap();
    });
    let (ran, second_ran) = mpsc::channel();
    compiler.enqueue(CompileLane::WarmUp, move || ran.send("second").unwrap());
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5)).unwrap(),
        "first"
    );
    drop(compiler);
    assert!(handle.is_active(), "a surviving handle keeps the worker");
    // The last handle waits for the job in flight, so it is released while
    // that drop waits.
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).expect("the first job is waiting");
    });
    drop(handle);
    releaser.join().expect("the releaser ends");
    assert_eq!(
        second_ran.recv_timeout(Duration::from_millis(500)),
        Err(mpsc::RecvTimeoutError::Disconnected),
        "the queued job must be dropped, not run"
    );
}

#[test]
fn an_inactive_compiler_drops_jobs() {
    let compiler = PipelineCompiler::inactive();
    assert!(!compiler.is_active());
    let (ran, observed) = mpsc::channel();
    compiler.enqueue(CompileLane::Demanded, move || ran.send(()).unwrap());
    let deadline = Instant::now() + Duration::from_millis(100);
    assert!(observed.recv_timeout(deadline - Instant::now()).is_err());
}

#[test]
fn the_last_handle_waits_for_the_compile_in_flight() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let compiler = PipelineCompiler::spawn();
    let (started, compiling) = mpsc::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&finished);
    compiler.enqueue(CompileLane::WarmUp, move || {
        started.send(()).expect("the test is waiting");
        std::thread::sleep(Duration::from_millis(200));
        done.store(true, Ordering::Release);
    });
    compiling
        .recv_timeout(Duration::from_secs(5))
        .expect("the compile starts");

    drop(compiler);

    assert!(
        finished.load(Ordering::Acquire),
        "a dropped compiler must not leave a compile running, which a process exit can crash"
    );
}
