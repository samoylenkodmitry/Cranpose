use std::sync::mpsc;

use web_time::{Duration, Instant};

use super::PipelineCompiler;

#[test]
fn jobs_run_in_queue_order_off_the_calling_thread() {
    let compiler = PipelineCompiler::spawn();
    assert!(compiler.is_active());
    let (finished, observed) = mpsc::channel();
    let caller = std::thread::current().id();
    for index in 0..3 {
        let finished = finished.clone();
        compiler.enqueue(move || {
            finished.send((index, std::thread::current().id())).unwrap();
        });
    }
    for expected in 0..3 {
        let (index, thread) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(index, expected);
        assert_ne!(thread, caller);
    }
}

#[test]
fn dropping_every_handle_skips_the_jobs_not_started() {
    let compiler = PipelineCompiler::spawn();
    let handle = compiler.clone();
    let (started, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel::<()>();
    compiler.enqueue(move || {
        started.send("first").unwrap();
        blocked.recv().unwrap();
    });
    let (ran, second_ran) = mpsc::channel();
    compiler.enqueue(move || ran.send("second").unwrap());
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(5)).unwrap(),
        "first"
    );
    drop(compiler);
    assert!(handle.is_active(), "a surviving handle keeps the worker");
    drop(handle);
    release.send(()).unwrap();
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
    compiler.enqueue(move || ran.send(()).unwrap());
    let deadline = Instant::now() + Duration::from_millis(100);
    assert!(observed.recv_timeout(deadline - Instant::now()).is_err());
}
