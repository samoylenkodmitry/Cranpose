use std::{
    cell::RefCell,
    num::NonZeroUsize,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

use coroflow::{
    CoroutineScope, DispatcherPool, DispatcherPoolConfig, Dispatchers, JobOutcome, yield_now,
};

const PATIENCE: Duration = Duration::from_secs(3);

fn pool() -> DispatcherPool {
    DispatcherPool::new(DispatcherPoolConfig {
        cpu_parallelism: NonZeroUsize::MIN,
        io_parallelism: NonZeroUsize::MIN,
        idle_timeout: Duration::from_millis(20),
    })
}

#[derive(Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn block(&self, scope: &CoroutineScope) -> coroflow::Job {
        let held = Arc::clone(&self.0);
        let (started, wait) = mpsc::channel();
        let job = scope.launch(async move {
            let guard = held.0.lock().expect("gate");
            started.send(()).expect("waiting for worker");
            drop(
                held.1
                    .wait_while(guard, |released| !*released)
                    .expect("release"),
            );
        });
        wait.recv_timeout(PATIENCE).expect("worker started");
        job
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        *self.0.0.lock().expect("gate") = true;
        self.0.1.notify_all();
    }
}

struct DropNotice(mpsc::Sender<()>);

impl Drop for DropNotice {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

thread_local! {
    static RESOURCE: RefCell<Option<DropNotice>> = const { RefCell::new(None) };
}

fn touch_resource(scope: &CoroutineScope) -> mpsc::Receiver<()> {
    let (dropped, observed) = mpsc::channel();
    let job = scope.launch(async move {
        RESOURCE.with(|slot| *slot.borrow_mut() = Some(DropNotice(dropped)));
    });
    assert_eq!(pollster::block_on(job.join()), JobOutcome::Completed);
    observed
}

#[test]
fn idle_workers_release_thread_resources_and_restart_for_new_work() {
    let pool = pool();
    for dispatcher in [pool.cpu(), pool.io()] {
        let scope = CoroutineScope::new(dispatcher);
        for _ in 0..2 {
            touch_resource(&scope)
                .recv_timeout(PATIENCE)
                .expect("idle thread resource released while dispatcher is still alive");
        }
    }
}

#[test]
fn dropping_the_last_dispatcher_releases_thread_resources_without_the_idle_timeout() {
    let pool = DispatcherPool::new(DispatcherPoolConfig {
        idle_timeout: Duration::from_secs(60),
        ..DispatcherPoolConfig::default()
    });
    let scope = CoroutineScope::new(pool.cpu());
    let observed = touch_resource(&scope);
    drop(pool);
    assert!(observed.try_recv().is_err());
    drop(scope);
    observed.recv_timeout(PATIENCE).expect("pool shut down");
}

#[test]
fn a_saturated_lane_does_not_starve_the_other_lanes_wakeups() {
    let pool = pool();
    for (blocked, available) in [(pool.io(), pool.cpu()), (pool.cpu(), pool.io())] {
        let blocked = CoroutineScope::new(blocked);
        let available = CoroutineScope::new(available);
        let gate = Gate::default();
        gate.block(&blocked);
        let (finished, observed) = mpsc::channel();
        for _ in 0..64 {
            let finished = finished.clone();
            available.launch(async move {
                for _ in 0..64 {
                    yield_now().await;
                }
                finished.send(()).expect("completed job observed");
            });
        }
        for _ in 0..64 {
            observed
                .recv_timeout(PATIENCE)
                .expect("other lane keeps making progress");
        }
    }
}

#[test]
fn each_lane_obeys_its_concurrency_limit_and_drains_queued_work() {
    let pool = pool();
    let scopes = [
        CoroutineScope::new(pool.cpu()),
        CoroutineScope::new(pool.io()),
    ];
    let gate = Gate::default();
    for scope in &scopes {
        gate.block(scope);
    }
    let (finished, observed) = mpsc::channel();
    for scope in &scopes {
        for _ in 0..32 {
            let finished = finished.clone();
            scope.launch(async move { finished.send(()).expect("queued job observed") });
        }
    }
    assert!(observed.recv_timeout(Duration::from_millis(30)).is_err());
    drop(gate);
    for _ in 0..64 {
        observed
            .recv_timeout(PATIENCE)
            .expect("queued job completes");
    }
}

#[test]
fn cancelled_queued_coroutines_drop_captures_without_running_their_body() {
    let pool = pool();
    let scope = CoroutineScope::new(pool.cpu());
    let gate = Gate::default();
    gate.block(&scope);
    let (dropped, observed) = mpsc::channel();
    let resource = DropNotice(dropped);
    let job = scope.launch(async move {
        drop(resource);
        panic!("cancelled body must not run");
    });
    job.cancel();
    drop(gate);
    observed
        .recv_timeout(PATIENCE)
        .expect("captured data released");
    assert_eq!(pollster::block_on(job.join()), JobOutcome::Cancelled);
}

#[test]
fn a_surviving_dispatcher_remains_usable_after_the_pool_handle_is_dropped() {
    let scope = CoroutineScope::new(pool().io());
    for _ in 0..2 {
        touch_resource(&scope)
            .recv_timeout(PATIENCE)
            .expect("resource released");
    }
}

#[test]
fn a_panicking_completion_handler_does_not_strand_the_next_coroutine() {
    let scope = CoroutineScope::new(Dispatchers::single_thread("completion-panic"));
    let (release, wait) = mpsc::channel();
    let first = scope.launch(async move { wait.recv().expect("release first job") });
    first.invoke_on_completion(|_| panic!("completion handler failed"));
    let (second_handler, observed_handler) = mpsc::channel();
    first.invoke_on_completion(move |_| second_handler.send(()).expect("second handler observed"));
    let (completed, next) = mpsc::channel();
    scope.launch(async move { completed.send(()).expect("next job observed") });
    release.send(()).expect("first job waiting");
    assert!(
        next.recv_timeout(Duration::from_secs(2)).is_ok(),
        "a callback panic stranded the next coroutine"
    );
    observed_handler
        .recv_timeout(PATIENCE)
        .expect("later completion handler still runs");
}
