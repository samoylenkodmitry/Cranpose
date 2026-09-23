use std::{
    sync::{Arc, Mutex as StdMutex, PoisonError},
    time::Duration,
};

use coroflow::{CoroutineScope, Dispatchers, Mutex, Semaphore, TestScheduler, delay, yield_now};

fn locked<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn assert_send<T: Send>(_: &T) {}

#[test]
fn a_semaphore_limits_how_many_coroutines_run_at_once() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let semaphore = Semaphore::new(2);
    let finished = Arc::new(StdMutex::new(Vec::new()));
    let clock = Arc::clone(scheduler.dispatcher().clock());
    for _ in 0..5 {
        let (semaphore, finished, clock) =
            (semaphore.clone(), Arc::clone(&finished), Arc::clone(&clock));
        scope.launch(async move {
            let _permit = semaphore.acquire().await;
            delay(Duration::from_millis(100)).await;
            locked(&finished).push(clock.now().as_millis());
        });
    }
    scheduler.advance_time_by(Duration::from_secs(1));
    assert_eq!(*locked(&finished), vec![100, 100, 200, 200, 300]);
    assert_eq!(semaphore.available_permits(), 2);
}

#[test]
fn waiting_coroutines_get_permits_in_the_order_they_asked() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let semaphore = Semaphore::new(1);
    let held = semaphore.try_acquire();
    assert!(held.is_some());
    let order = Arc::new(StdMutex::new(Vec::new()));
    for name in ["a", "b", "c"] {
        let (semaphore, order) = (semaphore.clone(), Arc::clone(&order));
        scope.launch(async move {
            let _permit = semaphore.acquire().await;
            locked(&order).push(name);
            yield_now().await;
        });
    }
    scheduler.run_current();
    assert!(locked(&order).is_empty());
    drop(held);
    assert!(
        semaphore.try_acquire().is_none(),
        "the permit went to the first waiter, not to a latecomer"
    );
    scheduler.run_current();
    assert_eq!(*locked(&order), vec!["a", "b", "c"]);
    assert_eq!(semaphore.available_permits(), 1);
}

#[test]
fn a_cancelled_waiter_passes_the_permit_it_was_handed_to_the_next_one() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let semaphore = Semaphore::new(1);
    let held = semaphore.try_acquire();
    let order = Arc::new(StdMutex::new(Vec::new()));
    let jobs: Vec<_> = ["b", "c"]
        .into_iter()
        .map(|name| {
            let (semaphore, order) = (semaphore.clone(), Arc::clone(&order));
            scope.launch(async move {
                let _permit = semaphore.acquire().await;
                locked(&order).push(name);
            })
        })
        .collect();
    scheduler.run_current();
    drop(held);
    if let Some(first_waiter) = jobs.first() {
        first_waiter.cancel();
    }
    scheduler.run_current();
    assert_eq!(*locked(&order), vec!["c"]);
    assert_eq!(semaphore.available_permits(), 1);
}

#[test]
fn a_mutex_keeps_a_read_modify_write_whole_across_suspension_points() {
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let counter = Mutex::new(0_u32);
    for _ in 0..10 {
        let counter = counter.clone();
        scope.launch(async move {
            let mut value = counter.lock().await;
            let read = *value;
            yield_now().await;
            *value = read + 1;
        });
    }
    scheduler.run_current();
    assert!(!counter.is_locked());
    assert_eq!(counter.try_lock().map(|value| *value), Some(10));
}

#[test]
fn try_lock_fails_while_the_lock_is_held_and_succeeds_after() {
    let names = Mutex::new(vec!["a"]);
    let mut guard = names.try_lock();
    assert!(names.is_locked());
    assert!(names.try_lock().is_none());
    if let Some(names) = guard.as_mut() {
        names.push("b");
    }
    drop(guard);
    assert_eq!(
        names.try_lock().map(|names| names.clone()),
        Some(vec!["a", "b"])
    );
}

#[test]
fn a_guard_may_be_held_across_an_await_on_a_background_pool() {
    let counter = Mutex::new(0_u32);
    let scope = CoroutineScope::new(Dispatchers::default_pool());
    let jobs: Vec<_> = (0..8)
        .map(|_| {
            let counter = counter.clone();
            let work = async move {
                let mut value = counter.lock().await;
                let read = *value;
                delay(Duration::from_millis(1)).await;
                *value = read + 1;
            };
            assert_send(&work);
            scope.launch(work)
        })
        .collect();
    for job in &jobs {
        pollster::block_on(job.join());
    }
    assert_eq!(counter.try_lock().map(|value| *value), Some(8));
}
