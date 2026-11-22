//! The system keyboard on Android: `CranposeInputMethodService` hands its
//! input view's surface here, and the board draws into it on the GPU.
//!
//! Everything runs on the main thread, where the service lives: the surface
//! callbacks, the `Choreographer` frames, the touches and the calls back into
//! the service. Only the frame waker may run elsewhere; it posts to the main
//! thread through the service.
#![expect(unsafe_code, reason = "JNI and NDK calls into the Android system")]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
    time::Instant,
};

use cranpose_app_shell::{TouchRouter, TouchSample};
use cranpose_core::{MutableState, rememberMutableStateOf};
use cranpose_ui::{EdgeInsets, PointerSource, Size, local_safe_area_insets};
use jni::{
    EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JValue},
    sys::{jboolean, jfloat, jint, jlong},
};
use ndk::native_window::NativeWindow;

use super::surface::{AndroidSurfaceError, create_android_wgpu_surface};
use crate::{
    AppSettings, GpuContext, GuestView,
    input_method::{EditorInfo, InputConnection, InputMethod, InputSession},
    wgpu_surface::SurfaceFrame,
};

const SERVICE_CLASS: &str = "dev/cranpose/android/CranposeInputMethodService";

// The keyboard's part of the main thread's state.
#[derive(Default)]
pub(crate) struct InputMethodState {
    // The settings of a process that has a keyboard, for the service.
    settings: RefCell<Option<AppSettings>>,
    host: RefCell<Option<Host>>,
    // The focused field and its turn with the keyboard.
    field: Published<Field>,
    // The edges of the board the navigation bar draws over.
    insets: Published<EdgeInsets>,
}

fn with_state<R>(f: impl FnOnce(&InputMethodState) -> R) -> R {
    crate::thread_state::with(|thread| f(&thread.input_method))
}

fn with_settings<R>(f: impl FnOnce(&RefCell<Option<AppSettings>>) -> R) -> R {
    with_state(|state| f(&state.settings))
}

fn with_host<R>(f: impl FnOnce(&RefCell<Option<Host>>) -> R) -> R {
    with_state(|state| f(&state.host))
}

// Keeps the settings of a process that has a keyboard, for the service.
pub(crate) fn install(settings: AppSettings) {
    if settings.input_method.is_some() {
        with_settings(|slot| *slot.borrow_mut() = Some(settings));
    }
}

fn input_method() -> Option<InputMethod> {
    with_settings(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|settings| settings.input_method)
    })
}

// A value the service sets and the composition reads as state.
#[derive(Clone, Default)]
struct Published<T: Copy + 'static> {
    value: Rc<Cell<T>>,
    state: Rc<Cell<Option<MutableState<T>>>>,
}

impl<T: Copy + PartialEq + 'static> Published<T> {
    fn set(&self, value: T) {
        self.value.set(value);
        if let Some(state) = self.state.get()
            && state.get_non_reactive() != value
        {
            state.set(value);
        }
    }

    // The state of the value in the composition that calls it.
    fn state(&self) -> MutableState<T> {
        let initial = self.value.get();
        let state = rememberMutableStateOf(|| initial);
        self.state.set(Some(state));
        state
    }
}

struct Target {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    // The surface draws into this window, so it lives as long as the surface.
    window: NativeWindow,
}

struct Host {
    gpu: GpuContext,
    target: Option<Target>,
    // The format the view draws in, the display view of the surface's format.
    format: wgpu::TextureFormat,
    view: GuestView,
    touches: TouchRouter<i32>,
    density: f32,
}

#[derive(Debug, thiserror::Error)]
enum HostError {
    #[error(transparent)]
    Surface(#[from] AndroidSurfaceError),
    #[error("the keyboard's view: {0}")]
    View(#[from] crate::GuestViewError),
    #[error("the application names no keyboard: AppLauncher::with_input_method")]
    NoKeyboard,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Field {
    editor: EditorInfo,
    session: InputSession,
}

fn root(content: fn()) -> impl FnMut() + 'static {
    let (field, insets) = with_state(|state| (state.field.clone(), state.insets.clone()));
    move || {
        let field = field.state().get();
        let insets = insets.state();
        cranpose_core::CompositionLocalProvider(
            [local_safe_area_insets().provides_computed(move |_| insets.get())],
            || {
                crate::input_method::provide(
                    InputConnection::new(field.editor, field.session),
                    content,
                );
            },
        );
    }
}

fn surface_changed(
    native_window: NativeWindow,
    width: u32,
    height: u32,
    density: f32,
) -> Result<(), HostError> {
    let Some(keyboard) = input_method() else {
        return Err(HostError::NoKeyboard);
    };
    with_host(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some(host) = slot.as_mut() {
            match host.target.as_mut() {
                // The same window at a new size keeps its surface: a window
                // takes one producer at a time.
                Some(target) if target.window.ptr() == native_window.ptr() => {
                    target.config.width = width;
                    target.config.height = height;
                    target.surface.configure(&host.gpu.device, &target.config);
                    host.view.resize(width, height, density)?;
                    host.view.set_visible(true);
                    host.density = density;
                    return Ok(());
                }
                // A new window: the old surface lets go of its window first.
                Some(_) => host.target = None,
                None => {}
            }
        }
        let gpu = match slot.as_ref() {
            Some(host) => host.gpu.clone(),
            None => new_gpu()?,
        };
        let surface = create_android_wgpu_surface(&gpu.instance, native_window.ptr().cast())?;
        let config = super::event_loop::create_android_surface_config(
            &surface,
            &gpu.adapter,
            width,
            height,
        )?;
        surface.configure(&gpu.device, &config);
        let view_format = crate::surface_format::display_surface_view_format(config.format);
        let target = Target {
            surface,
            config,
            window: native_window,
        };
        match slot.as_mut() {
            Some(host) if host.format == view_format => {
                host.view.resize(width, height, density)?;
                host.view.set_visible(true);
                host.target = Some(target);
                host.density = density;
            }
            _ => {
                let mut view = with_settings(|settings| {
                    let settings = settings.borrow();
                    let settings = settings.as_ref().ok_or(HostError::NoKeyboard)?;
                    GuestView::new(
                        settings,
                        &gpu,
                        view_format,
                        width,
                        height,
                        density,
                        root(keyboard.content),
                    )
                    .map_err(HostError::from)
                })?;
                view.shell().set_frame_waker(request_frame);
                *slot = Some(Host {
                    gpu,
                    target: Some(target),
                    format: view_format,
                    view,
                    touches: TouchRouter::default(),
                    density,
                });
            }
        }
        Ok(())
    })
}

fn new_gpu() -> Result<GpuContext, HostError> {
    let backend = with_settings(|settings| {
        settings
            .borrow()
            .as_ref()
            .map_or_else(Default::default, |settings| settings.android_gpu_backend)
    });
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = backend.wgpu_backends();
    descriptor.flags = wgpu::InstanceFlags::empty();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .map_err(AndroidSurfaceError::from)?;
    let (device, queue) =
        super::device::request_device(&adapter).map_err(AndroidSurfaceError::from)?;
    Ok(GpuContext {
        instance,
        adapter,
        device: Arc::new(device),
        queue: Arc::new(queue),
    })
}

// Draws a frame when the board changed and says when the next one is due:
// `0` at the next display frame, a positive count of milliseconds, or `-1`
// when the board waits for input.
fn frame() -> i64 {
    with_host(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(host) = slot.as_mut() else {
            return -1;
        };
        let owed = host
            .target
            .as_mut()
            .is_some_and(|target| draw(&mut host.view, target, &host.gpu));
        let schedule = host.view.frame_schedule();
        if owed || schedule.needs_frame || schedule.needs_update {
            0
        } else {
            schedule.next_deadline.map_or(-1, |deadline| {
                let wait = deadline.saturating_duration_since(Instant::now());
                i64::try_from(wait.as_millis()).unwrap_or(i64::MAX).max(1)
            })
        }
    })
}

// Draws the board when it changed. Returns whether it still owes a frame,
// because the surface gave no image this time.
fn draw(view: &mut GuestView, target: &mut Target, gpu: &GpuContext) -> bool {
    if !view.update() {
        return false;
    }
    let frame = match crate::wgpu_surface::current_surface_texture(&target.surface, "keyboard") {
        SurfaceFrame::Ready(frame) => frame,
        SurfaceFrame::Reconfigure => {
            target.surface.configure(&gpu.device, &target.config);
            return true;
        }
        SurfaceFrame::Skip => return true,
    };
    if let Err(error) = view.draw_surface(frame) {
        log::error!("the keyboard did not draw: {error}");
    }
    false
}

fn touch(pointer: i32, phase: i32, x: f32, y: f32) {
    with_host(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(host) = slot.as_mut() else {
            return;
        };
        let sample = TouchSample::new(x / host.density, y / host.density);
        let shell = host.view.shell();
        shell.set_pointer_source(PointerSource::Touch);
        let finger = Some(pointer);
        match phase {
            0 => {
                host.touches.press(shell, finger, sample);
            }
            1 => {
                host.touches.moved(shell, finger, sample);
            }
            2 => {
                host.touches.release(shell, finger, sample);
            }
            _ => {
                host.touches
                    .cancel(shell, finger, Some((sample.x, sample.y)));
            }
        }
    });
}

// Asks the service for a frame. Any thread may call it.
fn request_frame() {
    call_service(|env, class| {
        env.call_static_method(class, jni_str!("requestFrame"), jni_sig!("()V"), &[])
            .map(|_| ())
    });
}

pub(crate) fn commit_text(text: &str) {
    call_service(|env, class| {
        let text = env.new_string(text)?;
        env.call_static_method(
            class,
            jni_str!("commitText"),
            jni_sig!("(Ljava/lang/String;)V"),
            &[JValue::Object(text.as_ref())],
        )
        .map(|_| ())
    });
}

pub(crate) fn delete_backward() {
    call_service(|env, class| {
        env.call_static_method(class, jni_str!("deleteBackward"), jni_sig!("()V"), &[])
            .map(|_| ())
    });
}

pub(crate) fn perform_ime_action() {
    call_service(|env, class| {
        env.call_static_method(class, jni_str!("performImeAction"), jni_sig!("()V"), &[])
            .map(|_| ())
    });
}

pub(crate) fn switch_to_next_keyboard() {
    call_service(|env, class| {
        env.call_static_method(
            class,
            jni_str!("switchToNextKeyboard"),
            jni_sig!("()V"),
            &[],
        )
        .map(|_| ())
    });
}

fn call_service(
    run: impl for<'local> FnOnce(&mut jni::Env<'local>, JClass<'local>) -> jni::errors::Result<()>,
) {
    let Some(context) = super::process::context() else {
        log::warn!("the keyboard has no application context");
        return;
    };
    if let Err(error) = context.call_class(SERVICE_CLASS, |env, _context, class| run(env, class)) {
        log::warn!("the keyboard's service did not answer: {error}");
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeSurfaceChanged<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    surface: JObject<'local>,
    width: jint,
    height: jint,
    density: jfloat,
) {
    let window = env
        .with_env(|env| -> jni::errors::Result<Result<NativeWindow, String>> {
            Ok(super::jni::native_window_from_surface(env, surface))
        })
        .into_outcome();
    let Outcome::Ok(Ok(window)) = window else {
        log::error!("the keyboard's surface has no native window");
        return;
    };
    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
        return;
    };
    if width == 0 || height == 0 {
        return;
    }
    if let Err(error) = surface_changed(window, width, height, density) {
        log::error!("the keyboard cannot draw: {error}");
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeSurfaceDestroyed(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    with_host(|slot| {
        if let Some(host) = slot.borrow_mut().as_mut() {
            host.view.set_visible(false);
            host.target = None;
        }
    });
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeFrame(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    _frame_time_nanos: jlong,
) -> jlong {
    frame()
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeTouch(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    pointer: jint,
    phase: jint,
    x: jfloat,
    y: jfloat,
) {
    touch(pointer, phase, x, y);
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeEditor(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    input_type: jint,
    ime_options: jint,
    capitalize_next: jboolean,
    start: jboolean,
) {
    let editor = EditorInfo {
        capitalize_next,
        ..super::editor_info_wire::editor_info(input_type, ime_options)
    };
    with_state(|state| {
        let session = state.field.value.get().session;
        state.field.set(Field {
            editor,
            session: if start { session.next() } else { session },
        });
    });
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeInsets(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
    density: jfloat,
) {
    let logical = |pixels: jint| pixels as f32 / density;
    with_state(|state| {
        state.insets.set(EdgeInsets {
            left: logical(left),
            top: logical(top),
            right: logical(right),
            bottom: logical(bottom),
        });
    });
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeVisible(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    visible: jboolean,
) {
    with_host(|slot| {
        if let Some(host) = slot.borrow_mut().as_mut() {
            host.view.set_visible(visible && host.target.is_some());
        }
    });
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativePreferredHeight(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    screen_width: jfloat,
    screen_height: jfloat,
) -> jfloat {
    input_method().map_or(260.0, |keyboard| {
        (keyboard.height)(Size {
            width: screen_width,
            height: screen_height,
        })
    })
}
