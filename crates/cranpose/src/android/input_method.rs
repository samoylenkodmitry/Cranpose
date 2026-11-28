//! The system keyboard on Android: `CranposeInputMethodService` hands its
//! input view's surface here, and the board draws into it on the GPU.
//!
//! Everything runs on the main thread, where the service lives: the surface
//! callbacks, the `Choreographer` frames, the touches and the calls back into
//! the service. Only the frame waker may run elsewhere; it posts to the main
//! thread through the service.
#![expect(unsafe_code, reason = "JNI and NDK calls into the Android system")]

use cranpose_ui::{EdgeInsets, Size};
use jni::{
    EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::{jboolean, jfloat, jint, jlong},
};
use ndk::native_window::NativeWindow;

use super::surface::{AndroidSurfaceError, create_android_wgpu_surface};
use crate::{
    AppSettings, GpuContext,
    input_method::EditorInfo,
    keyboard_host::{FrameRequest, KeyboardState, KeyboardWindow, TouchPhase},
};

const SERVICE_CLASS: &str = "dev/cranpose/android/CranposeInputMethodService";

// The service's input view surface.
pub(crate) struct AndroidWindow(NativeWindow);

impl KeyboardWindow for AndroidWindow {
    type Error = AndroidSurfaceError;

    fn same(&self, other: &Self) -> bool {
        self.0.ptr() == other.0.ptr()
    }

    fn instance(settings: &AppSettings) -> wgpu::Instance {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = settings.android_gpu_backend.wgpu_backends();
        descriptor.flags = wgpu::InstanceFlags::empty();
        wgpu::Instance::new(descriptor)
    }

    fn device(adapter: &wgpu::Adapter) -> Result<(wgpu::Device, wgpu::Queue), AndroidSurfaceError> {
        Ok(super::device::request_device(adapter)?)
    }

    fn surface(
        &self,
        gpu: &GpuContext,
        (width, height): (u32, u32),
    ) -> Result<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration), AndroidSurfaceError> {
        let surface = create_android_wgpu_surface(&gpu.instance, self.0.ptr().cast())?;
        let config = super::event_loop::create_android_surface_config(
            &surface,
            &gpu.adapter,
            width,
            height,
        )?;
        surface.configure(&gpu.device, &config);
        Ok((surface, config))
    }

    fn request_frame() {
        call_service(|env, class| {
            env.call_static_method(class, jni_str!("requestFrame"), jni_sig!("()V"), &[])
                .map(|_| ())
        });
    }
}

fn with_keyboard<R>(f: impl FnOnce(&KeyboardState<AndroidWindow>) -> R) -> R {
    crate::thread_state::with(|thread| f(&thread.input_method))
}

// Keeps the settings of a process that has a keyboard, for the service.
pub(crate) fn install(settings: AppSettings) {
    with_keyboard(|keyboard| keyboard.install(settings));
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

pub(crate) fn text_before_cursor(max: usize) -> Option<String> {
    let max = jint::try_from(max).unwrap_or(jint::MAX);
    call_service(|env, class| {
        let value = env
            .call_static_method(
                class,
                jni_str!("textBeforeCursor"),
                jni_sig!("(I)Ljava/lang/String;"),
                &[JValue::Int(max)],
            )
            .and_then(jni::JValueOwned::l)?;
        if value.is_null() {
            return Ok(None);
        }
        JString::cast_local(env, value)?
            .try_to_string(env)
            .map(Some)
    })
    .flatten()
}

pub(crate) fn delete_surrounding_text(before: usize, after: usize) {
    let before = jint::try_from(before).unwrap_or(jint::MAX);
    let after = jint::try_from(after).unwrap_or(jint::MAX);
    call_service(|env, class| {
        env.call_static_method(
            class,
            jni_str!("deleteSurroundingText"),
            jni_sig!("(II)V"),
            &[JValue::Int(before), JValue::Int(after)],
        )
        .map(|_| ())
    });
}

pub(crate) fn move_cursor(offset: i32) {
    call_service(|env, class| {
        env.call_static_method(
            class,
            jni_str!("moveCursor"),
            jni_sig!("(I)V"),
            &[JValue::Int(offset)],
        )
        .map(|_| ())
    });
}

pub(crate) fn hide() {
    call_service(|env, class| {
        env.call_static_method(class, jni_str!("hide"), jni_sig!("()V"), &[])
            .map(|_| ())
    });
}

fn call_service<T>(
    run: impl for<'local> FnOnce(&mut jni::Env<'local>, JClass<'local>) -> jni::errors::Result<T>,
) -> Option<T> {
    let Some(context) = super::process::context() else {
        log::warn!("the keyboard has no application context");
        return None;
    };
    match context.call_class(SERVICE_CLASS, |env, _context, class| run(env, class)) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!("the keyboard's service did not answer: {error}");
            None
        }
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
    let changed = with_keyboard(|keyboard| {
        keyboard.surface_changed(AndroidWindow(window), (width, height), density)
    });
    if let Err(error) = changed {
        log::error!("the keyboard cannot draw: {error}");
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeSurfaceDestroyed(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
) {
    with_keyboard(KeyboardState::surface_destroyed);
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativeFrame(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    _frame_time_nanos: jlong,
) -> jlong {
    // `0` asks for the next display frame, a positive count of milliseconds
    // for a frame after that long, and `-1` for none until input comes.
    match with_keyboard(KeyboardState::frame) {
        FrameRequest::Next => 0,
        FrameRequest::After(wait) => i64::try_from(wait.as_millis()).unwrap_or(i64::MAX).max(1),
        FrameRequest::Idle => -1,
    }
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
    let phase = match phase {
        0 => TouchPhase::Down,
        1 => TouchPhase::Move,
        2 => TouchPhase::Up,
        _ => TouchPhase::Cancel,
    };
    // Android reuses a pointer id once its finger lifts.
    let pointer = u64::from(pointer.cast_unsigned());
    with_keyboard(|keyboard| keyboard.touch(pointer, phase, x, y));
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
    with_keyboard(|keyboard| {
        if start {
            keyboard.start_input(editor);
        } else {
            keyboard.update_editor(editor);
        }
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
    with_keyboard(|keyboard| {
        keyboard.set_insets(EdgeInsets {
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
    with_keyboard(|keyboard| keyboard.set_visible(visible));
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub(crate) extern "system" fn Java_dev_cranpose_android_CranposeInputMethodService_nativePreferredHeight(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    screen_width: jfloat,
    screen_height: jfloat,
) -> jfloat {
    with_keyboard(|keyboard| {
        keyboard.height(Size {
            width: screen_width,
            height: screen_height,
        })
    })
    .unwrap_or(260.0)
}
