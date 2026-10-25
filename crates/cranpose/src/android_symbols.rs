//! Platform functions newer than the oldest Android the library supports
//! (`minSdk` 24), found while the app runs. A direct call would leave an
//! undefined symbol that stops the whole library loading on an older device.

#![expect(unsafe_code, reason = "JNI and NDK calls into the Android system")]

use std::{
    ffi::{CStr, c_int, c_long, c_void},
    sync::OnceLock,
};

/// The callback a frame of the choreographer runs with its frame time.
pub(crate) type FrameCallback = unsafe extern "C" fn(frame_time_ns: i64, data: *mut c_void);

type PostFrameCallback64 = unsafe extern "C" fn(
    choreographer: *mut ndk_sys::AChoreographer,
    callback: Option<FrameCallback>,
    data: *mut c_void,
);

type Mallopt = unsafe extern "C" fn(param: c_int, value: c_int) -> c_int;

/// The address of the platform function `name`, or null where the device's
/// Android has none.
fn platform_function(name: &CStr) -> *mut c_void {
    // SAFETY: `dlsym` reads the symbols of the libraries the process loaded;
    // `name` ends in a NUL.
    unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) }
}

fn post_frame_callback_64() -> Option<PostFrameCallback64> {
    static FOUND: OnceLock<usize> = OnceLock::new();
    let address =
        *FOUND.get_or_init(|| platform_function(c"AChoreographer_postFrameCallback64") as usize);
    // SAFETY: a non-null address is the platform's
    // `AChoreographer_postFrameCallback64`, whose signature the type states.
    (address != 0).then(|| unsafe { std::mem::transmute::<usize, PostFrameCallback64>(address) })
}

/// Runs `callback` at the next frame of `choreographer`. Android 10 (API 29)
/// and later post it as it is. An older Android posts it through
/// `AChoreographer_postFrameCallback`, whose frame time is a C `long`: on a
/// 32-bit device that wraps every two seconds, so the callback gets the
/// monotonic clock's time instead.
///
/// # Safety
///
/// `choreographer` is the calling thread's choreographer, and the thread
/// runs a looper.
pub(crate) unsafe fn post_frame_callback(
    choreographer: *mut ndk_sys::AChoreographer,
    callback: FrameCallback,
) {
    match post_frame_callback_64() {
        // SAFETY: the caller's guarantee; the callback takes no user data.
        Some(post) => unsafe { post(choreographer, Some(callback), std::ptr::null_mut()) },
        // SAFETY: the caller's guarantee; the user data is the callback
        // itself, which `run_frame_callback` calls.
        None => unsafe {
            ndk_sys::AChoreographer_postFrameCallback(
                choreographer,
                Some(run_frame_callback),
                callback as *mut c_void,
            );
        },
    }
}

/// Calls the callback `data` holds with the frame's time in nanoseconds.
unsafe extern "C" fn run_frame_callback(frame_time_ns: c_long, data: *mut c_void) {
    // SAFETY: `post_frame_callback` passes a `FrameCallback` as the data.
    let callback = unsafe { std::mem::transmute::<*mut c_void, FrameCallback>(data) };
    #[allow(
        clippy::useless_conversion,
        reason = "a C long is an i64 on a 64-bit ABI and an i32 on a 32-bit one"
    )]
    let frame_time_ns = if size_of::<c_long>() == size_of::<i64>() {
        i64::from(frame_time_ns)
    } else {
        monotonic_now_ns()
    };
    // SAFETY: the callback runs on the choreographer's thread, as posted.
    unsafe { callback(frame_time_ns, std::ptr::null_mut()) };
}

fn monotonic_now_ns() -> i64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid `timespec` to write.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) };
    #[allow(
        clippy::useless_conversion,
        reason = "time_t and a C long are i64 on a 64-bit ABI and i32 on a 32-bit one"
    )]
    let now_ns = i64::from(now.tv_sec) * 1_000_000_000 + i64::from(now.tv_nsec);
    now_ns
}

/// Asks the allocator to release its free pages (`mallopt(M_PURGE)`, API 26),
/// and returns whether the device's Android has the call.
pub(crate) fn purge_free_memory() -> bool {
    const M_PURGE: c_int = -101;
    static FOUND: OnceLock<usize> = OnceLock::new();
    let address = *FOUND.get_or_init(|| platform_function(c"mallopt") as usize);
    if address == 0 {
        return false;
    }
    // SAFETY: a non-null address is the platform's `mallopt`. `M_PURGE`
    // releases the allocator's own free pages; it frees nothing a caller
    // holds a pointer to, and an unknown parameter is a no-op.
    unsafe { std::mem::transmute::<usize, Mallopt>(address)(M_PURGE, 0) };
    true
}
