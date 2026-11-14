use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use cranpose_core::Runtime;
use cranpose_runtime_std::StdScheduler;

use super::{FrameWakes, primary_frame_waker};

fn counting(counter: &Arc<AtomicUsize>) -> impl Fn() + Send + Sync + 'static {
    let counter = Arc::clone(counter);
    move || {
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn primary_frame_waker_on_the_event_loop_thread_redraws_a_shown_window_directly() {
    let redraws = Arc::new(AtomicUsize::new(0));
    let wake_ups = Arc::new(AtomicUsize::new(0));
    let shown = Arc::new(AtomicBool::new(true));
    let waker = primary_frame_waker(
        std::thread::current().id(),
        Some(Arc::clone(&shown)),
        Arc::default(),
        counting(&redraws),
        counting(&wake_ups),
    );

    waker();
    assert_eq!(redraws.load(Ordering::SeqCst), 1);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 0);

    shown.store(false, Ordering::SeqCst);
    waker();
    assert_eq!(redraws.load(Ordering::SeqCst), 1);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 1);
}

#[test]
fn frame_wakes_of_one_turn_of_the_loop_reach_the_window_system_once() {
    let redraws = Arc::new(AtomicUsize::new(0));
    let wake_ups = Arc::new(AtomicUsize::new(0));
    let shown = Arc::new(AtomicBool::new(true));
    let wakes = Arc::new(FrameWakes::default());
    let waker = primary_frame_waker(
        std::thread::current().id(),
        Some(Arc::clone(&shown)),
        Arc::clone(&wakes),
        counting(&redraws),
        counting(&wake_ups),
    );
    let asked = |count: usize| {
        (0..count).for_each(|_| waker());
        (
            redraws.load(Ordering::SeqCst),
            wake_ups.load(Ordering::SeqCst),
        )
    };

    assert_eq!(asked(3), (1, 0), "one request serves the three asks");
    wakes.redraw_taken(&mut true);
    assert_eq!(
        asked(2),
        (2, 0),
        "a redraw taken up lets the next ask through"
    );
    wakes.loop_waits();
    assert_eq!(
        asked(1),
        (3, 0),
        "a request the loop waited past is asked again"
    );

    shown.store(false, Ordering::SeqCst);
    assert_eq!(asked(3), (3, 1), "a hidden window wakes the proxy once");
    wakes.proxy_taken();
    assert_eq!(asked(1), (3, 2));
}

#[test]
fn background_ui_post_returns_without_the_event_loop_thread() {
    let (main_queue, main_queue_jobs) = mpsc::channel::<mpsc::SyncSender<()>>();
    let wake_ups = Arc::new(AtomicUsize::new(0));
    let scheduler = Arc::new(StdScheduler::new());
    scheduler.set_frame_waker(primary_frame_waker(
        std::thread::current().id(),
        Some(Arc::new(AtomicBool::new(true))),
        Arc::default(),
        move || {
            let (serviced, wait_for_main_thread) = mpsc::sync_channel(0);
            if main_queue.send(serviced).is_ok() {
                let _ = wait_for_main_thread.recv();
            }
        },
        counting(&wake_ups),
    ));
    let runtime = Runtime::new(Arc::<StdScheduler>::clone(&scheduler));
    let dispatcher = runtime.handle().dispatcher();
    let (posted, post_returned) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        dispatcher.post(|| {});
        let _ = posted.send(());
    });

    let returned = post_returned.recv_timeout(Duration::from_secs(5)).is_ok();
    let mut redraws_waiting_on_main_thread = 0;
    for serviced in main_queue_jobs.try_iter() {
        redraws_waiting_on_main_thread += 1;
        let _ = serviced.send(());
    }
    let worker_finished = worker.join().is_ok();

    assert!(
        returned,
        "a background post waited for the event loop thread"
    );
    assert!(worker_finished);
    assert_eq!(redraws_waiting_on_main_thread, 0);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 1);
    assert!(scheduler.has_frame_request());
    assert!(runtime.handle().has_pending_ui());
}

#[test]
fn a_headless_app_and_another_thread_wake_the_event_loop() {
    let redraws = Arc::new(AtomicUsize::new(0));
    let wake_ups = Arc::new(AtomicUsize::new(0));
    let headless = primary_frame_waker(
        std::thread::current().id(),
        None,
        Arc::default(),
        counting(&redraws),
        counting(&wake_ups),
    );
    headless();
    assert_eq!(redraws.load(Ordering::SeqCst), 0);
    assert_eq!(wake_ups.load(Ordering::SeqCst), 1);

    let shown = primary_frame_waker(
        std::thread::current().id(),
        Some(Arc::new(AtomicBool::new(true))),
        Arc::default(),
        counting(&redraws),
        counting(&wake_ups),
    );
    let joined = std::thread::spawn(shown).join();
    assert!(joined.is_ok());
    assert_eq!(
        redraws.load(Ordering::SeqCst),
        0,
        "no redraw off the loop's thread"
    );
    assert_eq!(wake_ups.load(Ordering::SeqCst), 2);
}
