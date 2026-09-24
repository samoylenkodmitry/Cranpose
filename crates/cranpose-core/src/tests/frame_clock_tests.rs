use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

use super::*;
use crate::{DefaultScheduler, Runtime};

thread_local! {
    static REENTRANT_NEXT_FRAME_STATE: RefCell<Option<Rc<RefCell<NextFrameState>>>> =
        const { RefCell::new(None) };
}

struct BorrowNextFrameStateOnWake;

impl Wake for BorrowNextFrameStateOnWake {
    fn wake(self: Arc<Self>) {
        REENTRANT_NEXT_FRAME_STATE.with(|slot| {
            let state = slot.borrow();
            let Some(state) = state.as_ref() else {
                return;
            };
            let _time = state.borrow().time;
        });
    }
}

#[test]
fn next_frame_wakes_after_releasing_state_borrow() {
    let runtime = Runtime::new(Arc::new(DefaultScheduler));
    let handle = runtime.handle();
    let clock = runtime.frame_clock();
    let mut next_frame = Box::pin(clock.next_frame());

    REENTRANT_NEXT_FRAME_STATE.with(|slot| {
        *slot.borrow_mut() = Some(Rc::clone(&next_frame.state));
    });

    let waker = Waker::from(Arc::new(BorrowNextFrameStateOnWake));
    let mut context = Context::from_waker(&waker);
    assert_eq!(next_frame.as_mut().poll(&mut context), Poll::Pending);

    let result = catch_unwind(AssertUnwindSafe(|| {
        handle.drain_frame_callbacks(123);
    }));

    REENTRANT_NEXT_FRAME_STATE.with(|slot| {
        *slot.borrow_mut() = None;
    });

    assert!(
        result.is_ok(),
        "next_frame must release its state borrow before waking the task"
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
