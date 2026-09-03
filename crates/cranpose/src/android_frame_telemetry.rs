#![allow(unsafe_code)]

use std::{
    ffi::{CString, c_void},
    sync::atomic::{AtomicBool, AtomicI64, Ordering},
};

const DEFAULT_WINDOW_FRAMES: usize = 120;
const PROP_VALUE_MAX: usize = 92;

pub(crate) fn system_property(name: &str) -> Option<String> {
    let name = CString::new(name).ok()?;
    let mut buffer = [0u8; PROP_VALUE_MAX];
    // SAFETY: `name` is a valid NUL-terminated C string and `buffer` has room
    // for `PROP_VALUE_MAX` bytes, which is the documented maximum written.
    let length = unsafe {
        libc::__system_property_get(name.as_ptr(), buffer.as_mut_ptr().cast::<libc::c_char>())
    };
    if length <= 0 {
        return None;
    }
    let value = String::from_utf8_lossy(&buffer[..length as usize])
        .trim()
        .to_owned();
    (!value.is_empty()).then_some(value)
}

fn property_flag(name: &str) -> bool {
    match system_property(name) {
        Some(value) => !matches!(value.as_str(), "0" | "false" | "off" | "no"),
        None => false,
    }
}

const PROPERTY_BACKED_ENV_VARS: [(&str, &str); 54] = [
    ("debug.cranpose.root_direct", "CRANPOSE_ROOT_DIRECT_DIAG"),
    ("debug.cranpose.recomp_diag", "CRANPOSE_RECOMP_DIAG"),
    (
        "debug.cranpose.layout_ms",
        "CRANPOSE_LAYOUT_MEASURE_TELEMETRY_MS",
    ),
    ("debug.cranpose.core_pin", "CRANPOSE_CORE_PIN"),
    ("debug.cranpose.gpu_stats", "CRANPOSE_GPU_STATS"),
    (
        "debug.cranpose.gpu_fence_profile",
        "CRANPOSE_GPU_FENCE_PROFILE",
    ),
    ("debug.cranpose.pass_timing", "CRANPOSE_GPU_PASS_TIMING"),
    (
        "debug.cranpose.composition_8bit",
        "CRANPOSE_COMPOSITION_8BIT",
    ),
    ("debug.cranpose.skip_shadows", "CRANPOSE_SKIP_SHADOWS"),
    ("debug.cranpose.a11y_sync", "CRANPOSE_A11Y_SYNC"),
    ("debug.cranpose.present_thread", "CRANPOSE_PRESENT_THREAD"),
    ("debug.cranpose.command_feed", "CRANPOSE_COMMAND_FEED"),
    ("debug.cranpose.arc_mesh", "CRANPOSE_ARC_MESH"),
    ("debug.cranpose.adpf", "CRANPOSE_ADPF"),
    ("debug.cranpose.rim_mesh", "CRANPOSE_RIM_MESH"),
    ("debug.cranpose.round_cull", "CRANPOSE_ROUND_CULL"),
    (
        "debug.cranpose.retained_mesh_px2",
        "CRANPOSE_RETAINED_MESH_PX2",
    ),
    ("debug.cranpose.catchup_pacing", "CRANPOSE_CATCHUP_PACING"),
    ("debug.cranpose.instanced_quads", "CRANPOSE_INSTANCED_QUADS"),
    (
        "debug.cranpose.retained_bundles",
        "CRANPOSE_RETAINED_BUNDLES",
    ),
    (
        "debug.cranpose.cmd_replay_diag",
        "CRANPOSE_COMMAND_REPLAY_DIAG",
    ),
    (
        "debug.cranpose.similarity_replay",
        "CRANPOSE_SIMILARITY_REPLAY",
    ),
    (
        "debug.cranpose.stale_transition",
        "CRANPOSE_STALE_TRANSITION",
    ),
    (
        "debug.cranpose.update_stage_ms",
        "CRANPOSE_UPDATE_STAGE_TELEMETRY_MS",
    ),
    (
        "debug.cranpose.dirty_diag",
        "CRANPOSE_RENDER_PHASE_DIRTY_DIAG",
    ),
    (
        "debug.cranpose.scene_update_diag",
        "CRANPOSE_SCENE_UPDATE_DIAG",
    ),
    ("debug.cranpose.backdrop_diag", "CRANPOSE_BACKDROP_DIAG"),
    (
        "debug.cranpose.no_underlay_bake",
        "CRANPOSE_DISABLE_UNDERLAY_BAKE",
    ),
    (
        "debug.cranpose.no_underlay_replay",
        "CRANPOSE_DISABLE_UNDERLAY_REPLAY",
    ),
    (
        "debug.cranpose.render_stage_ms",
        "CRANPOSE_WGPU_RENDER_STAGE_TELEMETRY_MS",
    ),
    (
        "debug.cranpose.frame_stage_ms",
        "CRANPOSE_FRAME_STAGE_TELEMETRY_MS",
    ),
    ("debug.cranpose.layer_diag", "CRANPOSE_LAYER_RENDER_DIAG"),
    ("debug.cranpose.segment_diag", "CRANPOSE_SEGMENT_DIAG"),
    (
        "debug.cranpose.text_prewarm_diag",
        "CRANPOSE_TEXT_PREWARM_DIAG",
    ),
    (
        "debug.cranpose.no_range_cache",
        "CRANPOSE_DISABLE_DIRECT_SCENE_RANGE_CACHE",
    ),
    (
        "debug.cranpose.no_prefix_snap",
        "CRANPOSE_DISABLE_PREFIX_SNAPSHOT",
    ),
    ("debug.cranpose.gpu_backend", "CRANPOSE_ANDROID_GPU_BACKEND"),
    ("debug.cranpose.async_haptics", "CRANPOSE_ASYNC_HAPTICS"),
    ("debug.cranpose.fill_diag", "CRANPOSE_FILL_DIAG"),
    ("debug.cranpose.static_span", "CRANPOSE_STATIC_SPAN"),
    ("debug.cranpose.segment_surface", "CRANPOSE_SEGMENT_SURFACE"),
    (
        "debug.cranpose.seg_surface_ratio",
        "CRANPOSE_SEGMENT_SURFACE_COST_RATIO",
    ),
    (
        "debug.cranpose.seg_surface_recolor",
        "CRANPOSE_SEGMENT_SURFACE_RECOLOR_RATE",
    ),
    (
        "debug.cranpose.seg_surface_scale",
        "CRANPOSE_SEGMENT_SURFACE_SCALE_EPS",
    ),
    (
        "debug.cranpose.segment_surface_waiver",
        "CRANPOSE_SEGMENT_SURFACE_WAIVER_RATIO",
    ),
    (
        "debug.cranpose.pipeline_disk_cache",
        "CRANPOSE_PIPELINE_DISK_CACHE",
    ),
    (
        "debug.cranpose.pipeline_prewarm",
        "CRANPOSE_PIPELINE_PREWARM",
    ),
    ("debug.cranpose.solid_trim", "CRANPOSE_SOLID_TRIM_VARYINGS"),
    (
        "debug.cranpose.uniform_gradient_stops",
        "CRANPOSE_UNIFORM_GRADIENT_STOPS",
    ),
    (
        "debug.cranpose.no_shader_specialization",
        "CRANPOSE_NO_SHADER_SPECIALIZATION",
    ),
    ("debug.cranpose.keep_box4", "CRANPOSE_KEEP_BOX4"),
    (
        "debug.cranpose.no_direct_surface",
        "CRANPOSE_NO_DIRECT_SURFACE",
    ),
    (
        "debug.cranpose.survive_gpu_errors",
        "CRANPOSE_SURVIVE_GPU_ERRORS",
    ),
    (
        "debug.cranpose.layer_cache_diag",
        "CRANPOSE_LAYER_CACHE_DIAG",
    ),
];

pub(crate) fn seed_env_from_system_properties() {
    for (property, variable) in PROPERTY_BACKED_ENV_VARS {
        if std::env::var_os(variable).is_some() {
            continue;
        }
        let Some(value) = system_property(property) else {
            continue;
        };
        // SAFETY: called from `android_main` before the frame loop, the render
        // thread or any worker pool exists, so no other thread can be reading
        // the environment concurrently.
        unsafe {
            std::env::set_var(variable, &value);
        }
        log::info!("[android-frame] {property} -> {variable}={value}");
    }
}

#[allow(
    clippy::useless_conversion,
    reason = "identity on 64-bit ABIs, required widening on armeabi-v7a and x86"
)]
pub(crate) fn monotonic_nanos() -> i64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a live, correctly sized `timespec`.
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now);
    }
    i64::from(now.tv_sec) * 1_000_000_000 + i64::from(now.tv_nsec)
}

static VSYNC_LAST_NS: AtomicI64 = AtomicI64::new(0);
static VSYNC_PERIOD_NS: AtomicI64 = AtomicI64::new(0);
static VSYNC_PROBE_RUNNING: AtomicBool = AtomicBool::new(false);
static DISPLAY_PERIOD_KNOWN: AtomicBool = AtomicBool::new(false);

const MIN_VSYNC_PERIOD_NS: i64 = 4_000_000;
const MAX_VSYNC_PERIOD_NS: i64 = 40_000_000;

pub(crate) fn start_vsync_probe_if_enabled() {
    if !property_flag("debug.cranpose.vsync_probe") {
        return;
    }
    if VSYNC_PROBE_RUNNING.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Err(error) = std::thread::Builder::new()
        .name("cranpose-vsync".to_owned())
        .spawn(run_vsync_probe)
    {
        VSYNC_PROBE_RUNNING.store(false, Ordering::Relaxed);
        log::warn!("[android-frame] vsync probe thread failed to start: {error}");
    }
}

fn run_vsync_probe() {
    // SAFETY: `ALooper_prepare` is being called on this freshly spawned thread,
    // which owns the looper it creates and is the only thread that polls it.
    let looper = unsafe { ndk_sys::ALooper_prepare(0) };
    if looper.is_null() {
        VSYNC_PROBE_RUNNING.store(false, Ordering::Relaxed);
        log::warn!("[android-frame] ALooper_prepare returned null; vsync probe not started");
        return;
    }
    // SAFETY: this thread owns a prepared looper, and the callback is a
    // `'static` function taking null user data.
    unsafe {
        let choreographer = ndk_sys::AChoreographer_getInstance();
        if choreographer.is_null() {
            VSYNC_PROBE_RUNNING.store(false, Ordering::Relaxed);
            log::warn!("[android-frame] AChoreographer_getInstance returned null on probe thread");
            return;
        }
        register_refresh_rate_callback(choreographer);
    }
    post_vsync_callback();
    log::info!("[android-frame] vsync probe started on its own looper");
    while VSYNC_PROBE_RUNNING.load(Ordering::Relaxed) {
        // SAFETY: this thread prepared the looper it is polling.
        let result = unsafe {
            ndk_sys::ALooper_pollOnce(
                -1,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if result == ndk_sys::ALOOPER_POLL_ERROR {
            log::warn!("[android-frame] vsync probe looper returned an error; stopping");
            VSYNC_PROBE_RUNNING.store(false, Ordering::Relaxed);
            return;
        }
    }
}

fn register_refresh_rate_callback(choreographer: *mut ndk_sys::AChoreographer) {
    type RegisterRefreshRateCallback = unsafe extern "C" fn(
        *mut ndk_sys::AChoreographer,
        ndk_sys::AChoreographer_refreshRateCallback,
        *mut c_void,
    );
    let symbol = unsafe {
        libc::dlsym(
            libc::RTLD_DEFAULT,
            c"AChoreographer_registerRefreshRateCallback".as_ptr(),
        )
    };
    if symbol.is_null() {
        log::info!(
            "[android-frame] AChoreographer_registerRefreshRateCallback needs API 30; \
             display period stays unknown on this device"
        );
        return;
    }
    // SAFETY: the symbol was just resolved from the loaded libandroid.so and
    // has the NDK-documented signature; the callback is a `'static` function
    // taking null user data.
    unsafe {
        let register: RegisterRefreshRateCallback = std::mem::transmute(symbol);
        register(choreographer, Some(on_refresh_rate), std::ptr::null_mut());
    }
}

unsafe extern "C" fn on_refresh_rate(vsync_period_ns: i64, _data: *mut c_void) {
    if (MIN_VSYNC_PERIOD_NS..=MAX_VSYNC_PERIOD_NS).contains(&vsync_period_ns) {
        VSYNC_PERIOD_NS.store(vsync_period_ns, Ordering::Relaxed);
        DISPLAY_PERIOD_KNOWN.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn vsync_period_ns() -> i64 {
    VSYNC_PERIOD_NS.load(Ordering::Relaxed)
}

fn vsync_offset_ns(now_ns: i64) -> Option<i64> {
    let last = VSYNC_LAST_NS.load(Ordering::Relaxed);
    let period = VSYNC_PERIOD_NS.load(Ordering::Relaxed);
    if last <= 0 || period <= 0 {
        return None;
    }
    let elapsed = now_ns - last;
    if elapsed < 0 {
        return None;
    }
    Some(elapsed % period)
}

unsafe extern "C" fn on_vsync(frame_time_ns: i64, _data: *mut c_void) {
    let previous = VSYNC_LAST_NS.swap(frame_time_ns, Ordering::Relaxed);
    if previous > 0 && !DISPLAY_PERIOD_KNOWN.load(Ordering::Relaxed) {
        let delta = frame_time_ns - previous;
        if (MIN_VSYNC_PERIOD_NS..=MAX_VSYNC_PERIOD_NS).contains(&delta) {
            let previous_period = VSYNC_PERIOD_NS.load(Ordering::Relaxed);
            if previous_period == 0 || delta < previous_period {
                VSYNC_PERIOD_NS.store(delta, Ordering::Relaxed);
            }
        }
    }
    post_vsync_callback();
}

fn post_vsync_callback() {
    // SAFETY: called on the looper-owning thread; the callback pointer is a
    // `'static` function and the user data is null.
    unsafe {
        let choreographer = ndk_sys::AChoreographer_getInstance();
        if choreographer.is_null() {
            VSYNC_PROBE_RUNNING.store(false, Ordering::Relaxed);
            log::warn!("[android-frame] AChoreographer_getInstance returned null");
            return;
        }
        ndk_sys::AChoreographer_postFrameCallback64(
            choreographer,
            Some(on_vsync),
            std::ptr::null_mut(),
        );
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct FrameTimings {
    pub(crate) iteration_start_ns: i64,
    pub(crate) after_poll_ns: i64,
    pub(crate) after_update_ns: i64,
    pub(crate) after_sync_ns: i64,
    pub(crate) after_acquire_ns: i64,
    pub(crate) after_render_ns: i64,
    pub(crate) after_present_ns: i64,
}

#[derive(Clone, Copy)]
struct Sample {
    period_us: i32,
    poll_us: i32,
    update_us: i32,
    sync_us: i32,
    acquire_us: i32,
    render_us: i32,
    present_us: i32,
    vsync_offset_us: i32,
}

pub(crate) struct AndroidFrameTelemetry {
    enabled: bool,
    window_frames: usize,
    samples: Vec<Sample>,
    last_present_ns: i64,
    idle_iterations: u32,
    window_start_ns: i64,
}

impl AndroidFrameTelemetry {
    pub(crate) fn from_system_properties() -> Self {
        let window_frames = system_property("debug.cranpose.frame_telemetry")
            .map(|value| match value.parse::<usize>() {
                Ok(0) => 0,
                Ok(1) => DEFAULT_WINDOW_FRAMES,
                Ok(frames) => frames,
                Err(_) => DEFAULT_WINDOW_FRAMES,
            })
            .unwrap_or(0);
        let enabled = window_frames > 0;
        if enabled {
            log::info!("[android-frame] telemetry enabled, window={window_frames} frames");
        }
        Self {
            enabled,
            window_frames,
            samples: Vec::with_capacity(window_frames),
            last_present_ns: 0,
            idle_iterations: 0,
            window_start_ns: 0,
        }
    }

    pub(crate) fn now(&self) -> i64 {
        if self.enabled { monotonic_nanos() } else { 0 }
    }

    pub(crate) fn note_idle_iteration(&mut self) {
        if self.enabled {
            self.idle_iterations = self.idle_iterations.saturating_add(1);
        }
    }

    pub(crate) fn record_frame(&mut self, timings: &FrameTimings) {
        if !self.enabled {
            return;
        }
        let period_us = if self.last_present_ns > 0 {
            us(timings.after_present_ns - self.last_present_ns)
        } else {
            0
        };
        if self.window_start_ns == 0 {
            self.window_start_ns = timings.iteration_start_ns;
        }
        self.last_present_ns = timings.after_present_ns;
        self.samples.push(Sample {
            period_us,
            poll_us: us(timings.after_poll_ns - timings.iteration_start_ns),
            update_us: us(timings.after_update_ns - timings.after_poll_ns),
            sync_us: us(timings.after_sync_ns - timings.after_update_ns),
            acquire_us: us(timings.after_acquire_ns - timings.after_sync_ns),
            render_us: us(timings.after_render_ns - timings.after_acquire_ns),
            present_us: us(timings.after_present_ns - timings.after_render_ns),
            vsync_offset_us: vsync_offset_ns(timings.iteration_start_ns)
                .map(us)
                .unwrap_or(-1),
        });
        if self.samples.len() >= self.window_frames {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let window_ns = self.last_present_ns - self.window_start_ns;
        let frames = self.samples.len();
        if frames < 2 || window_ns <= 0 {
            self.reset();
            return;
        }
        let fps = (frames - 1) as f64 * 1_000_000_000.0 / window_ns as f64;
        log::warn!(
            "[android-frame] n={frames} fps={fps:.2} idle_iters={} vsync_period_ms={:.3}",
            self.idle_iterations,
            vsync_period_ns() as f64 / 1e6,
        );
        self.report("period ", |sample| sample.period_us);
        self.report("poll   ", |sample| sample.poll_us);
        self.report("update ", |sample| sample.update_us);
        self.report("sync   ", |sample| sample.sync_us);
        self.report("acquire", |sample| sample.acquire_us);
        self.report("render ", |sample| sample.render_us);
        self.report("present", |sample| sample.present_us);
        self.report("cpu    ", |sample| {
            sample.update_us + sample.sync_us + sample.render_us + sample.present_us
        });
        self.report_vsync_phase();
        self.reset();
    }

    fn report_vsync_phase(&self) {
        let period_us = us(vsync_period_ns());
        if period_us <= 0
            || !self
                .samples
                .iter()
                .any(|sample| sample.vsync_offset_us >= 0)
        {
            return;
        }
        let late_threshold_us = period_us + period_us / 2;
        let (mut on_time, mut late) = (Vec::new(), Vec::new());
        for sample in &self.samples {
            if sample.vsync_offset_us < 0 || sample.period_us <= 0 {
                continue;
            }
            if sample.period_us > late_threshold_us {
                late.push(sample.vsync_offset_us);
            } else {
                on_time.push(sample.vsync_offset_us);
            }
        }
        on_time.sort_unstable();
        late.sort_unstable();
        log::warn!(
            "[android-frame]   vsync_phase on_time n={} p10={:.2} p50={:.2} p90={:.2} | late n={} p10={:.2} p50={:.2} p90={:.2} | period={:.2}ms",
            on_time.len(),
            ms(percentile(&on_time, 0.10)),
            ms(percentile(&on_time, 0.50)),
            ms(percentile(&on_time, 0.90)),
            late.len(),
            ms(percentile(&late, 0.10)),
            ms(percentile(&late, 0.50)),
            ms(percentile(&late, 0.90)),
            ms(period_us),
        );
    }

    fn report(&self, label: &str, value: impl Fn(&Sample) -> i32) {
        let mut values: Vec<i32> = self.samples.iter().map(&value).collect();
        values.sort_unstable();
        log::warn!(
            "[android-frame]   {label} p10={:.2} p50={:.2} p90={:.2} p99={:.2} max={:.2} mean={:.2}",
            ms(percentile(&values, 0.10)),
            ms(percentile(&values, 0.50)),
            ms(percentile(&values, 0.90)),
            ms(percentile(&values, 0.99)),
            ms(*values.last().unwrap_or(&0)),
            values.iter().map(|value| *value as f64).sum::<f64>()
                / values.len().max(1) as f64
                / 1000.0,
        );
    }

    fn reset(&mut self) {
        self.samples.clear();
        self.idle_iterations = 0;
        self.window_start_ns = 0;
    }
}

fn percentile(sorted: &[i32], fraction: f64) -> i32 {
    if sorted.is_empty() {
        return 0;
    }
    let index = ((sorted.len() - 1) as f64 * fraction).round() as usize;
    sorted[index.min(sorted.len() - 1)]
}

fn us(nanos: i64) -> i32 {
    (nanos / 1000).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn ms(micros: i32) -> f64 {
    micros as f64 / 1000.0
}
