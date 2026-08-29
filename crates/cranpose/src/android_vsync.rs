#![allow(unsafe_code)]

use std::{
    ffi::c_void,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

static CALLBACK_POSTED: AtomicBool = AtomicBool::new(false);

static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

pub(crate) fn install_waker(waker: impl Fn() + Send + Sync + 'static) {
    CALLBACK_POSTED.store(false, Ordering::Release);
    UNAVAILABLE.store(false, Ordering::Release);
    *WAKER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(waker));
}

pub(crate) fn request_wake_at_next_vsync() -> bool {
    if UNAVAILABLE.load(Ordering::Relaxed)
        || WAKER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_none()
    {
        return false;
    }
    if CALLBACK_POSTED.swap(true, Ordering::AcqRel) {
        return true;
    }
    // SAFETY: called on `android_main`, which `android-activity` runs on a
    // prepared looper; the callback is a `'static` function and takes no user
    // data.
    unsafe {
        let choreographer = ndk_sys::AChoreographer_getInstance();
        if choreographer.is_null() {
            CALLBACK_POSTED.store(false, Ordering::Release);
            UNAVAILABLE.store(true, Ordering::Relaxed);
            log::warn!(
                "[android-vsync] AChoreographer_getInstance returned null; frame loop will fall back to polling"
            );
            return false;
        }
        ndk_sys::AChoreographer_postFrameCallback64(
            choreographer,
            Some(on_vsync),
            std::ptr::null_mut(),
        );
    }
    true
}

unsafe extern "C" fn on_vsync(frame_time_ns: i64, _data: *mut c_void) {
    CALLBACK_POSTED.store(false, Ordering::Release);
    let previous = LAST_VSYNC_NS.swap(frame_time_ns, Ordering::Relaxed);
    if previous != 0 {
        let delta = frame_time_ns - previous;
        if (4_000_000..50_000_000).contains(&delta) {
            VSYNC_PERIOD_NS.store(delta, Ordering::Relaxed);
        }
    }
    let waker = WAKER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(waker) = waker {
        waker();
    }
}

use std::sync::atomic::AtomicI64;

static LAST_VSYNC_NS: AtomicI64 = AtomicI64::new(0);
static VSYNC_PERIOD_NS: AtomicI64 = AtomicI64::new(0);

pub(crate) fn observed_vsync_period_ns() -> Option<i64> {
    match VSYNC_PERIOD_NS.load(Ordering::Relaxed) {
        0 => None,
        period => Some(period),
    }
}
