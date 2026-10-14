use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use super::*;
use crate::{DefaultScheduler, Runtime};

thread_local! {
    static WAKE_CLOCK: std::cell::RefCell<Option<FrameClock>> = const { std::cell::RefCell::new(None) };
}

/// Registers a frame callback when woken, which borrows the runtime's frame
/// callbacks.
struct RegisterFrameCallbackOnWake;

impl Wake for RegisterFrameCallbackOnWake {
    fn wake(self: Arc<Self>) {
        WAKE_CLOCK.with(|clock| {
            if let Some(clock) = clock.borrow().as_ref() {
                drop(clock.with_frame_nanos(|_| {}));
            }
        });
    }
}

#[test]
fn next_frame_wakes_after_releasing_the_frame_callbacks() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let mut next_frame = Box::pin(clock.next_frame());

    WAKE_CLOCK.with(|slot| *slot.borrow_mut() = Some(clock.clone()));
    let waker = Waker::from(Arc::new(RegisterFrameCallbackOnWake));
    let mut context = Context::from_waker(&waker);
    assert_eq!(next_frame.as_mut().poll(&mut context), Poll::Pending);
    assert_eq!(next_frame.as_mut().poll(&mut context), Poll::Pending);

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.drain_frame_callbacks(123);
    }));
    WAKE_CLOCK.with(|slot| *slot.borrow_mut() = None);

    assert!(
        result.is_ok(),
        "the runtime must release its frame callbacks before waking a task"
    );
    assert_eq!(next_frame.as_mut().poll(&mut context), Poll::Ready(123));
}

#[test]
fn frame_callback_registration_tracks_transient_and_perpetual_work() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let transient = clock.with_frame_nanos(|_| {});
    let perpetual_handle = handle.clone();
    let perpetual = clock.with_perpetual_frame_nanos(move |_| {
        perpetual_handle.register_perpetual_frame_callback(|_| {});
    });

    assert!(handle.has_frame_callbacks());
    assert!(handle.has_transient_frame_callbacks());

    transient.cancel();
    assert!(handle.has_frame_callbacks());
    assert!(!handle.has_transient_frame_callbacks());

    handle.drain_frame_callbacks(123);
    assert!(handle.has_frame_callbacks());
    assert!(!handle.has_transient_frame_callbacks());
    drop(perpetual);
    handle.drain_frame_callbacks(456);
    assert!(!handle.has_frame_callbacks());
}
