use std::sync::mpsc;

use web_time::{Duration, Instant};

use super::{CompileLane, PipelineCompiler};

/// Both lanes run every job off the calling thread; the demanded lane's one
/// thread runs them in queue order, while the warm-up lane's threads start
/// them in that order and may finish them in any.
#[test]
fn each_lane_runs_its_jobs_off_the_calling_thread() {
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
        let mut indices: Vec<_> = (0..3)
            .map(|_| {
                let (index, thread) = observed.recv_timeout(Duration::from_secs(5)).unwrap();
                assert_ne!(thread, caller);
                index
            })
            .collect();
        if lane == CompileLane::WarmUp {
            indices.sort_unstable();
        }
        assert_eq!(indices, [0, 1, 2]);
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
    let threads = super::warm_up_threads();
    let releases: Vec<_> = (0..threads)
        .map(|_| {
            let (release, blocked) = mpsc::channel::<()>();
            let started = started.clone();
            compiler.enqueue(CompileLane::WarmUp, move || {
                started.send("blocking").unwrap();
                blocked.recv().unwrap();
            });
            release
        })
        .collect();
    let (ran, second_ran) = mpsc::channel();
    compiler.enqueue(CompileLane::WarmUp, move || ran.send("second").unwrap());
    for _ in 0..threads {
        assert_eq!(
            observed.recv_timeout(Duration::from_secs(5)).unwrap(),
            "blocking"
        );
    }
    drop(compiler);
    assert!(handle.is_active(), "a surviving handle keeps the worker");
    // The last handle waits for the jobs in flight, so they are released
    // while that drop waits.
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        for release in releases {
            release.send(()).expect("a blocking job is waiting");
        }
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

/// Runs a job on the demanded lane and returns once its thread is done with
/// it, landing included: the lane runs its jobs in order.
fn build_one(compiler: &PipelineCompiler) {
    let (done, finished) = mpsc::channel();
    compiler.enqueue(CompileLane::Demanded, || {});
    compiler.enqueue(CompileLane::Demanded, move || {
        done.send(()).expect("test waits");
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("the lane runs its jobs");
}

#[test]
fn a_pipeline_landing_after_a_placeholder_wakes_the_app_once() {
    let compiler = PipelineCompiler::spawn();
    let landing = compiler
        .landing()
        .expect("a spawned compiler reports landings");
    let (woke, wakes) = mpsc::channel();
    landing.wake_with(Box::new(move || {
        let _ = woke.send(());
    }));
    build_one(&compiler);
    assert!(
        wakes.try_recv().is_err(),
        "a landing nothing waits for wakes nothing"
    );
    landing.await_since(landing.built());
    build_one(&compiler);
    assert!(
        wakes.try_recv().is_ok(),
        "the awaited landing wakes the app"
    );
    assert!(wakes.try_recv().is_err(), "once");
    assert!(landing.take_landed());
    assert!(!landing.take_landed(), "reading the landing clears it");
}

#[test]
fn a_pipeline_landing_before_its_placeholder_is_noted_still_counts() {
    let compiler = PipelineCompiler::spawn();
    let landing = compiler
        .landing()
        .expect("a spawned compiler reports landings");
    let frame_start = landing.built();
    build_one(&compiler);
    landing.await_since(frame_start);
    assert!(
        landing.landed(),
        "a pipeline built during the frame that drew the placeholder replaces it next frame"
    );
}
