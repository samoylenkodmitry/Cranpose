//! ADPF performance-hint session for the Android frame loop.
//!
//! On a small watch the cpufreq governor samples the frame loop into a lower
//! OPP whenever a few frames finish early, and the next heavy frame then
//! misses its deadline before the clock climbs back — measured on a Pixel
//! Watch 3 as uniform double-vsync presents every few seconds with no other
//! system activity, the difference between 59.7 fps and a locked 60. The
//! platform's answer is the performance-hint session: the loop declares its
//! target work duration (one vsync) and reports the actual duration every
//! presented frame, and the kernel holds the clock exactly where the
//! deadline needs it — the sanctioned, vendor-neutral form of the priority
//! and DVFS pinning an unprivileged app cannot do itself.
//!
//! `APerformanceHint_*` lives in `libandroid.so` from API 33; every symbol
//! is `dlsym`-resolved so a minSdk 29 build loads everywhere and quietly
//! does nothing where the API (or a vendor implementation) is absent.
//! `CRANPOSE_ADPF=0` (property `debug.cranpose.adpf`) is the kill switch.

#![allow(unsafe_code)]

use std::ffi::c_void;

type GetManagerFn = unsafe extern "C" fn() -> *mut c_void;
type CreateSessionFn = unsafe extern "C" fn(*mut c_void, *const i32, usize, i64) -> *mut c_void;
type UpdateTargetFn = unsafe extern "C" fn(*mut c_void, i64) -> i32;
type ReportActualFn = unsafe extern "C" fn(*mut c_void, i64) -> i32;
type CloseSessionFn = unsafe extern "C" fn(*mut c_void);

struct HintApi {
    update_target: UpdateTargetFn,
    report_actual: ReportActualFn,
    close_session: CloseSessionFn,
}

/// One hint session for the calling thread. Created on the frame-loop
/// thread so the session's thread list is exactly the loop itself; the
/// worker pool's threads earn their cycles through the loop's reports (the
/// governor scales the cluster, not a core).
pub(crate) struct PerfHintSession {
    session: *mut c_void,
    api: HintApi,
    target_ns: i64,
}

// SAFETY: the session pointer is used and closed only from the frame-loop
// thread that owns this value; the NDK object itself is thread-safe.
unsafe impl Send for PerfHintSession {}

fn enabled() -> bool {
    std::env::var("CRANPOSE_ADPF").as_deref() != Ok("0")
}

unsafe fn resolve(name: &std::ffi::CStr) -> *mut c_void {
    // SAFETY: dlsym/dlopen with a static NUL-terminated name; libandroid.so
    // is always loadable by an app process and never closed here.
    unsafe {
        let direct = libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr());
        if !direct.is_null() {
            return direct;
        }
        let library = libc::dlopen(c"libandroid.so".as_ptr(), libc::RTLD_LAZY);
        if library.is_null() {
            return std::ptr::null_mut();
        }
        libc::dlsym(library, name.as_ptr())
    }
}

impl PerfHintSession {
    /// Opens the session with `target_ns` as the declared work budget.
    /// `None` where ADPF is absent, disabled, or refuses the session —
    /// callers keep exactly the pre-ADPF behavior.
    pub(crate) fn open(target_ns: i64) -> Option<Self> {
        if !enabled() || target_ns <= 0 {
            return None;
        }
        // SAFETY: symbols come from libandroid.so and the transmutes target
        // the NDK-documented APerformanceHint signatures; null checks gate
        // every call; gettid names the calling thread, which is the thread
        // the session is created for.
        unsafe {
            let get_manager = resolve(c"APerformanceHint_getManager");
            let create_session = resolve(c"APerformanceHint_createSession");
            let update_target = resolve(c"APerformanceHint_updateTargetWorkDuration");
            let report_actual = resolve(c"APerformanceHint_reportActualWorkDuration");
            let close_session = resolve(c"APerformanceHint_closeSession");
            if get_manager.is_null()
                || create_session.is_null()
                || update_target.is_null()
                || report_actual.is_null()
                || close_session.is_null()
            {
                log::info!("[perf-hint] APerformanceHint unavailable; running without");
                return None;
            }
            let get_manager = std::mem::transmute::<*mut c_void, GetManagerFn>(get_manager);
            let create_session =
                std::mem::transmute::<*mut c_void, CreateSessionFn>(create_session);
            let manager = get_manager();
            if manager.is_null() {
                log::info!("[perf-hint] no hint manager on this device");
                return None;
            }
            let tid = libc::gettid();
            let session = create_session(manager, &tid, 1, target_ns);
            if session.is_null() {
                log::info!("[perf-hint] session refused");
                return None;
            }
            log::info!(
                "[perf-hint] session open, target {:.2} ms",
                target_ns as f64 / 1e6
            );
            Some(Self {
                session,
                api: HintApi {
                    update_target: std::mem::transmute::<*mut c_void, UpdateTargetFn>(
                        update_target,
                    ),
                    report_actual: std::mem::transmute::<*mut c_void, ReportActualFn>(
                        report_actual,
                    ),
                    close_session: std::mem::transmute::<*mut c_void, CloseSessionFn>(
                        close_session,
                    ),
                },
                target_ns,
            })
        }
    }

    /// Reports one presented frame's work duration, refreshing the target
    /// first when the display period moved (mode switch); tiny jitter in
    /// the period estimate is not a new target.
    pub(crate) fn report(&mut self, actual_ns: i64, target_ns: i64) {
        if actual_ns <= 0 {
            return;
        }
        // SAFETY: session is the live pointer `open` created on this thread.
        unsafe {
            if target_ns > 0 && (target_ns - self.target_ns).abs() > self.target_ns / 64 {
                if (self.api.update_target)(self.session, target_ns) == 0 {
                    self.target_ns = target_ns;
                }
            }
            (self.api.report_actual)(self.session, actual_ns);
        }
    }
}

impl Drop for PerfHintSession {
    fn drop(&mut self) {
        // SAFETY: closes the pointer `open` created; dropped on the same
        // thread, after which it is never touched.
        unsafe { (self.api.close_session)(self.session) }
    }
}
