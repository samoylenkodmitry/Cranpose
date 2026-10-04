use std::sync::mpsc;

use web_time::{Duration, Instant};

use super::{CompileLane, PipelineCompiler};

/// Both lanes run every job off the calling thread. Each starts its jobs in
/// queue order, on several threads, so they may finish in any.
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
        indices.sort_unstable();
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

/// Holds every warm-up thread of `compiler` until the returned senders
/// drop, so nothing queued for them lands meanwhile.
pub(crate) fn hold_warm_ups(compiler: &PipelineCompiler) -> Vec<mpsc::Sender<()>> {
    hold(compiler, CompileLane::WarmUp, super::warm_up_threads())
}

/// Holds `count` threads of `compiler` inside jobs on `lane` until the
/// returned senders drop.
fn hold(compiler: &PipelineCompiler, lane: CompileLane, count: usize) -> Vec<mpsc::Sender<()>> {
    let (started, holding) = mpsc::channel();
    let releases = (0..count)
        .map(|_| {
            let (release, held) = mpsc::channel::<()>();
            let started = started.clone();
            compiler.enqueue(lane, move || {
                started.send(()).unwrap();
                let _ = held.recv();
            });
            release
        })
        .collect();
    for _ in 0..count {
        holding.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    releases
}

/// A screen of new materials demands many pipelines at once; once frames
/// stop building their own, the warm-up threads take them while the
/// demanded lane's own thread is busy.
#[test]
fn demanded_jobs_compile_on_idle_warm_up_threads_once_spread() {
    let compiler = PipelineCompiler::spawn();
    compiler.spread_demand(true);
    let threads = 1 + super::warm_up_threads();
    let _held = hold(&compiler, CompileLane::Demanded, threads);
}

/// While a frame builds its own pipelines, demanded jobs keep to their one
/// thread, so no more compiles run beside the frame's.
#[test]
fn a_demanded_job_waits_for_its_own_thread_until_spread() {
    let compiler = PipelineCompiler::spawn();
    let held = hold(&compiler, CompileLane::Demanded, 1);
    let (ran, demanded_ran) = mpsc::channel();
    compiler.enqueue(CompileLane::Demanded, move || ran.send(()).unwrap());
    assert!(
        demanded_ran
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "idle warm-up threads leave it queued"
    );
    compiler.spread_demand(true);
    assert_eq!(
        demanded_ran.recv_timeout(Duration::from_secs(5)),
        Ok(()),
        "a warm-up thread takes it once spread"
    );
    drop(held);
}

/// A warm-up waits for every demanded job queued before it, however many
/// threads are idle.
#[test]
fn a_warm_up_waits_while_demanded_jobs_are_queued() {
    let compiler = PipelineCompiler::spawn();
    compiler.spread_demand(true);
    let threads = 1 + super::warm_up_threads();
    let held = hold(&compiler, CompileLane::Demanded, threads);
    let (ran, order) = mpsc::channel();
    let warm_up = ran.clone();
    compiler.enqueue(CompileLane::WarmUp, move || {
        warm_up.send("warm-up").unwrap();
    });
    compiler.enqueue(CompileLane::Demanded, move || ran.send("demanded").unwrap());
    assert!(
        order.recv_timeout(Duration::from_millis(100)).is_err(),
        "every thread is held"
    );
    drop(held);
    assert_eq!(order.recv_timeout(Duration::from_secs(5)), Ok("demanded"));
    assert_eq!(order.recv_timeout(Duration::from_secs(5)), Ok("warm-up"));
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

/// Runs a job on the demanded lane and returns once it is counted built.
fn build_one(compiler: &PipelineCompiler) {
    let landing = compiler.landing().expect("a spawned compiler");
    let built = landing.built();
    compiler.enqueue(CompileLane::Demanded, || {});
    let deadline = Instant::now() + Duration::from_secs(5);
    while landing.built() == built {
        assert!(Instant::now() < deadline, "the lane runs its jobs");
        std::thread::sleep(Duration::from_millis(1));
    }
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
        wakes.recv_timeout(Duration::from_secs(5)).is_ok(),
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
