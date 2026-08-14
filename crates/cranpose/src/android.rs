//! Android runtime for Compose applications.
//!
//! This module provides the Android event loop implementation with proper
//! lifecycle management, input handling, and rendering coordination.

use crate::{
    android_host_window,
    android_jni::{clear_pending_android_jni_exception, with_android_activity_env},
    android_keyboard::{self, is_system_key, AndroidKeyTranslator, AndroidSoftKeyboard},
    android_overlay_window,
    android_surface::{create_android_wgpu_surface, AndroidSurfaceError},
    android_text_input::{self, AndroidImeEvent},
    launcher::{AndroidOverlayWindowOptions, AppSettings},
    wgpu_surface::surface_present_required,
    wgpu_surface::{current_surface_texture, SurfaceFrame},
};
use cranpose_app_shell::{
    default_root_key, AppShell, KeyEvent, PlatformFrameDriver, PointerSource,
};
use cranpose_platform_android::AndroidPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{Point, Size};
use ndk::native_window::NativeWindow;
use std::time::{Duration, Instant};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    ptr::NonNull,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// GPU resources for the current Android surface and its reusable WGPU device.
struct GpuResources {
    surface: wgpu::Surface<'static>,
    native_window_ptr: NonNull<c_void>,
    adapter: Arc<wgpu::Adapter>,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    backend: wgpu::Backend,
    config: wgpu::SurfaceConfiguration,
    _native_window: Option<NativeWindow>,
    /// Set when the surface is (re)created and cleared after a successful
    /// present, forcing the first frame to reach the screen even when `update()`
    /// reports no visual work (mirrors the desktop/web `surface_dirty` flag).
    surface_dirty: bool,
}

/// Pending input event to be processed outside poll_events callback.
/// This prevents blocking the main thread during input event acknowledgment.
///
/// Pointer variants carry the MotionEvent's own timestamp in milliseconds
/// (uptime clock). Android delivers touch input batched/frame-aligned, so
/// several samples are processed back-to-back in one loop iteration;
/// stamping them with the delivery time instead of the event time makes
/// gesture velocity computations wildly wrong (dt collapses to ~0).
enum PendingInput {
    PointerDown(f32, f32, Option<i64>, PointerSource),
    PointerUp(f32, f32, Option<i64>, PointerSource),
    PointerMove(f32, f32, Option<i64>, PointerSource),
    PointerCancel,
    /// Translated key event (physical keyboard or soft-keyboard fallback
    /// input), routed to the focused text field via `AppShell::on_key_event`.
    Key(KeyEvent),
    /// Additional finger of a multi-touch gesture (pinch/zoom). The first
    /// field is the shell pointer id (never 0, which is the primary finger).
    SecondaryPointerDown(u64, f32, f32, Option<i64>),
    SecondaryPointerUp(u64, f32, f32, Option<i64>),
    SecondaryPointerMove(u64, f32, f32, Option<i64>),
    /// Rotary encoder step (Wear OS crown / rotating bezel). The first field is
    /// the raw `AXIS_SCROLL` value in detents (converted to pixels by the shell
    /// using its rotary scroll factor); the second is the event's uptime in
    /// milliseconds.
    RotaryScroll(f32, u64),
}

/// Converts an Android event time (nanoseconds, `java.lang.System.nanoTime()`
/// base) into milliseconds for gesture velocity tracking.
fn android_event_time_ms(event_time_ns: i64) -> i64 {
    event_time_ns / 1_000_000
}

/// Maps an Android `MotionEvent` pointer onto the framework [`PointerSource`]
/// so text fields can show finger handles only for touch/stylus input.
///
/// The per-pointer tool type is the most specific signal, but many devices and
/// the Android emulator report `TOOL_TYPE_UNKNOWN` for genuine finger touches;
/// in that case the whole-event input source class (`getSource`) is consulted
/// so a touchscreen press is still stamped [`PointerSource::Touch`]. Without
/// this fallback the finger selection/cursor handles (touch-only) never appear.
fn android_pointer_source(
    pointer: &android_activity::input::Pointer<'_>,
    event_source: android_activity::input::Source,
) -> PointerSource {
    use crate::android_input::{resolve_pointer_source, AndroidSourceKind, AndroidToolKind};
    use android_activity::input::{Source, ToolType};

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

/// Translates one Android `KeyEvent` into a pending framework key event.
///
/// System keys (back, volume, media, ...) are reported as unhandled so the
/// platform keeps processing them. The back key is special: while the app has
/// back interception enabled (see [`cranpose_services::set_back_interception`])
/// it is consumed and forwarded to [`cranpose_services::push_back_request`],
/// mirroring Compose's `BackHandler`; otherwise it stays with the system so
/// the default leave-the-activity behavior keeps working. Returns `true` when
/// the event was consumed.
fn push_pending_input_from_android_key_event(
    key_event: &android_activity::input::KeyEvent<'_>,
    key_translator: &mut AndroidKeyTranslator,
    pending_inputs: &mut Vec<PendingInput>,
) -> bool {
    if key_event.key_code() == android_activity::input::Keycode::Back {
        if cranpose_services::back_interception_enabled() {
            // Act on key-up (Android convention) but consume the whole
            // down/up pair so the system never sees a half-handled back.
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
    /// Live platform environment (IME/safe-area insets in *logical* px, system
    /// theme) shared between the shell content wrapper — which provides the
    /// values to composition — and the event loop, which updates them from
    /// Java-forwarded keyboard heights, content-rect changes, and
    /// configuration changes.
    static ANDROID_PLATFORM_ENV: Rc<crate::platform_env::PlatformEnvironment> =
        crate::platform_env::PlatformEnvironment::new();
    /// Density used to convert Java's physical keyboard height to logical px.
    static ANDROID_IME_DENSITY: Cell<f32> = const { Cell::new(1.0) };
}

/// Shared handle to the live platform environment.
pub(crate) fn android_platform_env() -> Rc<crate::platform_env::PlatformEnvironment> {
    ANDROID_PLATFORM_ENV.with(Rc::clone)
}

/// Records the density used to convert the Java keyboard height to logical px.
fn set_android_ime_density(density: f32) {
    ANDROID_IME_DENSITY.with(|cell| cell.set(density.max(f32::EPSILON)));
}

/// Stores the keyboard's covered height (physical px, `0` when hidden) as
/// bottom IME insets in logical px. Returns whether the value changed.
fn set_android_ime_bottom_px(bottom_px: i32) -> bool {
    let density = ANDROID_IME_DENSITY.with(|cell| cell.get());
    let bottom = (bottom_px.max(0) as f32) / density;
    let insets = cranpose_ui::EdgeInsets {
        bottom,
        ..cranpose_ui::EdgeInsets::default()
    };
    android_platform_env().set_ime_insets(insets)
}

/// Maps the Android night-mode configuration onto the framework system theme.
fn system_theme_from_android(
    night: ndk::configuration::UiModeNight,
) -> cranpose_services::SystemTheme {
    match night {
        ndk::configuration::UiModeNight::Yes => cranpose_services::SystemTheme::Dark,
        _ => cranpose_services::SystemTheme::Light,
    }
}

/// `EditorInfo.IME_ACTION_DONE`: the user tapped the keyboard's Done action.
const IME_ACTION_DONE: i32 = 6;

/// Applies one editing operation forwarded by the Java `InputConnection`
/// (see [`crate::android_text_input`]) to the focused text field.
fn dispatch_android_ime_event(shell: &mut AppShell<WgpuRenderer>, event: AndroidImeEvent) {
    match event {
        AndroidImeEvent::CommitText { text, .. } => {
            // commitText replaces the composing text (if any); clearing the
            // preedit first deletes it, then the final text is inserted.
            // `new_cursor_position` values other than 1 are not honored yet;
            // the editor-state sync restarts the IME session if the cursor
            // ends up somewhere the IME did not expect.
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
            // Done follows the platform convention of dismissing the
            // keyboard (focus stays in the field). Other actions are
            // surfaced as an Enter key press: single-line fields ignore it
            // and apps can react through their key handlers. A dedicated
            // per-field ime-action callback (Compose `KeyboardActions`) is
            // not modeled yet.
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
            // Publish the keyboard height as IME insets and force a root render
            // so the content wrapper re-provides `local_ime_insets` (a plain
            // shared cell is not itself reactive).
            if set_android_ime_bottom_px(bottom_px) {
                shell.request_root_render();
            }
        }
    }
}

/// Maps an Android pointer id to the shell's pointer id space: the finger
/// that started the gesture (`ACTION_DOWN`) is the shell's primary pointer
/// `0`; every other finger gets a stable non-zero id.
fn shell_pointer_id(android_pointer_id: i32, primary_pointer_id: i32) -> u64 {
    if android_pointer_id == primary_pointer_id {
        0
    } else {
        android_pointer_id as u64 + 1
    }
}

/// Translates one Android input event into pending framework inputs.
///
/// The finger that started the gesture (`ACTION_DOWN`) is tracked in
/// `primary_pointer_id` and forwarded as the shell's primary pointer; its
/// Move events unpack the batched *historical* samples first (oldest to
/// newest, each with its own timestamp) and then the current sample, so the
/// velocity tracker sees every real touch sample instead of only the last
/// position of each batch. Additional fingers (`ACTION_POINTER_DOWN`) are
/// forwarded as secondary pointers with their ids so multi-touch gestures
/// (pinch/zoom) see them. Returns `true` when the event was consumed.
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
    // The whole-event input source class; used to recover a touch classification
    // when the per-pointer tool type is unreported (see `android_pointer_source`).
    let event_source = motion_event.source();
    let logical_of = |x: f32, y: f32| {
        let logical = android_platform.pointer_position(x as f64, y as f64);
        (logical.x as f32, logical.y as f32)
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
                    // The gesture ends with a finger that is not the one that
                    // started it (the primary lifted earlier without ending
                    // the stream). Close the secondary, then the gesture.
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
                // The first finger lifted while others are still down. The
                // shell's gesture is anchored to the primary pointer, so end
                // the whole gesture: close the remaining secondaries, then
                // release the primary. New touches start a fresh gesture.
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
        // Rotary input (Pixel Watch crown, Galaxy Watch rotating bezel) arrives
        // as a *generic* motion event with ACTION_SCROLL from the
        // SOURCE_ROTARY_ENCODER class, carrying its delta on AXIS_SCROLL in
        // detents. NativeActivity's AInputQueue delivers generic motion events
        // through the same queue as touch, so no extra plumbing is needed --
        // but the activity's view must be focusable and focused or the platform
        // never dispatches them at all (see the module docs on
        // `push_rotary_pending_input`).
        android_activity::input::MotionAction::Scroll => {
            push_rotary_pending_input(motion_event, event_source, time_ms, pending_inputs)
        }
        _ => false,
    }
}

/// Translates an Android rotary `ACTION_SCROLL` motion event into a pending
/// rotary input.
///
/// Returns `true` only when the event really came from a rotary encoder, so a
/// mouse-wheel `ACTION_SCROLL` stays unhandled and keeps its default platform
/// behavior.
///
/// # Host requirements
///
/// Wear OS delivers rotary events **only to a focused, focusable view**. The
/// host activity must, on its `NativeActivity` content view:
/// `setFocusable(true)`, `setFocusableInTouchMode(true)`, and `requestFocus()`.
/// Without focus, `AInputQueue` receives nothing and this function is never
/// reached.
fn push_rotary_pending_input(
    motion_event: &android_activity::input::MotionEvent<'_>,
    event_source: android_activity::input::Source,
    time_ms: Option<i64>,
    pending_inputs: &mut Vec<PendingInput>,
) -> bool {
    use crate::android_input::is_rotary_encoder_source;

    if !is_rotary_encoder_source(u32::from(event_source)) {
        // A non-rotary scroll (mouse wheel); leave it to the platform.
        return false;
    }

    // The rotary delta lives on AXIS_SCROLL of pointer 0, in detents.
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
    /// The thread running the frame loop, so a request raised *by* the loop can
    /// be told apart from one raised by a worker. Only the latter needs a wake.
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

    /// Raises a frame request, waking the looper only when the request came
    /// from somewhere other than the frame loop itself.
    ///
    /// A request raised on the loop's own thread - the shell re-arming its
    /// frame callback, an input handler, an effect resuming inside `update` -
    /// is picked up by the very iteration that raised it. Waking the looper for
    /// it leaves a byte in `android-activity`'s wake pipe that makes the next
    /// poll return instantly, which cancels the vsync wait the loop was about
    /// to enter and turns an idle app into a busy loop.
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

    /// The waker for the display's own frame callback, which always wakes.
    ///
    /// The choreographer delivers on the looper of the thread that posted, so
    /// this fires on the frame loop's thread - and it is the one same-thread
    /// request that must *not* be folded into the current iteration, because
    /// the loop is asleep waiting for exactly this and has no iteration left to
    /// fold it into.
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

/// Delay of the follow-up composition pass after one that carried work while
/// the app runs with no surface (see the main loop).
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

/// Get display density from Android NDK Configuration.
///
/// Uses the NDK's AConfiguration_getDensity which returns density constants
/// mapped to the standard Android density classes:
/// - mdpi: 1.0 (160 dpi baseline)
/// - hdpi: 1.5 (240 dpi)
/// - xhdpi: 2.0 (320 dpi) - most common modern phones
/// - xxhdpi: 3.0 (480 dpi)
/// - xxxhdpi: 4.0 (640 dpi)
///
/// The factor is calculated as DPI / 160 per Android NDK documentation.
fn get_display_density(app: &android_activity::AndroidApp) -> f32 {
    let config = app.config();
    let density_dpi = config.density(); // Returns Option<u32> with raw DPI value

    // Convert DPI to scale factor (baseline is 160 dpi = 1.0x)
    // e.g., 320 dpi / 160 = 2.0x (xhdpi)
    density_dpi.map(|dpi| dpi as f32 / 160.0).unwrap_or(2.0) // Fallback to xhdpi (2.0) if density unavailable
}

/// The freeform/DeX shadow-margin inset (in physical px) between the render
/// buffer and the on-screen frame. The native window buffer is enlarged by this
/// margin on every side; the renderer fills the buffer from (0,0) while pointer
/// events are frame-relative, so this is exactly the offset to add to pointers.
/// Returns `(0, 0)` when there is no margin (fullscreen: buffer == frame).
fn surface_inset_px(app: &android_activity::AndroidApp) -> (f64, f64) {
    let Some(window) = app.native_window() else {
        return (0.0, 0.0);
    };
    let buffer_w = window.width() as f64;
    let buffer_h = window.height() as f64;
    // `content_rect`'s right/bottom edges are the frame's size (the caption only
    // insets the top, so the content still reaches the frame's right and bottom).
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

    // A freeform/DeX window's render surface is LARGER than its on-screen frame:
    // the system adds a shadow/resize margin (the window `surfaceInsets`), so the
    // native buffer we draw into is e.g. 525x879 px while the visible frame is
    // only 447x801. The renderer fills the whole buffer starting at (0,0), which
    // lands at `frame_origin - inset` on screen — but pointer coordinates arrive
    // relative to the *frame*. Without correction every touch is off by that inset
    // (~39 px / 24 dp), so controls near the edges become unreachable. We recover
    // the inset as half the difference between the buffer and the frame, and add
    // it to incoming pointers so they map into the same surface space the renderer
    // draws in. Fullscreen (phone) has buffer == frame, so this is a no-op there.
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
    // The user's font-size setting. Sizes in Sp follow it, sizes in Dp do not.
    shell.set_font_scale(crate::android_font_scale::font_scale());
    // Rotary detents are reported in device-independent units; the shell needs
    // a pixels-per-detent factor to match Compose. Approximates
    // `ViewConfiguration.getScaledVerticalScrollFactor()`; a host that reads
    // the exact platform value over JNI can overwrite this afterwards.
    shell.set_rotary_scroll_factor(crate::android_input::android_rotary_scroll_factor(density));

    let (width, height) = shell.buffer_size();
    if width > 0 && height > 0 {
        let width_dp = width as f32 / density;
        let height_dp = height as f32 / density;
        shell.set_viewport(width_dp, height_dp);
        let actual = Size::new(width_dp, height_dp);
        android_host_window::sync_android_host_window_actual_size(host_window_registry, actual);
        Some(actual)
    } else {
        None
    }
}

/// Renders a single frame. Returns true if out of memory (should exit).
fn render_once(
    resources: &mut GpuResources,
    shell: &mut AppShell<WgpuRenderer>,
    telemetry: &mut crate::android_frame_telemetry::AndroidFrameTelemetry,
    timings: &mut crate::android_frame_telemetry::FrameTimings,
) -> bool {
    match current_surface_texture(&resources.surface, "android") {
        SurfaceFrame::Ready(frame) => {
            timings.after_acquire_ns = telemetry.now();
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let (width, height) = shell.buffer_size();

            if let Err(e) = shell.renderer().render(&view, width, height) {
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
            resources
                .surface
                .configure(&resources.device, &resources.config);
            // The reconfigured surface has not presented yet. Unlike web/desktop,
            // the android render block is gated behind `shell.needs_update()`, so
            // marking the shell dirty is what both wakes the looper and re-enters
            // the present path on the next iteration to flush the new swapchain.
            resources.surface_dirty = true;
            shell.mark_dirty();
            false
        }
        // Surface unavailable this tick; retry the present on the next frame.
        SurfaceFrame::Skip => {
            telemetry.note_idle_iteration();
            resources.surface_dirty = true;
            false
        }
    }
}

struct AndroidGpuSetup {
    resources: GpuResources,
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

    /// The backend to probe first, honouring `CRANPOSE_ANDROID_GPU_BACKEND`.
    ///
    /// Vulkan is the default and the only backend the fallback below can arrive
    /// at on its own, which makes the two impossible to compare on one device:
    /// whichever one Vulkan probing picks is the one you measure. The two have
    /// genuinely different CPU costs per frame — a Mali Vulkan driver rebuilds
    /// its command list every submit, where the GLES driver keeps more state —
    /// so which is cheaper is a question about the device, not one we can settle
    /// by reasoning. This switch exists so it can be measured instead.
    ///
    /// Unrecognised values fall back to Vulkan rather than failing, since a
    /// mistyped diagnostic should not stop an app from starting.
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
        // No debug/validation: emulator Vulkan debug utils can crash on null labels.
        descriptor.flags = wgpu::InstanceFlags::empty();
        Self {
            instance: wgpu::Instance::new(descriptor),
            backend,
        }
    }
}

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
) -> Result<(GpuResources, Option<Size>), AndroidSurfaceError>
where
    F: FnMut() + 'static,
{
    let setup = create_android_gpu_resources(
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
        renderer.init_gpu(
            setup.resources.device.clone(),
            setup.resources.queue.clone(),
            setup.resources.surface_format,
            setup.resources.backend,
        );

        let content_clone = content.clone();
        let density = density.max(f32::EPSILON);
        let platform_env = android_platform_env();
        let mut shell = AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            move || {
                // Provide the live platform environment (IME/safe-area insets,
                // system theme) to composition. Read on every recomposition;
                // the event loop forces a root render when a value changes.
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
            shell.renderer().init_gpu(
                setup.resources.device.clone(),
                setup.resources.queue.clone(),
                setup.resources.surface_format,
                setup.resources.backend,
            );
            log::info!("Renderer reinitialized with new Android GPU pipeline resources");
        }
    } else {
        log::debug!("Reused Android WGPU device and renderer resources for surface update");
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
    ) {
        Err(AndroidSurfaceError::RequestAdapter(error)) => {
            // Keep the two backends in separate instances. On Android emulators
            // where Vulkan probing fails, a combined Vulkan|GL instance can keep
            // the ANativeWindow connected while WGPU falls through to EGL. EGL
            // then aborts with BAD_ALLOC because the window belongs to another
            // graphics API. Dropping the Vulkan-only instance before constructing
            // the GL surface gives the fallback a clean native window.
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
        if resources.native_window_ptr == native_window_ptr {
            resources.config.width = width;
            resources.config.height = height;
            resources
                .surface
                .configure(&resources.device, &resources.config);
            if let Some(native_window_owner) = native_window_owner {
                resources._native_window = Some(native_window_owner);
            }
            return Ok(AndroidGpuSetup {
                resources,
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

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Android Device"),
        required_features: wgpu::Features::empty(),
        required_limits: crate::gpu_limits::mobile_device_limits(adapter.limits()),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: crate::gpu_limits::mobile_memory_hints(),
        trace: wgpu::Trace::Off,
    }))?;

    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let config = create_android_surface_config(&surface, &adapter, width, height)?;
    surface.configure(&device, &config);

    Ok(AndroidGpuSetup {
        resources: GpuResources {
            surface,
            native_window_ptr,
            adapter,
            device,
            queue,
            surface_format: config.format,
            backend: adapter_info.backend,
            config,
            _native_window: native_window_owner,
            surface_dirty: true,
        },
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
    surface.configure(&existing.device, &config);
    let renderer_needs_init = config.format != existing.surface_format;

    Ok(AndroidGpuSetup {
        resources: GpuResources {
            surface,
            native_window_ptr,
            adapter: existing.adapter.clone(),
            device: existing.device.clone(),
            queue: existing.queue.clone(),
            surface_format: config.format,
            backend: existing.backend,
            config,
            _native_window: native_window_owner,
            surface_dirty: true,
        },
        renderer_needs_init,
    })
}

fn create_android_surface_config(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    width: u32,
    height: u32,
) -> Result<wgpu::SurfaceConfiguration, AndroidSurfaceError> {
    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format =
        crate::surface_format::select_display_surface_format(&surface_caps.formats)
            .ok_or(AndroidSurfaceError::NoSurfaceFormat)?;
    let alpha_mode = surface_caps
        .alpha_modes
        .first()
        .copied()
        .ok_or(AndroidSurfaceError::NoAlphaMode)?;
    let present_mode = crate::present_mode::select_android_present_mode(&surface_caps);
    // `debug.cranpose.frame_latency` lets the swapchain depth be A/B'd on device.
    let desired_maximum_frame_latency =
        crate::android_frame_telemetry::system_property("debug.cranpose.frame_latency")
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|latency| (1..=3).contains(latency))
            .unwrap_or(2);
    log::info!(
        "Android surface: supported present modes {:?}, selected {:?}, desired_maximum_frame_latency {}",
        surface_caps.present_modes,
        present_mode,
        desired_maximum_frame_latency,
    );
    Ok(wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width,
        height,
        present_mode,
        alpha_mode,
        view_formats: vec![],
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

/// Whether the activity is currently in multi-window mode (split-screen,
/// freeform, or desktop windowing such as Samsung DeX).
///
/// A fullscreen phone activity's window is laid out edge-to-edge across the
/// whole display (`PhoneWindow.generateLayout` sets `FLAG_LAYOUT_IN_SCREEN |
/// FLAG_LAYOUT_INSET_DECOR` and clears `fitInsetsTypes` for every non-floating
/// window), so its `ANativeWindow` buffer already covers the areas behind the
/// status and navigation bars. Calling `Window.setLayout` with a fixed pixel
/// size on that window replaces `MATCH_PARENT`; devices that honor it shrink
/// and center the surface, leaving uncovered strips of raw display that show
/// as black bands at the top and bottom. Host-window sizing is therefore only
/// meaningful in multi-window modes, where the window is genuinely resizable.
///
/// Returns `false` when the query fails (including API < 24, where
/// `isInMultiWindowMode` does not exist and multi-window is unavailable).
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

/// Runs an Android Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_android()`. This is the framework-level
/// entrypoint that manages the Android lifecycle and event loop.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub fn run(
    app: android_activity::AndroidApp,
    settings: AppSettings,
    content: impl FnMut() + 'static,
) {
    use android_activity::{MainEvent, PollEvent};

    // Logging first: the registrations below already have failure paths worth
    // hearing about (a launch-args read that fails here is otherwise silent).
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("ComposeRS")
            .with_filter(
                android_logger::FilterBuilder::new()
                    .filter_level(log::LevelFilter::Info)
                    .filter_module("wgpu_core", log::LevelFilter::Warn)
                    .filter_module("wgpu_hal", log::LevelFilter::Warn)
                    .filter_module("naga", log::LevelFilter::Warn)
                    // `AChoreographer` registers its display-event fd with this
                    // thread's looper, so every vsync makes `ALooper_pollOnce`
                    // return `ALOOPER_POLL_CALLBACK`. android-activity 0.6.1
                    // believes that return value is impossible and logs it at
                    // `error!` — once per frame, which is a synchronous
                    // `writev` to logd plus a tag property lookup 60 times a
                    // second. The event is genuinely harmless (the crate
                    // ignores it and polls again), but the logging is not, so
                    // the module stays quiet. Frame pacing lives in
                    // `android_vsync`, which reports its own failures.
                    .filter_module("android_activity::activity_impl", log::LevelFilter::Off)
                    .build(),
            ),
    );

    // Mirror the `debug.cranpose.*` diagnostic properties into the environment
    // before anything reads it, so the renderer's and app shell's existing
    // environment-gated telemetry is reachable on device.
    crate::android_frame_telemetry::seed_env_from_system_properties();

    // Register the SAF document picker as the platform file picker. Requires the
    // app's activity to be `dev.cranpose.android.CranposeActivity`.
    crate::android_file_picker::register(app.clone());
    // Register the SAF writable-folder backend (write-side complement, used for
    // cross-device sync into a user-granted folder).
    crate::android_writable_folder::register(app.clone());
    // Haptics, share sheet, notifier, network status (JNI backends of the
    // CranposeActivity capability hooks).
    crate::android_services::register(app.clone());

    // Seed the system theme from the boot configuration; live switches arrive
    // as `MainEvent::ConfigChanged`.
    android_platform_env()
        .set_system_theme(system_theme_from_android(app.config().ui_mode_night()));

    // Same for the font-size setting, which the NDK configuration does not
    // carry — it is read over JNI here and again on every configuration
    // change. The shell picks it up with the rest of the geometry.
    crate::android_font_scale::refresh_font_scale(&app);

    // What Play actually packaged, which a version compiled into the binary
    // cannot know: Gradle's `versionNameSuffix` and a CI-stamped version code
    // are both applied after the compiler has been and gone. Read once — the
    // packaged identity of a running process does not change.
    crate::android_app_info::install_app_info(&app);

    // Install panic hook for better crash logging in Logcat
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown location".to_string());
        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| *s)
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|s| s.as_str())
            })
            .unwrap_or("Box<dyn Any>");
        log::error!("PANIC at {}: {}", location, message);
    }));

    // Wrap content in Rc<RefCell> for reuse across window recreations
    let content = std::rc::Rc::new(std::cell::RefCell::new(content));

    // App shell (created once, persists across window recreations)
    let mut app_shell: Option<AppShell<WgpuRenderer>> = None;
    let mut accessibility_elements = Vec::new();
    let mut accessibility_revision = None;

    log::info!("Starting Compose Android Application");

    let android_frame_driver = AndroidFrameDriver::new(app.create_waker());
    let mut frame_rate_voter = crate::android_frame_rate::FrameRateVoter::default();
    // How long after the last input event the Auto frame-rate vote keeps the
    // panel's fast rate. SurfaceFlinger's own touch boost holds for seconds,
    // and a shorter hold-off measurably hurts: at 1 s the display mode
    // flip-flopped between gestures and the switch hiccups pulled presented
    // fps *below* 60 (55.9 measured); at 3 s a continuously-driven scene
    // holds the fast rate while an untouched animation still settles quickly.
    const FRAME_RATE_BOOST_HOLD_OFF: Duration = Duration::from_secs(3);
    let mut last_interaction: Option<Instant> = None;
    crate::android_vsync::install_waker(android_frame_driver.vsync_waker());
    crate::android_accessibility::set_waker(app.create_waker());
    let host_window_registry = Rc::new(android_host_window::AndroidHostWindowRegistry::default());
    let overlay_event_queue = Arc::new(android_overlay_window::AndroidOverlayEventQueue::default());

    // IME editor bridge (InputConnection-backed text input). The queue crosses
    // the JNI/UI-thread boundary; the session state stays on this thread.
    let ime_event_queue = Arc::new(android_text_input::AndroidImeEventQueue::new());
    ime_event_queue.set_waker(app.create_waker());
    let ime_session =
        android_keyboard::AndroidImeSession::new(app.clone(), Arc::clone(&ime_event_queue));

    // Exit flag for Destroy event (can't break from inside poll_events closure)
    let should_exit = Arc::new(AtomicBool::new(false));

    // Probe Vulkan first, then create a fresh GL-only instance if no compatible
    // adapter exists. Keeping the backends separate is important on emulators:
    // a failed Vulkan probe can otherwise leave the ANativeWindow connected and
    // make EGL surface creation abort with `EGL_BAD_ALLOC`.
    let mut wgpu_context = AndroidWgpuContext::new(AndroidGpuBackend::preferred());

    // Platform abstraction for density/pointer conversion
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

    // GPU resources (recreated when window is destroyed/created)
    let mut gpu_resources: Option<GpuResources> = None;

    // Queue for input events (processed outside poll_events to prevent ANR)
    let mut pending_inputs: Vec<PendingInput> = Vec::new();
    // Android pointer id of the finger that started the current gesture
    // (the shell's primary pointer). None while no gesture is in progress.
    let mut primary_pointer_id: Option<i32> = None;

    // Key-event translation (KeyCharacterMap lookups, dead-key state)
    let mut key_translator = AndroidKeyTranslator::new(app.clone());
    // The soft-keyboard handler is installed once the app shell exists
    let mut soft_keyboard_installed = false;
    // When the next off-screen composition pass may run (see below).
    let mut next_offscreen_update: Option<web_time::Instant> = None;

    // Per-stage frame telemetry (system-property gated; free when off).
    let mut frame_telemetry =
        crate::android_frame_telemetry::AndroidFrameTelemetry::from_system_properties();
    crate::android_frame_telemetry::start_vsync_probe_if_enabled();

    /// How many turns of the loop an unhonoured exit request is retried for.
    /// Enough to ride out a transient JNI thread-attach failure, few enough
    /// that a permanent one is not a call and a log line every frame.
    const MAX_EXIT_ATTEMPTS: u32 = 3;
    let mut exit_attempts = 0u32;

    // Catch-up pacing (kill switch CRANPOSE_CATCHUP_PACING=0 /
    // debug.cranpose.catchup_pacing): when the previous iteration's present
    // landed more than a refresh period after the one before it, the frame
    // already missed its vsync — sleeping until the NEXT choreographer tick
    // would round that miss up to a whole extra period, turning an 18 ms
    // frame into a 33 ms one. Behind-deadline iterations poll immediately
    // instead; `Fifo` present back-pressure paces them. Any iteration that
    // does not present resets to vsync pacing, so an idle app that skips
    // presents can never busy-loop on the zero poll.
    let catchup_pacing = !matches!(std::env::var("CRANPOSE_CATCHUP_PACING").as_deref(), Ok("0"));
    if catchup_pacing {
        log::info!("[pacing] catch-up pacing enabled");
    }
    let mut last_present_at: Option<web_time::Instant> = None;
    let mut behind_deadline = false;

    // Main event loop
    loop {
        let mut frame_timings = crate::android_frame_telemetry::FrameTimings {
            iteration_start_ns: frame_telemetry.now(),
            ..Default::default()
        };
        let pending_confirmation_timeout = pending_host_window_confirmation.map(|pending| {
            android_host_window::HOST_WINDOW_CONFIRMATION_TIMEOUT
                .checked_sub(pending.requested_at.elapsed())
                .unwrap_or(Duration::ZERO)
        });

        // A frame deadline with no surface asks for a frame nothing will draw,
        // and an app with a running animation asks for one every turn of the
        // loop, which is a spun core for as long as the app is off screen.
        let no_surface = gpu_resources.is_none();
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
        let idle_timeout = match no_surface {
            true => earliest_android_poll_timeout(pending_confirmation_timeout, offscreen_timeout),
            false => {
                earliest_android_poll_timeout(pending_confirmation_timeout, frame_deadline_timeout)
            }
        };

        // Vote the display frame rate the way HWUI does: the panel's fastest
        // rate while the user is interacting, the quiet rate while an
        // untouched animation runs, no preference when still. The interaction
        // gate matters for fidelity both ways — Compose runs an untouched
        // infinite animation at the display's quiet rate (measured: its
        // spinning title holds 60 on a 120 Hz panel) and bursts on touch, so
        // a vote tied to animation alone would overshoot it. Without any vote
        // at all, SurfaceFlinger infers a rate from our cadence and pins us
        // via frameRateOverride, choreographer included, so the inferred 60
        // is self-reinforcing and even SF's own touch boost cannot lift it.
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
            // A frame request is satisfied at the next vsync, not immediately.
            // Spinning here is only invisible while every iteration ends in a
            // blocking `Fifo` present; the moment the loop starts skipping
            // presents for frames identical to the one on screen, a zero poll
            // turns an idle app into a busy loop. Fall back to the zero poll
            // when the display cannot wake us, so a device without a
            // choreographer keeps the behaviour it had.
            if behind_deadline {
                // The last present already missed its vsync (see the
                // catch-up pacing note above the loop); every behind
                // iteration presents, so the busy-loop hazard cannot arise.
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
            match event {
                PollEvent::Main(main_event) => match main_event {
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

                        if overlay_window_options.is_none() {
                            if let Some(native_window) = app.native_window() {
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

                                        // Only forward the launcher's initial size to the host
                                        // window in multi-window/freeform modes. A fullscreen
                                        // activity's window already spans the entire display
                                        // (edge-to-edge, including behind the system bars);
                                        // shrinking it with `Window.setLayout` pulls the native
                                        // surface away from the display edges and the uncovered
                                        // strips render as black bands.
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
                                        log::info!("Rendering initialized successfully");
                                    }
                                    Err(error) => {
                                        log::error!("Android rendering initialization failed: {error}");
                                    }
                                }
                            }
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        log::info!("Window terminated");
                        if overlay_window_options.is_none() {
                            gpu_resources = None;
                        }
                    }
                    MainEvent::WindowResized { .. } => {
                        if overlay_window_options.is_none() {
                            if let Some(native_window) = app.native_window() {
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
                                {
                                    if width > 0 && height > 0 {
                                        resources.config.width = width;
                                        resources.config.height = height;
                                        resources
                                            .surface
                                            .configure(&resources.device, &resources.config);

                                        // Set buffer_size to physical pixels
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

                        if let Some(shell) = &mut app_shell {
                            if let Some(actual_size) =
                                update_android_shell_geometry(shell, density, &host_window_registry)
                            {
                                current_host_window_size = actual_size;
                            }
                        }
                    }
                    MainEvent::RedrawNeeded { .. } => {
                        if let Some(shell) = &mut app_shell {
                            shell.mark_dirty();
                        }
                    }
                    MainEvent::Pause => {
                        log::info!("App paused");
                        // Withdraw any outstanding soft-keyboard request and
                        // close the IME editor session so the OS cannot restore
                        // the keyboard on resume for an editor view the
                        // framework no longer considers focused.
                        if let Some(shell) = &mut app_shell {
                            shell.notify_app_paused();
                        }
                        ime_session.ensure_hidden();
                    }
                    MainEvent::Resume { .. } => {
                        log::info!("App resumed");
                        // Never auto-reopen the soft keyboard on resume, even if a
                        // field is still focused: a warm resume keeps the field's
                        // caret/focus but must not resurrect the keyboard (the OS
                        // InputMethodManager would otherwise pop it back). The user
                        // taps the field to bring it back. `notify_app_resumed`
                        // always returns false, so we force the keyboard hidden.
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
                    }
                    MainEvent::Stop => {
                        log::info!("App stopped");
                    }
                    MainEvent::SaveState { .. } => {
                        log::info!("Save state requested");
                    }
                    MainEvent::Destroy => {
                        log::info!("App destroy requested, will exit after this event");
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
                        // Follow OS light/dark switches live (uiMode arrives as
                        // a configuration change).
                        let theme = system_theme_from_android(app.config().ui_mode_night());
                        if android_platform_env().set_system_theme(theme) {
                            if let Some(shell) = &mut app_shell {
                                shell.request_root_render();
                            }
                        }
                        // So does the font-size setting: the platform changes
                        // the configuration rather than restarting the process,
                        // so an app that never re-read it would keep laying
                        // text out at the size the user just moved away from.
                        if crate::android_font_scale::refresh_font_scale(&app) {
                            if let Some(shell) = &mut app_shell {
                                shell.set_font_scale(crate::android_font_scale::font_scale());
                            }
                        }
                    }
                    _ => {}
                },
                _ => {
                    // Non-main poll events do not own Android NativeActivity
                    // input. Native input is delivered as MainEvent::InputAvailable.
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
                            ) {
                                Ok((resources, actual_size)) => {
                                    if let Some(actual_size) = actual_size {
                                        current_host_window_size = actual_size;
                                    }
                                    gpu_resources = Some(resources);
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
                        ) {
                            Ok((resources, actual_size)) => {
                                if let Some(actual_size) = actual_size {
                                    current_host_window_size = actual_size;
                                }
                                gpu_resources = Some(resources);
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
                        gpu_resources = None;
                    }
                }
                android_overlay_window::AndroidOverlayWindowEvent::Pointer { action, x, y } => {
                    let logical = android_platform.pointer_position(x as f64, y as f64);
                    // Overlay pointer events cross a JNI bridge that does not
                    // forward MotionEvent timestamps; velocity tracking falls
                    // back to delivery-time stamping for them.
                    match action {
                        android_overlay_window::AndroidOverlayPointerAction::Down => {
                            pending_inputs.push(PendingInput::PointerDown(
                                logical.x as f32,
                                logical.y as f32,
                                None,
                                // The overlay JNI bridge does not forward tool
                                // type; overlay surfaces are touch in practice.
                                PointerSource::Touch,
                            ));
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Up => {
                            pending_inputs.push(PendingInput::PointerUp(
                                logical.x as f32,
                                logical.y as f32,
                                None,
                                PointerSource::Touch,
                            ));
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Cancel => {
                            pending_inputs.push(PendingInput::PointerCancel);
                        }
                        android_overlay_window::AndroidOverlayPointerAction::Move => {
                            pending_inputs.push(PendingInput::PointerMove(
                                logical.x as f32,
                                logical.y as f32,
                                None,
                                PointerSource::Touch,
                            ));
                        }
                    }
                }
            }
        }

        // Install the soft-keyboard focus hook as soon as the shell exists so
        // the first tap on a text field already opens the keyboard.
        if !soft_keyboard_installed {
            if let Some(shell) = &mut app_shell {
                shell.set_platform_text_input(Rc::new(AndroidSoftKeyboard::new(Rc::clone(
                    &ime_session,
                ))));
                soft_keyboard_installed = true;
                log::info!("Android soft keyboard focus hook installed");

                // The system clipboard is per-AppContext (it backs the text
                // selection menu), so it registers once the shell exists.
                let clipboard_app = app.clone();
                shell.app_context().enter(move || {
                    cranpose_ui::clipboard_session::set_platform_clipboard(Rc::new(
                        crate::android_services::AndroidClipboard { app: clipboard_app },
                    ));
                });

                // Cold-start guard (bug 5): on a fresh launch the OS can restore
                // the soft keyboard left over from the previous process — even on
                // a screen with no text field. Re-request it only if a field is
                // genuinely focused this launch; otherwise force it hidden so the
                // keyboard does not resurrect on relaunch. `notify_app_resumed`
                // returns whether a focused field re-opened it.
                if !shell.notify_app_resumed() {
                    ime_session.ensure_hidden();
                    log::info!("No focused field at launch; ensured soft keyboard hidden");
                }
            }
        }

        // Apply editing operations forwarded by the IME InputConnection
        // (commit/compose/delete/key/editor-action), in arrival order.
        if let Some(shell) = &mut app_shell {
            for (x, y) in crate::android_accessibility::drain_activations() {
                shell.set_cursor(x, y);
                shell.pointer_pressed();
                shell.pointer_released_at_position(x, y);
            }
            // A custom action has no position to synthesise a tap at — it is
            // the screen reader's way to reach a command with no on-screen
            // control — so it is dispatched by identity instead, against the
            // semantics tree as it stands this frame.
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

        // Apply capability signals parked by the Java UI thread (window
        // insets → safe area).
        crate::android_services::apply_pending_platform_signals(
            get_display_density(&app),
            &mut app_shell,
        );

        // Process pending input events outside poll_events to prevent ANR
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
                            // ACTION_UP coordinates carry lift-off roll-back
                            // jitter; they must NOT become a velocity sample
                            // (a synthesized Move here can flip the fling
                            // direction), so release without a Move dispatch.
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
                            // Additional fingers of a multi-touch gesture: touch.
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
                            // Detents -> pixels happens in the shell so the
                            // scroll factor can be refined at runtime.
                            shell.rotary_scrolled_by_detents(detents, uptime_ms);
                        }
                    }
                }
            }
        }

        // Mirror the editor state back to the Java InputConnection after
        // input handling: refreshes the IME's selection view and restarts
        // the input session when the field content diverged from the mirror
        // (e.g. the app transformed or rejected input).
        if ime_session.is_active() {
            if let Some(shell) = &mut app_shell {
                ime_session.sync_editor_state(shell.ime_editor_state());
            }
        }

        // Check if app side requested a frame (animations, state changes)
        if android_frame_driver.take_frame_request() {
            if let Some(shell) = &mut app_shell {
                shell.mark_dirty();
            }
        }

        confirm_android_host_window_request(
            &mut pending_host_window_confirmation,
            current_host_window_size,
        );

        // The app asked to be closed -- `cranpose_services::request_exit`. Not
        // a break: this loop leaving would drop the surface while the activity
        // is still up. `Activity.finish()` asks the platform to tear the
        // activity down, which arrives back here as `MainEvent::Destroy` and
        // takes the ordinary exit below. So the app's request and the system's
        // own back both end the same way.
        //
        // The flag is consumed only when the call lands. Taking it first and
        // ignoring the result means a JNI failure -- a thread that could not
        // attach, most plausibly -- swallows the app's one record that it
        // wanted to close: it stays open, the user's gesture did nothing, and
        // nothing will ever ask again. The retry is bounded so a permanent
        // failure does not become a JNI call and a log line every frame for
        // the rest of the process.
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

        // Check if Destroy event requested exit
        if should_exit.load(Ordering::Relaxed) {
            log::info!("Exiting cleanly after Destroy event");
            break;
        }

        // With the activity off screen the window is gone (`TerminateWindow`
        // drops the GPU resources), so the render block below is skipped and
        // composition stops. Anything the app posted to the UI dispatcher then
        // waits for the user to come back. An app that holds a foreground
        // service has told the OS it has work to finish, so keep composition
        // turning; there is no surface, so nothing is presented.
        //
        // Work waiting on the UI thread earns a pass. A running animation does
        // not: nothing is drawn, and paying for a composition per animation
        // frame off screen is how an app burns a core behind the user's back.
        // One follow-up pass is armed after a pass that carried work, so a
        // recomposition that work asked for still runs.
        let offscreen_due = next_offscreen_update.is_some_and(|at| at <= web_time::Instant::now());
        if offscreen && (offscreen_pending_ui || offscreen_due) {
            if let Some(shell) = &mut app_shell {
                if shell.needs_update() {
                    android_host_window::with_android_host_window_registry(
                        &host_window_registry,
                        || shell.update(),
                    );
                }
            }
            next_offscreen_update = match offscreen_pending_ui {
                true => Some(web_time::Instant::now() + OFFSCREEN_UPDATE_PERIOD),
                false => None,
            };
        }

        // Render outside event callback if needed
        if let (Some(resources), Some(shell)) = (&mut gpu_resources, &mut app_shell) {
            if shell.needs_update() {
                let update_result = android_host_window::with_android_host_window_registry(
                    &host_window_registry,
                    || shell.update(),
                );
                frame_timings.after_update_ns = frame_telemetry.now();
                if let Err(error) = crate::android_accessibility::sync(
                    &app,
                    shell,
                    android_platform.scale_factor() as f32,
                    &mut accessibility_elements,
                    &mut accessibility_revision,
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
                    if render_once(resources, shell, &mut frame_telemetry, &mut frame_timings) {
                        break; // Out of memory, exit
                    }
                } else {
                    frame_telemetry.note_idle_iteration();
                }
            } else {
                frame_telemetry.note_idle_iteration();
            }
        } else {
            frame_telemetry.note_idle_iteration();
        }

        // Catch-up pacing bookkeeping (see the note above the loop):
        // `after_present_ns` is stamped only by a successful present, and
        // `FrameTimings` is fresh per iteration, so a nonzero value is the
        // exact "this iteration presented" signal.
        if frame_timings.after_present_ns != 0 {
            let now = web_time::Instant::now();
            behind_deadline = catchup_pacing
                && last_present_at.is_some_and(|previous| {
                    // Display-reported period first (vsync-probe builds),
                    // then the always-on estimate from consecutive
                    // choreographer callback deltas, then the 60 Hz default.
                    let reported = crate::android_frame_telemetry::vsync_period_ns();
                    let period = if reported > 0 {
                        reported
                    } else {
                        crate::android_vsync::observed_vsync_period_ns().unwrap_or(16_666_667)
                    };
                    // period/16 of slack (~1 ms at 60 Hz) keeps at-rate
                    // presentation jitter from triggering catch-up.
                    now.duration_since(previous).as_nanos() as i64 > period + period / 16
                });
            last_present_at = Some(now);
        } else {
            behind_deadline = false;
        }
    }
}
