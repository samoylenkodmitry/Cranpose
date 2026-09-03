//! Android runtime for Compose applications.
//!
//! This module provides the Android event loop implementation with proper
//! lifecycle management, input handling, and rendering coordination.

use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_app_shell::{
    AppShell, KeyEvent, PlatformFrameDriver, PointerSource, default_root_key,
};
use cranpose_platform_android::AndroidPlatform;
use cranpose_render_wgpu::{PresentOutcome, PublishOutcome, WgpuRenderer};
use cranpose_ui::{Point, Size};
use ndk::native_window::NativeWindow;

use crate::{
    android_host_window,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
    android_keyboard::{self, AndroidKeyTranslator, AndroidSoftKeyboard, is_system_key},
    android_overlay_window,
    android_surface::{AndroidSurfaceError, create_android_wgpu_surface},
    android_text_input::{self, AndroidImeEvent},
    app_launcher::{AndroidOverlayWindowOptions, AppSettings},
    wgpu_surface::{
        SurfaceFrame, current_surface_texture, present_initial_placeholder_frame,
        surface_present_required,
    },
};

struct GpuResources {
    surface: Option<wgpu::Surface<'static>>,
    native_window_ptr: Option<NonNull<c_void>>,
    adapter: Arc<wgpu::Adapter>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    config: wgpu::SurfaceConfiguration,
    _native_window: Option<NativeWindow>,
    surface_dirty: bool,
}

impl GpuResources {
    fn has_surface(&self) -> bool {
        self.native_window_ptr.is_some()
    }
}

enum PendingInput {
    PointerDown(f32, f32, Option<i64>, PointerSource),
    PointerUp(f32, f32, Option<i64>, PointerSource),
    PointerMove(f32, f32, Option<i64>, PointerSource),
    PointerCancel,
    Key(KeyEvent),
    SecondaryPointerDown(u64, f32, f32, Option<i64>),
    SecondaryPointerUp(u64, f32, f32, Option<i64>),
    SecondaryPointerMove(u64, f32, f32, Option<i64>),
    RotaryScroll(f32, u64),
}

fn android_event_time_ms(event_time_ns: i64) -> i64 {
    event_time_ns / 1_000_000
}

fn android_pointer_source(
    pointer: &android_activity::input::Pointer<'_>,
    event_source: android_activity::input::Source,
) -> PointerSource {
    use android_activity::input::{Source, ToolType};

    use crate::android_input::{AndroidSourceKind, AndroidToolKind, resolve_pointer_source};

    let tool = match pointer.tool_type() {
        ToolType::Finger => AndroidToolKind::Finger,
        ToolType::Mouse => AndroidToolKind::Mouse,
        ToolType::Stylus | ToolType::Eraser => AndroidToolKind::Stylus,
        _ => AndroidToolKind::Indeterminate,
    };
    let source = match event_source {
        Source::Touchscreen => AndroidSourceKind::Touchscreen,
        Source::Stylus | Source::BluetoothStylus => AndroidSourceKind::Stylus,
        Source::Mouse | Source::MouseRelative => AndroidSourceKind::Mouse,
        _ => AndroidSourceKind::Other,
    };
    resolve_pointer_source(tool, source)
}

fn push_pending_input_from_android_key_event(
    key_event: &android_activity::input::KeyEvent<'_>,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
) -> bool {
    if key_event.key_code() == android_activity::input::Keycode::Back {
        if cranpose_services::back_interception_enabled() {
            if key_event.action() == android_activity::input::KeyAction::Up {
                cranpose_services::push_back_request();
            }
            return true;
        }
        return false;
    }
    if is_system_key(key_event.key_code()) {
        return false;
    }
    let Some(event) = key_translator.translate(key_event) else {
        return false;
    };
    pending_inputs.push(PendingInput::Key(event));
    true
}

thread_local! {
    static ANDROID_PLATFORM_ENV: Rc<crate::platform_env::PlatformEnvironment> =
        crate::platform_env::PlatformEnvironment::new();
    static ANDROID_IME_DENSITY: Cell<f32> = const { Cell::new(1.0) };
}

pub(crate) fn android_platform_env() -> Rc<crate::platform_env::PlatformEnvironment> {
    ANDROID_PLATFORM_ENV.with(Rc::clone)
}

fn set_android_ime_density(density: f32) {
    ANDROID_IME_DENSITY.with(|cell| cell.set(density.max(f32::EPSILON)));
}

fn set_android_ime_bottom_px(bottom_px: i32) -> bool {
    let density = ANDROID_IME_DENSITY.with(|cell| cell.get());
    let bottom = (bottom_px.max(0) as f32) / density;
    let insets = cranpose_ui::EdgeInsets {
        bottom,
        ..cranpose_ui::EdgeInsets::default()
    };
    android_platform_env().set_ime_insets(insets)
}

fn system_theme_from_android(
    night: ndk::configuration::UiModeNight,
) -> cranpose_services::SystemTheme {
    match night {
        ndk::configuration::UiModeNight::Yes => cranpose_services::SystemTheme::Dark,
        _ => cranpose_services::SystemTheme::Light,
    }
}

const IME_ACTION_DONE: i32 = 6;

fn dispatch_android_ime_event(shell: &mut AppShell<WgpuRenderer>, event: AndroidImeEvent) {
    match event {
        AndroidImeEvent::CommitText { text, .. } => {
            let _ = shell.on_ime_preedit("", None);
            let _ = shell.on_paste(&text);
        }
        AndroidImeEvent::SetComposingText { text, cursor_bytes } => {
            let _ = shell.on_ime_preedit(&text, Some((cursor_bytes, cursor_bytes)));
        }
        AndroidImeEvent::SetComposingRegion {
            start_bytes,
            end_bytes,
        } => {
            let _ = shell.on_ime_set_composing_region(start_bytes, end_bytes);
        }
        AndroidImeEvent::SetSelection {
            start_bytes,
            end_bytes,
        } => {
            let _ = shell.on_ime_set_selection(start_bytes, end_bytes);
        }
        AndroidImeEvent::FinishComposing => {
            let _ = shell.on_ime_finish_composing();
        }
        AndroidImeEvent::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => {
            let _ = shell.on_ime_delete_surrounding(before_bytes, after_bytes);
        }
        AndroidImeEvent::Key {
            action,
            key_code,
            meta_state,
            unicode_char,
        } => {
            if let Some(event) =
                android_keyboard::ime_key_event(action, key_code, meta_state, unicode_char)
            {
                let _ = shell.on_key_event(&event);
            }
        }
        AndroidImeEvent::EditorAction { action } => {
            if action == IME_ACTION_DONE {
                let _ = shell.on_ime_finish_composing();
                shell.clear_text_field_focus();
            } else {
                for event_type in [
                    cranpose_app_shell::KeyEventType::KeyDown,
                    cranpose_app_shell::KeyEventType::KeyUp,
                ] {
                    let key = KeyEvent::new(
                        cranpose_app_shell::KeyCode::Enter,
                        "",
                        cranpose_app_shell::Modifiers::NONE,
                        event_type,
                    );
                    let _ = shell.on_key_event(&key);
                }
            }
        }
        AndroidImeEvent::ImeInsetsChanged { bottom_px } => {
            if set_android_ime_bottom_px(bottom_px) {
                shell.request_root_render();
            }
        }
    }
}

fn shell_pointer_id(android_pointer_id: i32, primary_pointer_id: i32) -> u64 {
    if android_pointer_id == primary_pointer_id {
        0
    } else {
        android_pointer_id as u64 + 1
    }
}

fn push_pending_inputs_from_android_event(
    event: &android_activity::input::InputEvent<'_>,
    android_platform: &AndroidPlatform,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
) -> bool {
    let motion_event = match event {
        android_activity::input::InputEvent::MotionEvent(motion_event) => motion_event,
        android_activity::input::InputEvent::KeyEvent(key_event) => {
            return push_pending_input_from_android_key_event(
                key_event,
                key_translator,
                pending_inputs,
            );
        }
        _ => return false,
    };

    let time_ms = Some(android_event_time_ms(motion_event.event_time()));
    let event_source = motion_event.source();
    let logical_of = |x: f32, y: f32| {
        let logical = android_platform.pointer_position(x as f64, y as f64);
        (logical.x, logical.y)
    };

    match motion_event.action() {
        android_activity::input::MotionAction::Down => {
            let pointer = motion_event.pointer_at_index(0);
            *primary_pointer_id = Some(pointer.pointer_id());
            let (x, y) = logical_of(pointer.x(), pointer.y());
            pending_inputs.push(PendingInput::PointerDown(
                x,
                y,
                time_ms,
                android_pointer_source(&pointer, event_source),
            ));
            true
        }
        android_activity::input::MotionAction::PointerDown => {
            let pointer = motion_event.pointer_at_index(motion_event.pointer_index());
            let Some(primary) = *primary_pointer_id else {
                return true;
            };
            let (x, y) = logical_of(pointer.x(), pointer.y());
            pending_inputs.push(PendingInput::SecondaryPointerDown(
                shell_pointer_id(pointer.pointer_id(), primary),
                x,
                y,
                time_ms,
            ));
            true
        }
        android_activity::input::MotionAction::Up => {
            let pointer = motion_event.pointer_at_index(motion_event.pointer_index());
            let (x, y) = logical_of(pointer.x(), pointer.y());
            match *primary_pointer_id {
                Some(primary) if pointer.pointer_id() != primary => {
                    pending_inputs.push(PendingInput::SecondaryPointerUp(
                        shell_pointer_id(pointer.pointer_id(), primary),
                        x,
                        y,
                        time_ms,
                    ));
                }
                _ => {}
            }
            pending_inputs.push(PendingInput::PointerUp(
                x,
                y,
                time_ms,
                android_pointer_source(&pointer, event_source),
            ));
            *primary_pointer_id = None;
            true
        }
        android_activity::input::MotionAction::PointerUp => {
            let pointer = motion_event.pointer_at_index(motion_event.pointer_index());
            let Some(primary) = *primary_pointer_id else {
                return true;
            };
            let (x, y) = logical_of(pointer.x(), pointer.y());
            if pointer.pointer_id() == primary {
                for other in motion_event.pointers() {
                    if other.pointer_id() == primary {
                        continue;
                    }
                    let (ox, oy) = logical_of(other.x(), other.y());
                    pending_inputs.push(PendingInput::SecondaryPointerUp(
                        shell_pointer_id(other.pointer_id(), primary),
                        ox,
                        oy,
                        time_ms,
                    ));
                }
                pending_inputs.push(PendingInput::PointerUp(
                    x,
                    y,
                    time_ms,
                    android_pointer_source(&pointer, event_source),
                ));
                *primary_pointer_id = None;
            } else {
                pending_inputs.push(PendingInput::SecondaryPointerUp(
                    shell_pointer_id(pointer.pointer_id(), primary),
                    x,
                    y,
                    time_ms,
                ));
            }
            true
        }
        android_activity::input::MotionAction::Move => {
            let primary = *primary_pointer_id;
            for pointer in motion_event.pointers() {
                let is_primary = primary == Some(pointer.pointer_id());
                let source = android_pointer_source(&pointer, event_source);
                if is_primary {
                    for historical in pointer.history() {
                        let (hx, hy) = logical_of(historical.x(), historical.y());
                        pending_inputs.push(PendingInput::PointerMove(
                            hx,
                            hy,
                            Some(android_event_time_ms(historical.event_time())),
                            source,
                        ));
                    }
                }
                let (x, y) = logical_of(pointer.x(), pointer.y());
                match (is_primary, primary) {
                    (true, _) => {
                        pending_inputs.push(PendingInput::PointerMove(x, y, time_ms, source));
                    }
                    (false, Some(primary)) => {
                        pending_inputs.push(PendingInput::SecondaryPointerMove(
                            shell_pointer_id(pointer.pointer_id(), primary),
                            x,
                            y,
                            time_ms,
                        ));
                    }
                    (false, None) => {}
                }
            }
            true
        }
        android_activity::input::MotionAction::Cancel => {
            pending_inputs.push(PendingInput::PointerCancel);
            *primary_pointer_id = None;
            true
        }
        android_activity::input::MotionAction::Scroll => {
            push_rotary_pending_input(motion_event, event_source, time_ms, pending_inputs)
        }
        _ => false,
    }
}

fn push_rotary_pending_input(
    motion_event: &android_activity::input::MotionEvent<'_>,
    event_source: android_activity::input::Source,
    time_ms: Option<i64>,
    pending_inputs: &mut Vec<PendingInput>,
) -> bool {
    use crate::android_input::is_rotary_encoder_source;

    if !is_rotary_encoder_source(u32::from(event_source)) {
        return false;
    }

    let Some(pointer) = motion_event.pointers().next() else {
        return true;
    };
    let detents = pointer.axis_value(android_activity::input::Axis::Scroll);
    if !detents.is_finite() || detents == 0.0 {
        return true;
    }

    pending_inputs.push(PendingInput::RotaryScroll(
        detents,
        time_ms.unwrap_or(0).max(0) as u64,
    ));
    true
}

fn drain_android_input_events(
    app: &android_activity::AndroidApp,
    android_platform: &AndroidPlatform,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
    primary_pointer_id: &mut Option<i32>,
) {
    let Ok(mut iter) = app.input_events_iter() else {
        return;
    };

    for _ in 0..MAX_ANDROID_INPUT_EVENTS_PER_POLL {
        let event_available = iter.next(|event| {
            if push_pending_inputs_from_android_event(
                event,
                android_platform,
                key_translator,
                pending_inputs,
                primary_pointer_id,
            ) {
                android_activity::InputStatus::Handled
            } else {
                android_activity::InputStatus::Unhandled
            }
        });
        if !event_available {
            break;
        }
    }
}

const MAX_ANDROID_INPUT_EVENTS_PER_POLL: usize = 10;

#[derive(Clone, Copy)]
struct PendingHostWindowSizeRequest {
    state: Option<android_host_window::AndroidHostWindowState>,
    requested: Size,
    requested_at: Instant,
}

struct AndroidFrameDriver {
    need_frame: Arc<AtomicBool>,
    app_waker: android_activity::AndroidAppWaker,
    loop_thread: std::thread::ThreadId,
    next_deadline: Cell<Option<web_time::Instant>>,
}

impl AndroidFrameDriver {
    fn new(app_waker: android_activity::AndroidAppWaker) -> Self {
        Self {
            need_frame: Arc::new(AtomicBool::new(false)),
            app_waker,
            loop_thread: std::thread::current().id(),
            next_deadline: Cell::new(None),
        }
    }

    fn raise_frame_request(
        need_frame: &AtomicBool,
        app_waker: &android_activity::AndroidAppWaker,
        loop_thread: std::thread::ThreadId,
    ) {
        need_frame.store(true, Ordering::Relaxed);
        if std::thread::current().id() != loop_thread {
            app_waker.wake();
        }
    }

    fn frame_waker(&self) -> impl Fn() + Send + Sync + 'static {
        let need_frame = self.need_frame.clone();
        let app_waker = self.app_waker.clone();
        let loop_thread = self.loop_thread;
        move || Self::raise_frame_request(&need_frame, &app_waker, loop_thread)
    }

    fn vsync_waker(&self) -> impl Fn() + Send + Sync + 'static {
        let need_frame = self.need_frame.clone();
        let app_waker = self.app_waker.clone();
        move || {
            need_frame.store(true, Ordering::Relaxed);
            app_waker.wake();
        }
    }

    fn frame_requested(&self) -> bool {
        self.need_frame.load(Ordering::Relaxed)
    }

    fn take_frame_request(&self) -> bool {
        self.need_frame.swap(false, Ordering::Relaxed)
    }

    fn deadline_timeout(&self) -> Option<Duration> {
        self.next_deadline.get().map(duration_until_frame_deadline)
    }
}

impl PlatformFrameDriver for AndroidFrameDriver {
    fn request_frame(&self) {
        Self::raise_frame_request(&self.need_frame, &self.app_waker, self.loop_thread);
    }

    fn request_wake_at(&self, deadline: web_time::Instant) {
        self.next_deadline.set(Some(deadline));
    }

    fn clear_wake(&self) {
        self.next_deadline.set(None);
    }
}

const OFFSCREEN_UPDATE_PERIOD: Duration = Duration::from_millis(16);

fn duration_until_frame_deadline(deadline: web_time::Instant) -> Duration {
    deadline
        .checked_duration_since(web_time::Instant::now())
        .unwrap_or(Duration::ZERO)
}

fn earliest_android_poll_timeout(
    first: Option<Duration>,
    second: Option<Duration>,
) -> Option<Duration> {
    match (first, second) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(duration), None) | (None, Some(duration)) => Some(duration),
        (None, None) => None,
    }
}

fn get_display_density(app: &android_activity::AndroidApp) -> f32 {
    let config = app.config();
    let density_dpi = config.density();

    density_dpi.map(|dpi| dpi as f32 / 160.0).unwrap_or(2.0)
}

fn surface_inset_px(app: &android_activity::AndroidApp) -> (f64, f64) {
    let Some(window) = app.native_window() else {
        return (0.0, 0.0);
    };
    let buffer_w = window.width() as f64;
    let buffer_h = window.height() as f64;
    let content = app.content_rect();
    let frame_w = content.right as f64;
    let frame_h = content.bottom as f64;
    if frame_w <= 0.0 || frame_h <= 0.0 || frame_w > buffer_w || frame_h > buffer_h {
        return (0.0, 0.0);
    }
    (
        ((buffer_w - frame_w) / 2.0).max(0.0),
        ((buffer_h - frame_h) / 2.0).max(0.0),
    )
}

fn update_android_platform_geometry(
    app: &android_activity::AndroidApp,
    android_platform: &mut AndroidPlatform,
) -> f32 {
    let density = get_display_density(app);
    android_platform.set_scale_factor(density as f64);

    let (offset_x, offset_y) = surface_inset_px(app);
    android_platform.set_input_surface_offset_px(offset_x, offset_y);

    density
}

fn update_android_shell_geometry(
    shell: &mut AppShell<WgpuRenderer>,
    density: f32,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
) -> Option<Size> {
    shell.renderer().set_root_scale(density);
    shell.set_density(density);
    shell.set_font_scale_curve(crate::android_font_scale::font_scale_curve());
    shell.set_rotary_scroll_factor(crate::android_input::android_rotary_scroll_factor(density));

    let (width, height) = shell.buffer_size();
    if width > 0 && height > 0 {
        let width_dp = width as f32 / density;
        let height_dp = height as f32 / density;
        shell.set_viewport(width_dp, height_dp);
        let actual = Size::new(width_dp, height_dp);
        cranpose_services::publish_host_surface_size(
            cranpose_services::host_surface::HostSurfaceSize {
                width: width_dp,
                height: height_dp,
                scale: density,
            },
        );
        android_host_window::sync_android_host_window_actual_size(host_window_registry, actual);
        Some(actual)
    } else {
        None
    }
}

fn apply_display_visible_region(
    app: &android_activity::AndroidApp,
    app_shell: &mut Option<AppShell<WgpuRenderer>>,
) {
    let Some(shell) = app_shell else {
        return;
    };
    let round = crate::android_display::display_is_round(app)
        && !android_activity_in_multi_window_mode(app);
    shell.renderer().set_display_visible_region(if round {
        cranpose_render_wgpu::DisplayVisibleRegion::InscribedCircle
    } else {
        cranpose_render_wgpu::DisplayVisibleRegion::Full
    });
}

fn apply_initialized_android_rendering(
    app: &android_activity::AndroidApp,
    app_shell: &mut Option<AppShell<WgpuRenderer>>,
    gpu_resources: &mut Option<GpuResources>,
    current_host_window_size: &mut Size,
    resources: GpuResources,
    actual_size: Option<Size>,
) {
    if let Some(actual_size) = actual_size {
        *current_host_window_size = actual_size;
    }
    *gpu_resources = Some(resources);
    apply_display_visible_region(app, app_shell);
}

fn render_once(
    resources: &mut GpuResources,
    shell: &mut AppShell<WgpuRenderer>,
    telemetry: &mut crate::android_frame_telemetry::AndroidFrameTelemetry,
    timings: &mut crate::android_frame_telemetry::FrameTimings,
) -> bool {
    let Some(surface) = resources.surface.as_ref() else {
        telemetry.note_idle_iteration();
        return false;
    };
    match current_surface_texture(surface, "android") {
        SurfaceFrame::Ready(frame) => {
            timings.after_acquire_ns = telemetry.now();
            let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(crate::surface_format::display_surface_view_format(
                    resources.config.format,
                )),
                ..Default::default()
            });
            let (width, height) = shell.buffer_size();

            if let Err(e) =
                shell
                    .renderer()
                    .render_surface_texture(&frame.texture, &view, width, height)
            {
                log::error!("Render error: {:?}", e);
            }

            timings.after_render_ns = telemetry.now();
            frame.present();
            timings.after_present_ns = telemetry.now();
            telemetry.record_frame(timings);
            resources.surface_dirty = false;
            false
        }
        SurfaceFrame::Reconfigure => {
            telemetry.note_idle_iteration();
            let (width, height) = shell.buffer_size();
            resources.config.width = width;
            resources.config.height = height;
            surface.configure(&resources.device, &resources.config);
            resources.surface_dirty = true;
            shell.mark_dirty();
            false
        }
        SurfaceFrame::Skip => {
            telemetry.note_idle_iteration();
            resources.surface_dirty = true;
            false
        }
    }
}

fn drain_present_returns_into_loop(
    shell: &mut AppShell<WgpuRenderer>,
    gpu_resources: &mut Option<GpuResources>,
    telemetry: &mut crate::android_frame_telemetry::AndroidFrameTelemetry,
    pending_present_timings: &mut Vec<(u64, crate::android_frame_telemetry::FrameTimings)>,
) -> Option<(web_time::Instant, web_time::Instant)> {
    let mut presented = false;
    let mut refused = false;
    let mut frame_started_at_ns = 0i64;
    let mut presented_at_ns = 0i64;
    shell
        .renderer()
        .drain_present_returns_with(&mut |frame_id, outcome, timings| {
            let producer_timings = pending_present_timings
                .iter()
                .position(|(id, _)| *id == frame_id)
                .map(|index| pending_present_timings.remove(index).1);
            match outcome {
                PresentOutcome::Presented => {
                    presented = true;
                    presented_at_ns = timings.after_present_ns;
                    if let Some(mut frame_timings) = producer_timings {
                        frame_started_at_ns = frame_timings.iteration_start_ns;
                        frame_timings.after_acquire_ns = timings.after_acquire_ns;
                        frame_timings.after_render_ns = timings.after_render_ns;
                        frame_timings.after_present_ns = timings.after_present_ns;
                        telemetry.record_frame(&frame_timings);
                    }
                }
                PresentOutcome::Cancelled(_) | PresentOutcome::NotRun => {
                    refused = true;
                    telemetry.note_idle_iteration();
                }
            }
        });
    if presented && let Some(resources) = gpu_resources.as_mut() {
        resources.surface_dirty = false;
    }
    if refused {
        if let Some(resources) = gpu_resources.as_mut() {
            resources.surface_dirty = true;
        }
        shell.mark_dirty();
    }
    presented.then(|| {
        let now = web_time::Instant::now();
        let now_ns = telemetry.now();
        let instant_at = |timestamp_ns: i64| {
            let age_ns = now_ns.saturating_sub(timestamp_ns);
            if timestamp_ns > 0 && age_ns > 0 {
                now.checked_sub(Duration::from_nanos(age_ns as u64))
                    .unwrap_or(now)
            } else {
                now
            }
        };
        let frame_finished_at = instant_at(presented_at_ns);
        let frame_started_at = if frame_started_at_ns > 0 {
            instant_at(frame_started_at_ns).min(frame_finished_at)
        } else {
            frame_finished_at
        };
        (frame_started_at, frame_finished_at)
    })
}

fn record_presented_frame(
    shell: Option<&mut AppShell<WgpuRenderer>>,
    frame_started_at: web_time::Instant,
    frame_finished_at: web_time::Instant,
) {
    if let Some(shell) = shell {
        shell.record_presented_frame(frame_started_at, frame_finished_at);
    }
}

struct AndroidGpuSetup {
    resources: GpuResources,
    surface: Option<wgpu::Surface<'static>>,
    renderer_needs_init: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AndroidGpuBackend {
    Vulkan,
    Gl,
}

impl AndroidGpuBackend {
    fn wgpu_backends(self) -> wgpu::Backends {
        match self {
            Self::Vulkan => wgpu::Backends::VULKAN,
            Self::Gl => wgpu::Backends::GL,
        }
    }

    fn preferred() -> Self {
        Self::preferred_from(
            std::env::var("CRANPOSE_ANDROID_GPU_BACKEND")
                .ok()
                .as_deref(),
        )
    }

    fn preferred_from(value: Option<&str>) -> Self {
        match value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("gl") | Some("gles") | Some("opengl") => Self::Gl,
            _ => Self::Vulkan,
        }
    }
}

#[cfg(test)]
mod gpu_backend_tests {
    use super::AndroidGpuBackend;

    #[test]
    fn vulkan_is_what_an_app_gets_without_asking() {
        assert_eq!(
            AndroidGpuBackend::preferred_from(None),
            AndroidGpuBackend::Vulkan
        );
    }

    #[test]
    fn the_switch_names_gl_however_it_is_spelled() {
        for spelling in ["gl", "GL", " gles ", "OpenGL"] {
            assert_eq!(
                AndroidGpuBackend::preferred_from(Some(spelling)),
                AndroidGpuBackend::Gl,
                "{spelling:?} should select the GL backend"
            );
        }
    }

    #[test]
    fn a_mistyped_value_still_starts_the_app() {
        assert_eq!(
            AndroidGpuBackend::preferred_from(Some("vulkan2")),
            AndroidGpuBackend::Vulkan
        );
    }
}

struct AndroidWgpuContext {
    instance: wgpu::Instance,
    backend: AndroidGpuBackend,
}

impl AndroidWgpuContext {
    fn new(backend: AndroidGpuBackend) -> Self {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backend.wgpu_backends();
        descriptor.flags = wgpu::InstanceFlags::empty();
        Self {
            instance: wgpu::Instance::new(descriptor),
            backend,
        }
    }
}

fn init_gpu_threaded_for_android(
    renderer: &mut WgpuRenderer,
    resources: &GpuResources,
    frame_driver: &AndroidFrameDriver,
) -> Result<(), AndroidSurfaceError> {
    renderer
        .init_gpu_threaded(
            resources.device.clone(),
            resources.queue.clone(),
            resources.surface_format,
            resources.backend,
            resources.adapter.get_downlevel_capabilities().flags,
            Arc::new(frame_driver.frame_waker()),
            Some(Arc::new(crate::android_frame_telemetry::monotonic_nanos)),
        )
        .map_err(|error| AndroidSurfaceError::PresentRuntime(format!("{error:?}")))
}

fn drop_android_surface(
    gpu_resources: &mut Option<GpuResources>,
    app_shell: &mut Option<AppShell<WgpuRenderer>>,
    present_thread: bool,
) {
    match (gpu_resources.as_mut(), app_shell.as_mut()) {
        (Some(resources), Some(shell)) => {
            if present_thread {
                let renderer = shell.renderer();
                renderer.note_surface_reconfigured();
                renderer.present_drop_surface();
            }
            resources.surface = None;
            resources.native_window_ptr = None;
            resources._native_window = None;
            resources.surface_dirty = true;
        }
        _ => {
            *gpu_resources = None;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn initialize_android_rendering<F>(
    instance: &wgpu::Instance,
    existing_resources: Option<GpuResources>,
    app_shell: &mut Option<AppShell<WgpuRenderer>>,
    content: &Rc<RefCell<F>>,
    settings: &AppSettings,
    frame_driver: &AndroidFrameDriver,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
    density: f32,
    present_thread: bool,
) -> Result<(GpuResources, Option<Size>), AndroidSurfaceError>
where
    F: FnMut() + 'static,
{
    let mut setup = create_android_gpu_resources(
        instance,
        existing_resources,
        native_window_ptr,
        native_window_owner,
        width,
        height,
    )?;

    if app_shell.is_none() {
        let fonts = settings.resolve_font_set();
        let mut renderer = WgpuRenderer::with_font_set(fonts);
        if present_thread {
            init_gpu_threaded_for_android(&mut renderer, &setup.resources, frame_driver)?;
        } else {
            renderer.init_gpu(
                setup.resources.device.clone(),
                setup.resources.queue.clone(),
                setup.resources.surface_format,
                setup.resources.backend,
                setup.resources.adapter.get_downlevel_capabilities().flags,
            );
        }

        let content_clone = content.clone();
        let density = density.max(f32::EPSILON);
        let platform_env = android_platform_env();
        let mut shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            move || {
                platform_env.compose_root(|| content_clone.borrow_mut()());
            },
            (width, height),
            (width as f32 / density, height as f32 / density),
            density,
        );
        shell.set_semantics_enabled(true);

        *app_shell = Some(shell);

        if let Some(shell) = app_shell {
            shell.set_frame_waker(frame_driver.frame_waker());
        }

        log::info!("App shell created");
    } else if setup.renderer_needs_init {
        if let Some(shell) = app_shell {
            if present_thread {
                init_gpu_threaded_for_android(shell.renderer(), &setup.resources, frame_driver)?;
            } else {
                shell.renderer().init_gpu(
                    setup.resources.device.clone(),
                    setup.resources.queue.clone(),
                    setup.resources.surface_format,
                    setup.resources.backend,
                    setup.resources.adapter.get_downlevel_capabilities().flags,
                );
            }
            log::info!("Renderer reinitialized with new Android GPU pipeline resources");
        }
    } else {
        log::debug!("Reused Android WGPU device and renderer resources for surface update");
    }

    if present_thread {
        if let Some(shell) = app_shell.as_mut() {
            let renderer = shell.renderer();
            renderer.note_surface_reconfigured();
            match setup.surface.take() {
                Some(surface) => {
                    renderer.present_replace_surface(surface, setup.resources.config.clone());
                }
                None => {
                    renderer.present_reconfigure(setup.resources.config.clone());
                }
            }
            setup.resources.surface_dirty = true;
            shell.mark_dirty();
        }
    } else {
        match setup.surface.take() {
            Some(surface) => {
                surface.configure(&setup.resources.device, &setup.resources.config);
                present_initial_placeholder_frame(
                    &surface,
                    &setup.resources.device,
                    &setup.resources.queue,
                    setup.resources.surface_format,
                    "android initial present",
                );
                setup.resources.surface = Some(surface);
            }
            None => {
                if let Some(surface) = setup.resources.surface.as_ref() {
                    surface.configure(&setup.resources.device, &setup.resources.config);
                }
            }
        }

        if let Some(shell) = app_shell.as_mut() {
            setup.resources.surface_dirty = true;
            shell.mark_dirty();
        }
    }

    if let Some(shell) = app_shell {
        shell.renderer().set_root_scale(density);
        shell.set_density(density);
        set_android_ime_density(density);
    }

    let actual_size = app_shell.as_mut().and_then(|shell| {
        shell.set_buffer_size(width, height);
        update_android_shell_geometry(shell, density, host_window_registry)
    });

    Ok((setup.resources, actual_size))
}

#[allow(clippy::too_many_arguments)]
fn initialize_android_rendering_with_backend_fallback<F>(
    wgpu_context: &mut AndroidWgpuContext,
    existing_resources: Option<GpuResources>,
    app_shell: &mut Option<AppShell<WgpuRenderer>>,
    content: &Rc<RefCell<F>>,
    settings: &AppSettings,
    frame_driver: &AndroidFrameDriver,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
    density: f32,
    present_thread: bool,
) -> Result<(GpuResources, Option<Size>), AndroidSurfaceError>
where
    F: FnMut() + 'static,
{
    if existing_resources.is_some() || wgpu_context.backend == AndroidGpuBackend::Gl {
        return initialize_android_rendering(
            &wgpu_context.instance,
            existing_resources,
            app_shell,
            content,
            settings,
            frame_driver,
            host_window_registry,
            native_window_ptr,
            native_window_owner,
            width,
            height,
            density,
            present_thread,
        );
    }

    match initialize_android_rendering(
        &wgpu_context.instance,
        None,
        app_shell,
        content,
        settings,
        frame_driver,
        host_window_registry,
        native_window_ptr,
        native_window_owner.clone(),
        width,
        height,
        density,
        present_thread,
    ) {
        Err(AndroidSurfaceError::RequestAdapter(error)) => {
            log::warn!("No compatible Vulkan adapter ({error}); retrying with a fresh GL instance");
            *wgpu_context = AndroidWgpuContext::new(AndroidGpuBackend::Gl);
            initialize_android_rendering(
                &wgpu_context.instance,
                None,
                app_shell,
                content,
                settings,
                frame_driver,
                host_window_registry,
                native_window_ptr,
                native_window_owner,
                width,
                height,
                density,
                present_thread,
            )
        }
        result => result,
    }
}

fn create_android_gpu_resources(
    instance: &wgpu::Instance,
    existing_resources: Option<GpuResources>,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
) -> Result<AndroidGpuSetup, AndroidSurfaceError> {
    if let Some(mut resources) = existing_resources {
        if resources.native_window_ptr == Some(native_window_ptr) {
            resources.config.width = width;
            resources.config.height = height;
            if let Some(native_window_owner) = native_window_owner {
                resources._native_window = Some(native_window_owner);
            }
            return Ok(AndroidGpuSetup {
                resources,
                surface: None,
                renderer_needs_init: false,
            });
        }

        return create_android_gpu_resources_for_existing_device(
            instance,
            &resources,
            native_window_ptr,
            native_window_owner,
            width,
            height,
        );
    }

    let surface = create_android_wgpu_surface(instance, native_window_ptr)?;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))?;

    let adapter_info = adapter.get_info();
    log::info!("Found adapter: {:?}", adapter_info.backend);
    let adapter = Arc::new(adapter);

    if cranpose_render_wgpu::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_FILE").is_none()
        && let Some(cache_dir) =
            cranpose_render_wgpu::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_DIR")
    {
        let mut driver_hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in adapter_info
            .driver
            .bytes()
            .chain(adapter_info.driver_info.bytes())
        {
            driver_hash ^= u64::from(byte);
            driver_hash = driver_hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let file_name = format!(
            "pipeline_cache_v1_{:04x}_{:04x}_{driver_hash:016x}.bin",
            adapter_info.vendor, adapter_info.device,
        );
        let file_path = std::path::Path::new(&cache_dir).join(file_name);
        cranpose_render_wgpu::set_debug_toggle_os(
            "CRANPOSE_PIPELINE_CACHE_FILE",
            Some(file_path.as_os_str()),
        );
    }

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Android Device"),
        required_features: cranpose_render_wgpu::optional_device_features(&adapter),
        required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: crate::gpu_limits::mobile_memory_hints(),
        trace: wgpu::Trace::Off,
    }))?;

    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let config = create_android_surface_config(&surface, &adapter, width, height)?;

    Ok(AndroidGpuSetup {
        resources: GpuResources {
            surface: None,
            native_window_ptr: Some(native_window_ptr),
            adapter,
            device,
            queue,
            surface_format: crate::surface_format::display_surface_view_format(config.format),
            backend: adapter_info.backend,
            config,
            _native_window: native_window_owner,
            surface_dirty: true,
        },
        surface: Some(surface),
        renderer_needs_init: true,
    })
}

fn create_android_gpu_resources_for_existing_device(
    instance: &wgpu::Instance,
    existing: &GpuResources,
    native_window_ptr: NonNull<c_void>,
    native_window_owner: Option<NativeWindow>,
    width: u32,
    height: u32,
) -> Result<AndroidGpuSetup, AndroidSurfaceError> {
    let surface = create_android_wgpu_surface(instance, native_window_ptr)?;
    let config = create_android_surface_config(&surface, &existing.adapter, width, height)?;
    let renderer_needs_init = crate::surface_format::display_surface_view_format(config.format)
        != existing.surface_format;

    Ok(AndroidGpuSetup {
        resources: GpuResources {
            surface: None,
            native_window_ptr: Some(native_window_ptr),
            adapter: existing.adapter.clone(),
            device: existing.device.clone(),
            queue: existing.queue.clone(),
            surface_format: crate::surface_format::display_surface_view_format(config.format),
            backend: existing.backend,
            config,
            _native_window: native_window_owner,
            surface_dirty: true,
        },
        surface: Some(surface),
        renderer_needs_init,
    })
}

fn android_frame_latency(requested: Option<&str>) -> u32 {
    requested
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|latency| (1..=3).contains(latency))
        .unwrap_or(3)
}

#[cfg(test)]
mod frame_latency_tests {
    use super::android_frame_latency;

    #[test]
    fn the_platform_depth_is_what_an_app_gets_without_asking() {
        assert_eq!(android_frame_latency(None), 3);
    }

    #[test]
    fn the_switch_takes_any_depth_the_surface_can_hold() {
        for (spelling, depth) in [("1", 1), (" 2 ", 2), ("3", 3)] {
            assert_eq!(android_frame_latency(Some(spelling)), depth);
        }
    }

    #[test]
    fn a_depth_the_surface_cannot_hold_still_starts_the_app() {
        for mistyped in ["0", "4", "two", ""] {
            assert_eq!(android_frame_latency(Some(mistyped)), 3, "{mistyped:?}");
        }
    }
}

fn resolve_present_thread(requested: Option<&str>, available_cores: usize) -> bool {
    match requested.map(str::trim) {
        Some("1") | Some("true") | Some("on") => true,
        Some("0") | Some("false") | Some("off") => false,
        _ => available_cores >= 6,
    }
}

#[cfg(test)]
mod present_thread_tests {
    use super::resolve_present_thread;

    #[test]
    fn a_phone_with_idle_cores_overlaps_by_default() {
        assert!(resolve_present_thread(None, 8));
        assert!(resolve_present_thread(None, 6));
    }

    #[test]
    fn a_small_saturated_part_stays_synchronous() {
        assert!(!resolve_present_thread(None, 4));
        assert!(!resolve_present_thread(None, 1));
    }

    #[test]
    fn the_override_wins_in_both_directions_on_any_core_count() {
        assert!(resolve_present_thread(Some("1"), 4));
        assert!(resolve_present_thread(Some("on"), 1));
        assert!(!resolve_present_thread(Some("0"), 8));
        assert!(!resolve_present_thread(Some(" off "), 8));
    }

    #[test]
    fn an_unparsable_override_falls_back_to_the_core_default() {
        assert!(resolve_present_thread(Some("maybe"), 8));
        assert!(!resolve_present_thread(Some(""), 4));
    }
}

fn create_android_surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, AndroidSurfaceError> {
    let surface_caps = surface.get_capabilities(adapter);
    let surface_format =
        crate::surface_format::select_display_surface_format(&surface_caps.formats)
            .ok_or(AndroidSurfaceError::NoSurfaceFormat)?;
    let alpha_mode = surface_caps
        .alpha_modes
        .first()
        .copied()
        .ok_or(AndroidSurfaceError::NoAlphaMode)?;
    let present_mode = crate::present_mode::select_android_present_mode(&surface_caps);
    let desired_maximum_frame_latency = android_frame_latency(
        crate::android_frame_telemetry::system_property("debug.cranpose.frame_latency").as_deref(),
    );
    let usage = cranpose_render_wgpu::presentable_root_usages(surface_caps.usages);
    log::info!(
        "Android surface: formats {:?}, usages {:?} (configured {usage:?}), supported present modes {:?}, selected {:?}, desired_maximum_frame_latency {}",
        surface_caps.formats,
        surface_caps.usages,
        surface_caps.present_modes,
        present_mode,
        desired_maximum_frame_latency,
    );
    Ok(wgpu::SurfaceConfiguration {
        usage,
        format: surface_format,
        width,
        height,
        present_mode,
        alpha_mode,
        view_formats: crate::surface_format::display_surface_view_formats(surface_format),
        desired_maximum_frame_latency,
    })
}

fn dispatch_android_surface_size_request(
    app: &android_activity::AndroidApp,
    requested: Size,
    position: Point,
    density: f32,
    overlay_options: Option<AndroidOverlayWindowOptions>,
) -> Result<(), String> {
    let requested =
        android_host_window::validate_logical_size(requested).map_err(|error| error.to_string())?;
    if overlay_options.is_some() {
        return android_overlay_window::update_android_overlay_window_bounds(
            app, position, requested, density,
        );
    }

    let (width_px, height_px) =
        android_host_window::logical_to_physical_window_size(requested, density);
    set_android_window_layout_px(app, width_px, height_px)
}

fn dispatch_registered_android_surface_size_request(
    app: &android_activity::AndroidApp,
    host_window_registry: &android_host_window::AndroidHostWindowRegistry,
    density: f32,
    overlay_options: Option<AndroidOverlayWindowOptions>,
    last_dispatched: &mut Option<(android_host_window::AndroidHostWindowState, u64, u64)>,
    pending_confirmation: &mut Option<PendingHostWindowSizeRequest>,
) {
    let Some(request) =
        android_host_window::latest_android_host_window_request(host_window_registry)
    else {
        return;
    };
    let dispatch_key = (
        request.state,
        request.size_revision,
        if overlay_options.is_some() {
            request.position_revision
        } else {
            0
        },
    );
    if *last_dispatched == Some(dispatch_key) {
        return;
    }

    let position = overlay_options
        .filter(|_| request.position_revision == 0)
        .map(|options| Point::new(options.x as f32, options.y as f32))
        .unwrap_or(request.position);
    request.state.mark_pending(request.size);
    match dispatch_android_surface_size_request(
        app,
        request.size,
        position,
        density,
        overlay_options,
    ) {
        Ok(()) => {
            *last_dispatched = Some(dispatch_key);
            *pending_confirmation = Some(PendingHostWindowSizeRequest {
                state: Some(request.state),
                requested: request.size,
                requested_at: Instant::now(),
            });
            let target = if overlay_options.is_some() {
                "Android overlay surface"
            } else {
                "Android host-window"
            };
            if overlay_options.is_some() {
                log::info!(
                    "Requested {target} bounds {:.1}x{:.1} dp at {:.1},{:.1} dp",
                    request.size.width,
                    request.size.height,
                    position.x,
                    position.y
                );
            } else {
                log::info!(
                    "Requested {target} size {:.1}x{:.1} dp",
                    request.size.width,
                    request.size.height
                );
            }
        }
        Err(message) => {
            *last_dispatched = Some(dispatch_key);
            request.state.mark_dispatch_failed(request.size, message);
        }
    }
}

fn confirm_android_host_window_request(
    pending_confirmation: &mut Option<PendingHostWindowSizeRequest>,
    actual_size: Size,
) {
    let Some(pending) = *pending_confirmation else {
        return;
    };

    if android_host_window::sizes_match(pending.requested, actual_size) {
        if let Some(state) = pending.state {
            state.mark_applied(pending.requested, actual_size);
        }
        *pending_confirmation = None;
        return;
    }

    if pending.requested_at.elapsed() >= android_host_window::HOST_WINDOW_CONFIRMATION_TIMEOUT {
        if let Some(state) = pending.state {
            state.mark_unsupported(pending.requested, actual_size);
        }
        log::info!(
            "Android surface size request {:.1}x{:.1} dp was not honored; actual is {:.1}x{:.1} dp",
            pending.requested.width,
            pending.requested.height,
            actual_size.width,
            actual_size.height
        );
        *pending_confirmation = None;
    }
}

fn android_activity_in_multi_window_mode(app: &android_activity::AndroidApp) -> bool {
    use jni::{jni_sig, jni_str};

    with_android_activity_env(app, |env, activity| {
        env.call_method(
            &activity,
            jni_str!("isInMultiWindowMode"),
            jni_sig!("()Z"),
            &[],
        )
        .and_then(|value| value.z())
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to query Android multi-window mode: {error}")
        })
    })
    .unwrap_or_else(|error| {
        log::warn!("{error}");
        false
    })
}

fn set_android_window_layout_px(
    app: &android_activity::AndroidApp,
    width_px: i32,
    height_px: i32,
) -> Result<(), String> {
    use jni::{jni_sig, jni_str, objects::JValue};

    with_android_activity_env(app, |env, activity| {
        let class = android_overlay_window::find_android_overlay_class(env, &activity)?;
        let result = env
            .call_static_method(
                class,
                jni_str!("setActivityWindowLayout"),
                jni_sig!("(Landroid/app/Activity;II)I"),
                &[
                    JValue::Object(&activity),
                    JValue::Int(width_px),
                    JValue::Int(height_px),
                ],
            )
            .and_then(|value| value.i())
            .map_err(|error| {
                clear_pending_android_jni_exception(env);
                format!("failed to request Android window layout: {error}")
            })?;

        match result {
            0 => Ok(()),
            code => Err(format!(
                "Android window layout request failed with code {code}"
            )),
        }
    })
}

const DEFAULT_LOG_TAG: &str = "Cranpose";

/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub fn run(
    app: android_activity::AndroidApp,
    settings: AppSettings,
    content: impl FnMut() + 'static,
) {
    use android_activity::{MainEvent, PollEvent};

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag(settings.log_tag.as_deref().unwrap_or(DEFAULT_LOG_TAG))
            .with_filter(
                android_logger::FilterBuilder::new()
                    .filter_level(log::LevelFilter::Info)
                    .filter_module("wgpu_core", log::LevelFilter::Warn)
                    .filter_module("wgpu_hal", log::LevelFilter::Warn)
                    .filter_module("naga", log::LevelFilter::Warn)
                    .filter_module("android_activity::activity_impl", log::LevelFilter::Off)
                    .build(),
            ),
    );

    crate::android_frame_telemetry::seed_env_from_system_properties();

    let machine_parallelism = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1);

    cranpose_render_wgpu::pin_current_thread_to_fast_cores("producer");

    if cranpose_render_wgpu::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_DIR").is_none()
        && let Some(data_path) = app.internal_data_path()
    {
        let cache_dir = data_path.join("cranpose_gpu");
        cranpose_render_wgpu::set_debug_toggle_os(
            "CRANPOSE_PIPELINE_CACHE_DIR",
            Some(cache_dir.as_os_str()),
        );
    }

    let present_thread = resolve_present_thread(
        std::env::var("CRANPOSE_PRESENT_THREAD").ok().as_deref(),
        machine_parallelism,
    );
    if present_thread {
        log::info!("[present-runtime] threaded present enabled");
    }

    crate::android_file_picker::register(app.clone());
    crate::android_writable_folder::register(app.clone());
    crate::android_services::register(app.clone());
    crate::android_host::install(app.clone());
    crate::process_info::install();

    android_platform_env()
        .set_system_theme(system_theme_from_android(app.config().ui_mode_night()));

    crate::android_font_scale::refresh_font_scale(&app);

    crate::android_app_info::install_app_info(&app);

    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(crate::android_panic_hook::chained_panic_hook(
        |panic_info| {
            let location = panic_info
                .location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown location".to_string());
            let message = panic_info
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| {
                    panic_info
                        .payload()
                        .downcast_ref::<String>()
                        .map(String::as_str)
                })
                .unwrap_or("Box<dyn Any>");
            let backtrace = std::backtrace::Backtrace::force_capture();
            log::error!("PANIC at {location}: {message}\n{backtrace}");
        },
        previous_hook,
    ));

    let content = std::rc::Rc::new(std::cell::RefCell::new(content));

    let mut app_shell: Option<AppShell<WgpuRenderer>> = None;
    let mut accessibility_elements = Vec::new();
    let mut accessibility_revision = None;
    let mut accessibility_policy =
        crate::accessibility_publish_policy::AccessibilityPublishPolicy::new();

    log::info!("Starting Compose Android Application");

    let android_frame_driver = AndroidFrameDriver::new(app.create_waker());
    let mut frame_rate_voter = crate::android_frame_rate::FrameRateVoter::default();
    let mut perf_hint: Option<Option<crate::android_perf_hint::PerfHintSession>> = None;
    const FRAME_RATE_BOOST_HOLD_OFF: Duration = Duration::from_secs(3);
    let mut last_interaction: Option<Instant> = None;
    crate::android_vsync::install_waker(android_frame_driver.vsync_waker());
    crate::android_accessibility::set_waker(app.create_waker());
    let host_window_registry = Rc::new(android_host_window::AndroidHostWindowRegistry::default());
    let overlay_event_queue = Arc::new(android_overlay_window::AndroidOverlayEventQueue::default());

    let ime_event_queue = Arc::new(android_text_input::AndroidImeEventQueue::new());
    ime_event_queue.set_waker(app.create_waker());
    let ime_session =
        android_keyboard::AndroidImeSession::new(app.clone(), Arc::clone(&ime_event_queue));

    let should_exit = Arc::new(AtomicBool::new(false));

    let mut wgpu_context = AndroidWgpuContext::new(AndroidGpuBackend::preferred());

    let mut android_platform = AndroidPlatform::new();
    let mut current_host_window_size = Size::ZERO;
    let mut initial_host_window_size = settings.initial_size_explicit.then(|| {
        Size::new(
            settings.initial_width as f32,
            settings.initial_height as f32,
        )
    });
    let mut last_dispatched_host_window_request =
        None::<(android_host_window::AndroidHostWindowState, u64, u64)>;
    let mut pending_host_window_confirmation = None::<PendingHostWindowSizeRequest>;
    let mut overlay_window_options = settings.android_overlay_window;
    let mut overlay_window_requested = false;

    let mut gpu_resources: Option<GpuResources> = None;

    let mut pending_inputs: Vec<PendingInput> = Vec::new();
    let mut primary_pointer_id: Option<i32> = None;

    let mut key_translator = AndroidKeyTranslator::new(app.clone());
    let mut soft_keyboard_installed = false;
    let mut next_offscreen_update: Option<web_time::Instant> = None;

    let mut frame_telemetry =
        crate::android_frame_telemetry::AndroidFrameTelemetry::from_system_properties();
    crate::android_frame_telemetry::start_vsync_probe_if_enabled();

    const MAX_EXIT_ATTEMPTS: u32 = 3;
    let mut exit_attempts = 0u32;

    let catchup_pacing = !matches!(std::env::var("CRANPOSE_CATCHUP_PACING").as_deref(), Ok("0"));
    if catchup_pacing {
        log::info!("[pacing] catch-up pacing enabled");
    }
    let mut last_present_at: Option<web_time::Instant> = None;
    let mut behind_deadline = false;
    let mut catchup_coasts = 0u32;

    let mut pending_present_timings =
        Vec::<(u64, crate::android_frame_telemetry::FrameTimings)>::with_capacity(2);

    loop {
        let mut frame_timings = crate::android_frame_telemetry::FrameTimings {
            iteration_start_ns: frame_telemetry.now(),
            ..Default::default()
        };

        let drained_present_interval = match app_shell.as_mut() {
            Some(shell) => drain_present_returns_into_loop(
                shell,
                &mut gpu_resources,
                &mut frame_telemetry,
                &mut pending_present_timings,
            ),
            None => None,
        };

        let pending_confirmation_timeout = pending_host_window_confirmation.map(|pending| {
            android_host_window::HOST_WINDOW_CONFIRMATION_TIMEOUT
                .checked_sub(pending.requested_at.elapsed())
                .unwrap_or(Duration::ZERO)
        });

        let no_surface = gpu_resources
            .as_ref()
            .is_none_or(|resources| !resources.has_surface());
        match app_shell.as_ref() {
            Some(shell) if !no_surface => {
                shell.schedule_platform_frame(&android_frame_driver);
            }
            _ => android_frame_driver.clear_wake(),
        }
        let frame_deadline_timeout = android_frame_driver.deadline_timeout();
        let offscreen = no_surface && cranpose_services::background_active();
        if !offscreen {
            next_offscreen_update = None;
        }
        let offscreen_pending_ui = offscreen
            && app_shell
                .as_ref()
                .is_some_and(|shell| shell.has_pending_ui());
        let offscreen_timeout = next_offscreen_update.map(duration_until_frame_deadline);
        let accessibility_flush_timeout = accessibility_policy
            .wake_deadline()
            .map(|deadline| deadline.saturating_duration_since(std::time::Instant::now()));
        let idle_timeout = match no_surface {
            true => earliest_android_poll_timeout(pending_confirmation_timeout, offscreen_timeout),
            false => earliest_android_poll_timeout(
                earliest_android_poll_timeout(pending_confirmation_timeout, frame_deadline_timeout),
                accessibility_flush_timeout,
            ),
        };

        if let Some(shell) = app_shell.as_ref() {
            let preference = shell.frame_rate_preference();
            let producing_frames = android_frame_driver.frame_requested();
            let interacting =
                last_interaction.is_some_and(|at| at.elapsed() < FRAME_RATE_BOOST_HOLD_OFF);
            let panel_max = match preference {
                cranpose_app_shell::FrameRatePreference::Auto if interacting => {
                    crate::android_frame_rate::panel_max_refresh_rate(&app)
                }
                _ => None,
            };
            frame_rate_voter.apply(
                &app,
                preference.desired_rate_hz(producing_frames, interacting, panel_max),
            );
        }

        let poll_duration = if !pending_inputs.is_empty() {
            Some(Duration::ZERO)
        } else if no_surface {
            match offscreen_pending_ui {
                true => Some(Duration::ZERO),
                false => idle_timeout,
            }
        } else if android_frame_driver.frame_requested() {
            if behind_deadline {
                Some(Duration::ZERO)
            } else if crate::android_vsync::request_wake_at_next_vsync() {
                idle_timeout
            } else {
                Some(Duration::ZERO)
            }
        } else {
            idle_timeout
        };

        app.poll_events(poll_duration, |event| {
            if let PollEvent::Main(main_event) = event {
                match main_event {
                    MainEvent::InitWindow { .. } => {
                        log::info!("Window initialized, setting up rendering");

                        if let Some(options) = overlay_window_options {
                            let density =
                                update_android_platform_geometry(&app, &mut android_platform);
                            android_platform.set_input_surface_offset_px(0.0, 0.0);
                            if !overlay_window_requested {
                                match android_overlay_window::show_android_overlay_window(
                                    &app,
                                    options,
                                    density,
                                    &overlay_event_queue,
                                ) {
                                    Ok(()) => {
                                        overlay_window_requested = true;
                                        log::info!(
                                            "Requested Android overlay surface {}x{} dp at ({}, {})",
                                            options.width,
                                            options.height,
                                            options.x,
                                            options.y
                                        );
                                    }
                                    Err(error) => {
                                        overlay_window_options = None;
                                        log::warn!(
                                            "Android overlay surface unavailable; waiting for activity surface fallback: {error}"
                                        );
                                    }
                                }
                            }
                        }

                        if overlay_window_options.is_none()
                            && let Some(native_window) = app.native_window() {
                                let width = native_window.width() as u32;
                                let height = native_window.height() as u32;
                                let density =
                                    update_android_platform_geometry(&app, &mut android_platform);
                                let (input_offset_x, input_offset_y) =
                                    android_platform.input_surface_offset_px();
                                log::info!(
                                    "Display density: {:.2}x, input surface offset: ({:.1}, {:.1}) px",
                                    density,
                                    input_offset_x,
                                    input_offset_y
                                );

                                match initialize_android_rendering_with_backend_fallback(
                                    &mut wgpu_context,
                                    gpu_resources.take(),
                                    &mut app_shell,
                                    &content,
                                    &settings,
                                    &android_frame_driver,
                                    &host_window_registry,
                                    native_window.ptr().cast(),
                                    None,
                                    width,
                                    height,
                                    density,
                                    present_thread,
                                ) {
                                    Ok((resources, actual_size)) => {
                                        if let Some(actual_size) = actual_size {
                                            current_host_window_size = actual_size;
                                        }
                                        let width_dp = current_host_window_size.width;
                                        let height_dp = current_host_window_size.height;
                                        log::info!(
                                            "Set viewport to {:.1}x{:.1} dp ({}x{} px at {:.2}x density)",
                                            width_dp,
                                            height_dp,
                                            width,
                                            height,
                                            density
                                        );

                                        if let Some(requested) = initial_host_window_size.take() {
                                            if !android_activity_in_multi_window_mode(&app) {
                                                log::info!(
                                                    "Ignoring initial window size {:.1}x{:.1} dp: fullscreen Android activities keep the display-sized edge-to-edge surface; size requests apply in multi-window/freeform modes",
                                                    requested.width,
                                                    requested.height
                                                );
                                            } else {
                                                match dispatch_android_surface_size_request(
                                                    &app,
                                                    requested,
                                                    Point::ZERO,
                                                    density,
                                                    None,
                                                ) {
                                                    Ok(()) => {
                                                        pending_host_window_confirmation =
                                                            Some(PendingHostWindowSizeRequest {
                                                                state: None,
                                                                requested,
                                                                requested_at: Instant::now(),
                                                            });
                                                        log::info!(
                                                            "Requested initial Android host-window size {:.1}x{:.1} dp",
                                                            requested.width,
                                                            requested.height
                                                        );
                                                    }
                                                    Err(error) => {
                                                        log::warn!(
                                                            "Initial Android host-window size request failed: {error}"
                                                        );
                                                    }
                                                }
                                            }
                                        }

                                        gpu_resources = Some(resources);
                                        apply_display_visible_region(&app, &mut app_shell);
                                        log::info!("Rendering initialized successfully");
                                    }
                                    Err(error) => {
                                        log::error!("Android rendering initialization failed: {error}");
                                    }
                                }
                            }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        log::info!("Window terminated");
                        if overlay_window_options.is_none() {
                            drop_android_surface(
                                &mut gpu_resources,
                                &mut app_shell,
                                present_thread,
                            );
                        }
                    }
                    MainEvent::WindowResized { .. } => {
                        if overlay_window_options.is_none()
                            && let Some(native_window) = app.native_window() {
                                let width = native_window.width() as u32;
                                let height = native_window.height() as u32;

                                let density =
                                    update_android_platform_geometry(&app, &mut android_platform);
                                let (input_offset_x, input_offset_y) =
                                    android_platform.input_surface_offset_px();
                                log::info!(
                                    "Window resized to {}x{} at {:.2}x density with input surface offset ({:.1}, {:.1}) px",
                                    width,
                                    height,
                                    density,
                                    input_offset_x,
                                    input_offset_y
                                );

                                if let (Some(resources), Some(shell)) =
                                    (&mut gpu_resources, &mut app_shell)
                                    && width > 0 && height > 0 {
                                        resources.config.width = width;
                                        resources.config.height = height;
                                        if present_thread {
                                            let renderer = shell.renderer();
                                            renderer.note_surface_reconfigured();
                                            renderer.present_reconfigure(resources.config.clone());
                                            resources.surface_dirty = true;
                                        } else if let Some(surface) = resources.surface.as_ref() {
                                            surface.configure(&resources.device, &resources.config);
                                        }

                                        shell.set_buffer_size(width, height);

                                        if let Some(actual_size) =
                                            update_android_shell_geometry(
                                                shell,
                                                density,
                                                &host_window_registry,
                                            )
                                        {
                                            current_host_window_size = actual_size;
                                        }
                                    }
                            }
                    }
                    MainEvent::ContentRectChanged { .. } => {
                        let density = update_android_platform_geometry(&app, &mut android_platform);
                        if overlay_window_options.is_some() {
                            android_platform.set_input_surface_offset_px(0.0, 0.0);
                        }
                        let (input_offset_x, input_offset_y) =
                            android_platform.input_surface_offset_px();
                        log::info!(
                            "Content rect changed; input surface offset: ({:.1}, {:.1}) px at {:.2}x density",
                            input_offset_x,
                            input_offset_y,
                            density
                        );

                        if let Some(shell) = &mut app_shell
                            && let Some(actual_size) =
                                update_android_shell_geometry(shell, density, &host_window_registry)
                            {
                                current_host_window_size = actual_size;
                            }
                    }
                    MainEvent::RedrawNeeded { .. } => {
                        if let Some(shell) = &mut app_shell {
                            shell.mark_dirty();
                        }
                    }
                    MainEvent::Pause => {
                        log::info!("App paused");
                        cranpose_services::dispatch_lifecycle_state(
                            cranpose_services::LifecycleState::Paused,
                        );
                        if let Some(shell) = &mut app_shell {
                            shell.notify_app_paused();
                        }
                        ime_session.ensure_hidden();
                    }
                    MainEvent::Resume { .. } => {
                        log::info!("App resumed");
                        cranpose_services::dispatch_lifecycle_state(
                            cranpose_services::LifecycleState::Resumed,
                        );
                        let reopened = app_shell
                            .as_mut()
                            .map(|shell| shell.notify_app_resumed())
                            .unwrap_or(false);
                        if !reopened {
                            ime_session.ensure_hidden();
                        }
                    }
                    MainEvent::Start => {
                        log::info!("App started");
                        cranpose_services::dispatch_lifecycle_state(
                            cranpose_services::LifecycleState::Started,
                        );
                    }
                    MainEvent::Stop => {
                        log::info!("App stopped");
                        cranpose_services::dispatch_lifecycle_state(
                            cranpose_services::LifecycleState::Stopped,
                        );
                    }
                    MainEvent::SaveState { .. } => {
                        log::info!("Save state requested");
                    }
                    MainEvent::Destroy => {
                        log::info!("App destroy requested, will exit after this event");
                        cranpose_services::dispatch_lifecycle_state(
                            cranpose_services::LifecycleState::Destroyed,
                        );
                        if overlay_window_options.is_some() {
                            android_overlay_window::hide_android_overlay_window(&app);
                        }
                        should_exit.store(true, Ordering::Relaxed);
                    }
                    MainEvent::InputAvailable => {
                        drain_android_input_events(
                            &app,
                            &android_platform,
                            &mut key_translator,
                            &mut pending_inputs,
                            &mut primary_pointer_id,
                        );
                    }
                    MainEvent::ConfigChanged { .. } => {
                        let theme = system_theme_from_android(app.config().ui_mode_night());
                        if android_platform_env().set_system_theme(theme)
                            && let Some(shell) = &mut app_shell {
                                shell.request_root_render();
                            }
                        if crate::android_font_scale::refresh_font_scale(&app)
                            && let Some(shell) = &mut app_shell {
                                shell.set_font_scale_curve(
                                    crate::android_font_scale::font_scale_curve(),
                                );
                            }
                        apply_display_visible_region(&app, &mut app_shell);
                    }
                    _ => {}
                }
            }
        });
        frame_timings.after_poll_ns = frame_telemetry.now();

        for event in
            android_overlay_window::drain_android_overlay_window_events(&overlay_event_queue)
        {
            match event {
                android_overlay_window::AndroidOverlayWindowEvent::CreateFailed(message) => {
                    log::warn!("Android overlay surface failed: {message}");
                    overlay_window_options = None;

                    if let Some(native_window) = app.native_window() {
                        let width = native_window.width() as u32;
                        let height = native_window.height() as u32;
                        if width > 0 && height > 0 {
                            let density =
                                update_android_platform_geometry(&app, &mut android_platform);
                            match initialize_android_rendering_with_backend_fallback(
                                &mut wgpu_context,
                                gpu_resources.take(),
                                &mut app_shell,
                                &content,
                                &settings,
                                &android_frame_driver,
                                &host_window_registry,
                                native_window.ptr().cast(),
                                None,
                                width,
                                height,
                                density,
                                present_thread,
                            ) {
                                Ok((resources, actual_size)) => {
                                    apply_initialized_android_rendering(
                                        &app,
                                        &mut app_shell,
                                        &mut gpu_resources,
                                        &mut current_host_window_size,
                                        resources,
                                        actual_size,
                                    );
                                }
                                Err(error) => {
                                    log::error!(
                                        "Android activity surface fallback initialization failed: {error}"
                                    );
                                }
                            }
                        }
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::SurfaceChanged {
                    native_window,
                    width,
                    height,
                } => {
                    if width > 0 && height > 0 {
                        let density = get_display_density(&app);
                        android_platform.set_scale_factor(density as f64);
                        android_platform.set_input_surface_offset_px(0.0, 0.0);
                        if let Some(shell) = app_shell.as_mut() {
                            shell.set_density(density);
                        }

                        let native_window_ptr = native_window.ptr().cast();
                        match initialize_android_rendering_with_backend_fallback(
                            &mut wgpu_context,
                            gpu_resources.take(),
                            &mut app_shell,
                            &content,
                            &settings,
                            &android_frame_driver,
                            &host_window_registry,
                            native_window_ptr,
                            Some(native_window),
                            width,
                            height,
                            density,
                            present_thread,
                        ) {
                            Ok((resources, actual_size)) => {
                                apply_initialized_android_rendering(
                                    &app,
                                    &mut app_shell,
                                    &mut gpu_resources,
                                    &mut current_host_window_size,
                                    resources,
                                    actual_size,
                                );
                                log::info!(
                                    "Android overlay surface ready at {}x{} px ({:.2}x density)",
                                    width,
                                    height,
                                    density
                                );
                            }
                            Err(error) => {
                                log::error!(
                                    "Android overlay surface initialization failed: {error}"
                                );
                            }
                        }
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::SurfaceDestroyed => {
                    if overlay_window_options.is_some() {
                        drop_android_surface(&mut gpu_resources, &mut app_shell, present_thread);
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::Pointer { action, x, y } => {
                    let logical = android_platform.pointer_position(x as f64, y as f64);
                    match action {
                        android_overlay_window::AndroidOverlayPointerAction::Down => {
                            pending_inputs.push(PendingInput::PointerDown(
                                logical.x,
                                logical.y,
                                None,
                                PointerSource::Touch,
                            ));
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Up => {
                            pending_inputs.push(PendingInput::PointerUp(
                                logical.x,
                                logical.y,
                                None,
                                PointerSource::Touch,
                            ));
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Cancel => {
                            pending_inputs.push(PendingInput::PointerCancel);
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Move => {
                            pending_inputs.push(PendingInput::PointerMove(
                                logical.x,
                                logical.y,
                                None,
                                PointerSource::Touch,
                            ));
                        }
                    }
                }
            }
        }

        if !soft_keyboard_installed && let Some(shell) = &mut app_shell {
            shell.set_platform_text_input(Rc::new(AndroidSoftKeyboard::new(Rc::clone(
                &ime_session,
            ))));
            soft_keyboard_installed = true;
            log::info!("Android soft keyboard focus hook installed");

            let clipboard_app = app.clone();
            shell.app_context().enter(move || {
                cranpose_ui::clipboard_session::set_platform_clipboard(Rc::new(
                    crate::android_services::AndroidClipboard { app: clipboard_app },
                ));
            });

            if !shell.notify_app_resumed() {
                ime_session.ensure_hidden();
                log::info!("No focused field at launch; ensured soft keyboard hidden");
            }
        }

        if let Some(shell) = &mut app_shell {
            for (x, y) in crate::android_accessibility::drain_activations() {
                shell.set_cursor(x, y);
                shell.pointer_pressed();
                shell.pointer_released_at_position(x, y);
            }
            for (virtual_id, action_index) in crate::android_accessibility::drain_custom_actions() {
                let Some((node_id, canvas_key)) =
                    crate::accessibility::resolve_element_id(&accessibility_elements, virtual_id)
                else {
                    continue;
                };
                let Some(tree) = shell.semantics_tree() else {
                    continue;
                };
                crate::accessibility::perform_custom_action(
                    tree.root(),
                    node_id,
                    canvas_key,
                    action_index,
                );
            }
            for event in ime_event_queue.drain() {
                dispatch_android_ime_event(shell, event);
            }
        }

        crate::android_services::apply_pending_platform_signals(
            get_display_density(&app),
            &mut app_shell,
        );

        if !pending_inputs.is_empty() {
            last_interaction = Some(Instant::now());
            if let Some(shell) = &mut app_shell {
                for input in pending_inputs.drain(..) {
                    match input {
                        PendingInput::PointerDown(x, y, time_ms, source) => {
                            shell.set_pointer_source(source);
                            let event_time = shell.realtime_pointer_event_time(time_ms);
                            shell.set_cursor_at_event_time(x, y, event_time);
                            shell.pointer_pressed_at_event_time(event_time);
                        }
                        PendingInput::PointerUp(x, y, time_ms, source) => {
                            shell.set_pointer_source(source);
                            let event_time = shell.realtime_pointer_event_time(time_ms);
                            shell.pointer_released_at_position_event_time(x, y, event_time);
                        }
                        PendingInput::PointerMove(x, y, time_ms, source) => {
                            shell.set_pointer_source(source);
                            let event_time = shell.realtime_pointer_event_time(time_ms);
                            shell.set_cursor_at_event_time(x, y, event_time);
                        }
                        PendingInput::PointerCancel => {
                            shell.cancel_gesture();
                        }
                        PendingInput::Key(event) => {
                            shell.on_key_event(&event);
                        }
                        PendingInput::SecondaryPointerDown(id, x, y, time_ms) => {
                            shell.set_pointer_source(PointerSource::Touch);
                            shell.secondary_pointer_pressed(id, x, y, time_ms);
                        }
                        PendingInput::SecondaryPointerUp(id, x, y, time_ms) => {
                            shell.set_pointer_source(PointerSource::Touch);
                            shell.secondary_pointer_released(id, x, y, time_ms);
                        }
                        PendingInput::SecondaryPointerMove(id, x, y, time_ms) => {
                            shell.set_pointer_source(PointerSource::Touch);
                            shell.secondary_pointer_moved(id, x, y, time_ms);
                        }
                        PendingInput::RotaryScroll(detents, uptime_ms) => {
                            shell.rotary_scrolled_by_detents(detents, uptime_ms);
                        }
                    }
                }
            }
        }

        if ime_session.is_active()
            && let Some(shell) = &mut app_shell
        {
            ime_session.sync_editor_state(shell.ime_editor_state());
        }

        if android_frame_driver.take_frame_request()
            && let Some(shell) = &mut app_shell
        {
            shell.mark_dirty();
        }

        confirm_android_host_window_request(
            &mut pending_host_window_confirmation,
            current_host_window_size,
        );

        if cranpose_services::exit_requested() && exit_attempts < MAX_EXIT_ATTEMPTS {
            exit_attempts += 1;
            log::info!("App requested exit; finishing the activity");
            if crate::android_finish::finish_activity(&app) {
                let _ = cranpose_services::take_exit_request();
            } else if exit_attempts == MAX_EXIT_ATTEMPTS {
                log::error!(
                    "giving up on the app's exit request after {MAX_EXIT_ATTEMPTS} attempts; \
                     the activity is still up"
                );
                let _ = cranpose_services::take_exit_request();
            }
        }

        if should_exit.load(Ordering::Relaxed) {
            log::info!("Exiting cleanly after Destroy event");
            break;
        }

        let offscreen_due = next_offscreen_update.is_some_and(|at| at <= web_time::Instant::now());
        if offscreen && (offscreen_pending_ui || offscreen_due) {
            if let Some(shell) = &mut app_shell
                && shell.needs_update()
            {
                android_host_window::with_android_host_window_registry(
                    &host_window_registry,
                    || shell.update(),
                );
            }
            next_offscreen_update = match offscreen_pending_ui {
                true => Some(web_time::Instant::now() + OFFSCREEN_UPDATE_PERIOD),
                false => None,
            };
        }

        let mut adpf_work_started: Option<web_time::Instant> = None;
        let mut adpf_sync_presented = false;
        if let (Some(resources), Some(shell)) = (&mut gpu_resources, &mut app_shell) {
            if resources.has_surface()
                && shell.needs_update()
                && shell.renderer().has_frame_credit()
            {
                adpf_work_started = Some(web_time::Instant::now());
                let update_result = android_host_window::with_android_host_window_registry(
                    &host_window_registry,
                    || shell.update(),
                );
                frame_timings.after_update_ns = frame_telemetry.now();
                if let Err(error) = crate::android_accessibility::sync(
                    &app,
                    shell,
                    android_platform.scale_factor(),
                    &mut accessibility_elements,
                    &mut accessibility_revision,
                    &mut accessibility_policy,
                ) {
                    log::warn!("{error}");
                }
                dispatch_registered_android_surface_size_request(
                    &app,
                    &host_window_registry,
                    android_platform.scale_factor(),
                    overlay_window_options,
                    &mut last_dispatched_host_window_request,
                    &mut pending_host_window_confirmation,
                );
                frame_timings.after_sync_ns = frame_telemetry.now();
                if surface_present_required(
                    resources.surface_dirty,
                    update_result.visual_changed,
                    shell.needs_redraw(),
                ) {
                    if present_thread {
                        let (width, height) = shell.buffer_size();
                        match shell.renderer().publish_frame(width, height) {
                            PublishOutcome::Published => {
                                pending_present_timings.push((
                                    shell.renderer().last_published_frame_id(),
                                    frame_timings,
                                ));
                            }
                            PublishOutcome::NoGraph | PublishOutcome::NoCredit => {
                                frame_telemetry.note_idle_iteration();
                            }
                        }
                    } else if render_once(
                        resources,
                        shell,
                        &mut frame_telemetry,
                        &mut frame_timings,
                    ) {
                        break;
                    } else {
                        adpf_sync_presented = !resources.surface_dirty;
                    }
                } else {
                    frame_telemetry.note_idle_iteration();
                }
            } else {
                frame_telemetry.note_idle_iteration();
                if accessibility_policy
                    .wake_deadline()
                    .is_some_and(|deadline| deadline <= std::time::Instant::now())
                    && let Err(error) = crate::android_accessibility::sync(
                        &app,
                        shell,
                        android_platform.scale_factor(),
                        &mut accessibility_elements,
                        &mut accessibility_revision,
                        &mut accessibility_policy,
                    )
                {
                    log::warn!("{error}");
                }
            }
        } else {
            frame_telemetry.note_idle_iteration();
        }

        let presented_interval = if frame_timings.after_present_ns != 0 {
            let frame_finished_at = web_time::Instant::now();
            let frame_started_at = adpf_work_started.unwrap_or(frame_finished_at);
            Some((frame_started_at, frame_finished_at))
        } else {
            drained_present_interval
        };
        if adpf_sync_presented && let Some(started) = adpf_work_started {
            let reported = crate::android_frame_telemetry::vsync_period_ns();
            let period = if reported > 0 {
                reported
            } else {
                crate::android_vsync::observed_vsync_period_ns().unwrap_or(16_666_667)
            };
            let session = perf_hint
                .get_or_insert_with(|| crate::android_perf_hint::PerfHintSession::open(period));
            if let Some(session) = session.as_mut() {
                session.report(started.elapsed().as_nanos() as i64, period);
            }
        }
        if let Some((frame_started_at, frame_finished_at)) = presented_interval {
            record_presented_frame(app_shell.as_mut(), frame_started_at, frame_finished_at);
            behind_deadline = catchup_pacing
                && last_present_at.is_some_and(|previous| {
                    let reported = crate::android_frame_telemetry::vsync_period_ns();
                    let period = if reported > 0 {
                        reported
                    } else {
                        crate::android_vsync::observed_vsync_period_ns().unwrap_or(16_666_667)
                    };
                    frame_finished_at.duration_since(previous).as_nanos() as i64
                        > period + period / 16
                });
            catchup_coasts = 0;
            last_present_at = Some(frame_finished_at);
        } else if behind_deadline {
            catchup_coasts += 1;
            if catchup_coasts >= 3 {
                behind_deadline = false;
            }
        }
    }

    if let Some(shell) = app_shell.as_mut() {
        shell.renderer().shutdown_present_runtime();
    }
}
