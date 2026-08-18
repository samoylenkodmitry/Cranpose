//! Vsync pacing for the `android_main` frame loop.
//!
//! The loop used to be paced by its own output: a `Fifo` present blocks until
//! the display has taken the previous image, so "render every iteration" and
//! "render once per vsync" were the same thing and nothing else was needed. As
//! soon as the loop learned to *skip* presenting a frame identical to the one
//! already on screen, that pacing disappeared with it - an app holding a frame
//! await open (any game loop, any polling effect) asked for a frame, got no
//! present, asked again, and the loop span at whatever rate the CPU could
//! manage. Measured on a Pixel 9 Pro: a big core pinned at 2.97 GHz and
//! 3062 mW on `CPU(BIG)/S3M_VDD_CPUCL2`, against 4.4 mW for the same screen
//! under HWUI.
//!
//! So the loop needs a clock of its own, and the display's is the right one -
//! it is the rate at which a new frame can possibly become visible, and it is
//! what HWUI paces on. `AChoreographer_postFrameCallback64` delivers it.
//!
//! The callback fires on the looper of the thread that posted it, so
//! [`request_wake_at_next_vsync`] must only ever be called from `android_main`.
//! Waking the looper is not enough on its own: `android-activity` owns the
//! `ALooper_pollOnce` call and decides when to hand control back, so the
//! callback goes through the same [`android_activity::AndroidAppWaker`] the
//! rest of the shell uses rather than relying on the poll returning by itself.

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Set while a frame callback is posted and has not fired yet, so a loop that
/// iterates several times before the next vsync posts one callback, not one per
/// iteration. The choreographer coalesces duplicates anyway; this keeps the
/// bookkeeping honest and the FFI call off the hot path.
static CALLBACK_POSTED: AtomicBool = AtomicBool::new(false);

/// Set once `AChoreographer_getInstance` has failed, so a device that cannot
/// give us a display clock is asked exactly once and the caller falls back to
/// its previous behaviour instead of paying for a null check every iteration.
static UNAVAILABLE: AtomicBool = AtomicBool::new(false);

static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

pub(crate) fn install_waker(waker: impl Fn() + Send + Sync + 'static) {
    CALLBACK_POSTED.store(false, Ordering::Release);
    UNAVAILABLE.store(false, Ordering::Release);
    *WAKER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(waker));
}

/// Asks the display to wake the frame loop at the next vsync.
///
/// Returns `false` when no choreographer is available, which tells the caller
/// to keep polling the way it did before rather than sleep on a clock that will
/// never tick.
///
/// Must be called from `android_main`: the callback is delivered on the looper
/// of the posting thread.
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
            log::warn!("[android-vsync] AChoreographer_getInstance returned null; frame loop will fall back to polling");
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
        // Only single-period deltas train the estimate: catch-up iterations
        // skip arming the choreographer, so consecutive callbacks can be
        // many periods apart and those gaps must not stretch the period.
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

/// Timestamp of the most recent choreographer callback, for period
/// measurement in [`observed_vsync_period_ns`].
static LAST_VSYNC_NS: AtomicI64 = AtomicI64::new(0);
/// Most recent single-period delta between consecutive choreographer
/// callbacks; 0 until two callbacks have been observed.
static VSYNC_PERIOD_NS: AtomicI64 = AtomicI64::new(0);

/// The display's refresh period as measured between consecutive
/// choreographer callbacks, if two have been seen. Reflects
/// SurfaceFlinger's `frameRateOverride` pinning automatically, which a
/// panel-mode query would not.
pub(crate) fn observed_vsync_period_ns() -> Option<i64> {
    match VSYNC_PERIOD_NS.load(Ordering::Relaxed) {
        0 => None,
        period => Some(period),
    }
}
