#![expect(unsafe_code)]

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

struct PerfHintSession {
    session: *mut c_void,
    api: HintApi,
    target_ns: i64,
    present_thread: Option<i32>,
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

/// Tells the scheduler how long each presented frame's work took, against
/// the display's refresh period, for the frame loop's thread and the thread
/// that presents. The session opens at the first report, and again when the
/// present thread changes.
#[derive(Default)]
pub(crate) struct FrameWorkHints {
    session: Option<PerfHintSession>,
    unavailable: bool,
}

impl FrameWorkHints {
    pub(crate) fn report(&mut self, work_ns: i64, target_ns: i64, present_thread: Option<i32>) {
        if self
            .session
            .as_ref()
            .is_some_and(|session| session.present_thread != present_thread)
        {
            self.session = None;
        }
        if self.session.is_none() && !self.unavailable {
            self.session = PerfHintSession::open(target_ns, present_thread);
            self.unavailable = self.session.is_none();
        }
        if let Some(session) = self.session.as_mut() {
            session.report(work_ns, target_ns);
        }
    }
}

impl PerfHintSession {
    fn open(target_ns: i64, present_thread: Option<i32>) -> Option<Self> {
        if !enabled() || target_ns <= 0 {
            return None;
        }
        // SAFETY: symbols come from libandroid.so and the transmutes target
        // the NDK-documented APerformanceHint signatures; null checks gate
        // every call; the thread list outlives the create call that reads it.
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
            let threads: Vec<i32> = std::iter::once(libc::gettid())
                .chain(present_thread)
                .collect();
            let session = create_session(manager, threads.as_ptr(), threads.len(), target_ns);
            if session.is_null() {
                log::info!("[perf-hint] session refused");
                return None;
            }
            log::info!(
                "[perf-hint] session open for threads {threads:?}, target {:.2} ms",
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
                present_thread,
            })
        }
    }

    fn report(&mut self, actual_ns: i64, target_ns: i64) {
        if actual_ns <= 0 {
            return;
        }
        // SAFETY: session is the live pointer `open` created on this thread.
        unsafe {
            if target_ns > 0
                && (target_ns - self.target_ns).abs() > self.target_ns / 64
                && (self.api.update_target)(self.session, target_ns) == 0
            {
                self.target_ns = target_ns;
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
