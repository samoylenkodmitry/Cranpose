use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use coroflow::{CoroutineScope, JobOutcome, MutableStateFlow, TestScheduler};
use cranpose_coroflow::{LifecycleFlowExt, lifecycle_state_flow, repeat_on_lifecycle};
use cranpose_services::{LifecycleState, dispatch_lifecycle_state};

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn lifecycle_flows_follow_the_host() {
    let scheduler = TestScheduler::new();
    let states = lifecycle_state_flow();
    dispatch_lifecycle_state(LifecycleState::Started);
    dispatch_lifecycle_state(LifecycleState::Resumed);
    assert_eq!(states.value(), LifecycleState::Resumed);

    let source = MutableStateFlow::new(1);
    let gated = source
        .as_state_flow()
        .flow_with_lifecycle(LifecycleState::Started);
    let mut run = scheduler.turbine(&gated);
    assert_eq!(run.await_item(), 1);
    assert_eq!(source.subscription_count().value(), 1);
    dispatch_lifecycle_state(LifecycleState::Paused);
    dispatch_lifecycle_state(LifecycleState::Stopped);
    run.expect_no_events();
    assert_eq!(
        source.subscription_count().value(),
        0,
        "stopped hosts do not collect"
    );
    source.set(2);
    dispatch_lifecycle_state(LifecycleState::Started);
    assert_eq!(run.await_item(), 2, "restarted with the latest value");
    drop(run);

    let scope = CoroutineScope::new(scheduler.dispatcher());
    let (starts, cancels) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (started, cancelled) = (Arc::clone(&starts), Arc::clone(&cancels));
    let job = scope.launch(repeat_on_lifecycle(
        LifecycleState::Started,
        async move || {
            started.fetch_add(1, Ordering::SeqCst);
            let _marker = DropMarker(Arc::clone(&cancelled));
            std::future::pending::<()>().await;
        },
    ));
    scheduler.run_current();
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    dispatch_lifecycle_state(LifecycleState::Stopped);
    scheduler.run_current();
    assert_eq!(cancels.load(Ordering::SeqCst), 1);
    dispatch_lifecycle_state(LifecycleState::Resumed);
    scheduler.run_current();
    assert_eq!(starts.load(Ordering::SeqCst), 2, "runs again once back");
    dispatch_lifecycle_state(LifecycleState::Destroyed);
    scheduler.run_current();
    assert_eq!(
        job.outcome(),
        Some(JobOutcome::Completed),
        "returns once destroyed"
    );
}
