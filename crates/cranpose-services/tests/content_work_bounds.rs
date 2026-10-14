//! One-shot content work, such as reading a whole file, runs on the shared
//! bounded blocking executor: a burst of it never runs on more threads than
//! the executor has.

use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use cranpose_services::run_threaded;

#[test]
fn a_burst_of_content_work_runs_within_the_executor_threads() {
    let threads = thread::available_parallelism().map_or(1, NonZeroUsize::get);
    let running = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let callers: Vec<_> = (0..threads * 4)
        .map(|_| {
            let (running, most) = (Arc::clone(&running), Arc::clone(&most));
            thread::spawn(move || {
                pollster::block_on(run_threaded(move || {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(20));
                    running.fetch_sub(1, Ordering::SeqCst);
                    Ok(())
                }))
            })
        })
        .collect();
    for caller in callers {
        assert!(caller.join().expect("caller").is_ok());
    }
    let most = most.load(Ordering::SeqCst);
    assert!(
        most <= threads,
        "{most} jobs ran at once on {threads} threads"
    );
}
