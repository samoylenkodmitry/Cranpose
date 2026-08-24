//! Android IME bridge: real `InputConnection` support for text fields.
//!
//! A `NativeActivity` has no Java view overriding `onCreateInputConnection`,
//! so IMEs run in fallback mode and can only deliver plain key events (the
//! [`crate::android_keyboard`] `KeyCharacterMap` path). This module drives the
//! `dev.cranpose.android.CranposeTextInput` Java helper, which attaches an
//! invisible focusable view to the activity and returns a
//! `BaseInputConnection` subclass when the soft keyboard opens. Editing
//! operations flow back through the JNI callbacks below into an event queue
//! drained by the Android main loop:
//!
//! - `commitText` → [`AndroidImeEvent::CommitText`] → `AppShell::on_paste`
//! - `setComposingText` → [`AndroidImeEvent::SetComposingText`] →
//!   `AppShell::on_ime_preedit`
//! - `setComposingRegion` → [`AndroidImeEvent::SetComposingRegion`] →
//!   `AppShell::on_ime_set_composing_region`
//! - `finishComposingText` → [`AndroidImeEvent::FinishComposing`] →
//!   `AppShell::on_ime_finish_composing`
//! - `deleteSurroundingText` → [`AndroidImeEvent::DeleteSurrounding`] →
//!   `AppShell::on_ime_delete_surrounding`
//! - `sendKeyEvent` → [`AndroidImeEvent::Key`] → `AppShell::on_key_event`
//! - `performEditorAction` → [`AndroidImeEvent::EditorAction`]
//!
//! The Java side keeps a UTF-16 `Editable` mirror so IME text queries are
//! answered synchronously; the Rust text field remains the source of truth
//! and pushes its state back through [`update_android_text_input_state`]
//! (selection updates, or an input-session restart when the contents
//! diverged). All boundary offsets are converted at this JNI layer: Rust
//! works in UTF-8 bytes, Java in UTF-16 code units.
#![allow(unsafe_code)]

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use android_activity::AndroidAppWaker;
use cranpose_app_shell::ImeEditorState;
use jni::{
    jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::{jint, jlong},
    EnvUnowned, Outcome,
};

use crate::android_jni::{
    clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
};

const TEXT_INPUT_CLASS: &str = "dev/cranpose/android/CranposeTextInput";

/// One editing operation forwarded from the Java `InputConnection`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AndroidImeEvent {
    /// `commitText`: replace the composing text (if any) with `text` and move
    /// the cursor after it. `new_cursor_position` follows the Android
    /// contract; values other than 1 are currently treated as 1 (cursor
    /// after the committed text) and self-correct through the state sync.
    CommitText {
        text: String,
        new_cursor_position: i32,
    },
    /// `setComposingText`: replace the current preedit with `text`;
    /// `cursor_bytes` is the caret offset inside `text` in UTF-8 bytes.
    SetComposingText { text: String, cursor_bytes: usize },
    /// `setComposingRegion`: mark existing text as composing (UTF-8 bytes).
    SetComposingRegion {
        start_bytes: usize,
        end_bytes: usize,
    },
    /// `setSelection`: move the selection/caret without editing text (UTF-8
    /// bytes). Gboard's spacebar-swipe cursor control scrubs the caret this way.
    SetSelection {
        start_bytes: usize,
        end_bytes: usize,
    },
    /// `finishComposingText`: keep the composed text, clear the region.
    FinishComposing,
    /// `deleteSurroundingText`, already converted to UTF-8 byte counts.
    DeleteSurrounding {
        before_bytes: usize,
        after_bytes: usize,
    },
    /// `sendKeyEvent`: editing keys some IMEs deliver as key events
    /// (backspace, enter). Raw Android values; translated by
    /// [`crate::android_keyboard::ime_key_event`].
    Key {
        action: i32,
        key_code: i32,
        meta_state: i32,
        unicode_char: i32,
    },
    /// `performEditorAction` with an `EditorInfo.IME_ACTION_*` code.
    EditorAction { action: i32 },
    /// The on-screen keyboard's height (physical px) at the bottom of the
    /// window changed. `0` when the keyboard is hidden. Forwarded from a Java
    /// `View.OnApplyWindowInsetsListener` / visible-frame listener so the shell
    /// can expose it as IME insets to composition.
    ImeInsetsChanged { bottom_px: i32 },
}

/// Cross-thread queue for IME events (pushed on the Android UI thread,
/// drained by the native main loop). Pushes wake the main-loop looper so
/// events are handled without waiting for the next frame.
pub(crate) struct AndroidImeEventQueue {
    events: Mutex<Vec<AndroidImeEvent>>,
    waker: OnceLock<AndroidAppWaker>,
}

impl AndroidImeEventQueue {
    pub(crate) fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
            waker: OnceLock::new(),
        }
    }

    /// Installs the main-loop waker. Events pushed before this are delivered
    /// on the next natural poll.
    pub(crate) fn set_waker(&self, waker: AndroidAppWaker) {
        let _ = self.waker.set(waker);
    }

    pub(crate) fn push(&self, event: AndroidImeEvent) {
        self.lock_events().push(event);
        if let Some(waker) = self.waker.get() {
            waker.wake();
        }
    }

    pub(crate) fn drain(&self) -> Vec<AndroidImeEvent> {
        std::mem::take(&mut *self.lock_events())
    }

    fn lock_events(&self) -> MutexGuard<'_, Vec<AndroidImeEvent>> {
        self.events
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Opaque `jlong` wrapper over a retained `Arc<AndroidImeEventQueue>`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AndroidImeQueueHandle(jlong);

impl AndroidImeQueueHandle {
    fn raw(self) -> jlong {
        self.0
    }
}

fn retain_ime_queue_handle(queue: &Arc<AndroidImeEventQueue>) -> AndroidImeQueueHandle {
    let pointer = Arc::into_raw(Arc::clone(queue));
    AndroidImeQueueHandle(pointer as usize as jlong)
}

fn release_ime_queue_handle_raw(handle: jlong) {
    if handle == 0 {
        return;
    }
    // SAFETY: handles are created by `retain_ime_queue_handle` (Arc::into_raw)
    // and released exactly once: by the Java helper when the session is
    // disposed, or by Rust when `show` failed to transfer ownership.
    unsafe {
        drop(Arc::from_raw(
            handle as usize as *const AndroidImeEventQueue,
        ));
    }
}

fn push_ime_event_for_handle(handle: jlong, event: AndroidImeEvent) {
    if handle == 0 {
        log::warn!("dropped Android IME event because the Java callback carried no queue handle");
        return;
    }
    // SAFETY: the Java helper stores a handle retained from an
    // Arc<AndroidImeEventQueue> and releases it only when the session is
    // disposed on the Android UI thread; callbacks check `disposed` first.
    let Some(queue) = (unsafe { (handle as usize as *const AndroidImeEventQueue).as_ref() }) else {
        log::warn!("dropped Android IME event because the queue handle was invalid");
        return;
    };
    queue.push(event);
}

/// Converts a UTF-8 byte offset into `text` to UTF-16 code units, clamping
/// into the valid range (and down to a character boundary).
fn utf16_offset(text: &str, byte_offset: usize) -> i32 {
    let mut offset = byte_offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    text[..offset].encode_utf16().count() as i32
}

/// Opens (or reseeds) the Java text-input session for the focused field.
///
/// Transfers one retained queue handle to Java on success; on failure the
/// handle is released here. Errors are returned so the caller can fall back
/// to the plain `show_soft_input` path (for apps that do not ship the
/// `cranpose/android/java` sources).
pub(crate) fn show_android_text_input(
    app: &android_activity::AndroidApp,
    queue: &Arc<AndroidImeEventQueue>,
    state: &ImeEditorState,
) -> Result<(), String> {
    let handle = retain_ime_queue_handle(queue);
    let sel_start = utf16_offset(&state.text, state.selection_start);
    let sel_end = utf16_offset(&state.text, state.selection_end);

    let result = with_android_activity_env(app, |env, activity| {
        let class = load_cranpose_java_class(env, &activity, TEXT_INPUT_CLASS)?;
        let text = env.new_string(&state.text).map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to create Android IME text string: {error}")
        })?;
        let text = JObject::from(text);
        env.call_static_method(
            class,
            jni_str!("show"),
            jni_sig!("(Landroid/app/Activity;JLjava/lang/String;IIZ)I"),
            &[
                JValue::Object(&activity),
                JValue::Long(handle.raw()),
                JValue::Object(&text),
                JValue::Int(sel_start),
                JValue::Int(sel_end),
                JValue::Bool(state.single_line),
            ],
        )
        .and_then(|value| value.i())
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to show Android text input: {error}")
        })
    });

    match result {
        Ok(0) => Ok(()),
        Ok(code) => {
            release_ime_queue_handle_raw(handle.raw());
            Err(format!("Android text input helper returned {code}"))
        }
        Err(error) => {
            release_ime_queue_handle_raw(handle.raw());
            Err(error)
        }
    }
}

/// Closes the Java text-input session (hides the IME, detaches the editor
/// view, releases the queue handle owned by the session).
pub(crate) fn hide_android_text_input(app: &android_activity::AndroidApp) -> Result<(), String> {
    with_android_activity_env(app, |env, activity| {
        let class = load_cranpose_java_class(env, &activity, TEXT_INPUT_CLASS)?;
        env.call_static_method(
            class,
            jni_str!("hide"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(&activity)],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to hide Android text input: {error}")
        })?;
        Ok(())
    })
}

/// Pushes the Rust-side editor state into the Java mirror. Java refreshes the
/// IME's selection view, or restarts the input session when the text content
/// diverged from the mirror.
pub(crate) fn update_android_text_input_state(
    app: &android_activity::AndroidApp,
    state: &ImeEditorState,
) -> Result<(), String> {
    let sel_start = utf16_offset(&state.text, state.selection_start);
    let sel_end = utf16_offset(&state.text, state.selection_end);
    let (comp_start, comp_end) = match state.composition {
        Some((start, end)) => (
            utf16_offset(&state.text, start),
            utf16_offset(&state.text, end),
        ),
        None => (-1, -1),
    };

    with_android_activity_env(app, |env, activity| {
        let class = load_cranpose_java_class(env, &activity, TEXT_INPUT_CLASS)?;
        let text = env.new_string(&state.text).map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to create Android IME text string: {error}")
        })?;
        let text = JObject::from(text);
        env.call_static_method(
            class,
            jni_str!("updateState"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;IIII)V"),
            &[
                JValue::Object(&activity),
                JValue::Object(&text),
                JValue::Int(sel_start),
                JValue::Int(sel_end),
                JValue::Int(comp_start),
                JValue::Int(comp_end),
            ],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to update Android text input state: {error}")
        })?;
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// JNI callbacks from dev.cranpose.android.CranposeTextInput (UI thread)
// ---------------------------------------------------------------------------

fn jstring_to_string(env: &mut EnvUnowned<'_>, text: JString<'_>) -> Option<String> {
    match env
        .with_env(|env| -> jni::errors::Result<String> { text.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(text) => Some(text),
        Outcome::Err(_) | Outcome::Panic(_) => {
            log::warn!("failed to read Android IME text from JNI");
            None
        }
    }
}

fn non_negative(value: jint) -> usize {
    value.max(0) as usize
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeCommitText<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    queue_handle: jlong,
    text: JString<'local>,
    new_cursor_position: jint,
) {
    let Some(text) = jstring_to_string(&mut env, text) else {
        return;
    };
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::CommitText {
            text,
            new_cursor_position,
        },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeSetComposingText<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    queue_handle: jlong,
    text: JString<'local>,
    cursor_bytes: jint,
) {
    let Some(text) = jstring_to_string(&mut env, text) else {
        return;
    };
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::SetComposingText {
            text,
            cursor_bytes: non_negative(cursor_bytes),
        },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeSetComposingRegion(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    start_bytes: jint,
    end_bytes: jint,
) {
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::SetComposingRegion {
            start_bytes: non_negative(start_bytes),
            end_bytes: non_negative(end_bytes),
        },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeSetSelection(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    start_bytes: jint,
    end_bytes: jint,
) {
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::SetSelection {
            start_bytes: non_negative(start_bytes),
            end_bytes: non_negative(end_bytes),
        },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeFinishComposing(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
) {
    push_ime_event_for_handle(queue_handle, AndroidImeEvent::FinishComposing);
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeDeleteSurrounding(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    before_bytes: jint,
    after_bytes: jint,
) {
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::DeleteSurrounding {
            before_bytes: non_negative(before_bytes),
            after_bytes: non_negative(after_bytes),
        },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeSendKeyEvent(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    action: jint,
    key_code: jint,
    meta_state: jint,
    unicode_char: jint,
) {
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::Key {
            action,
            key_code,
            meta_state,
            unicode_char,
        },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImePerformEditorAction(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    action: jint,
) {
    push_ime_event_for_handle(queue_handle, AndroidImeEvent::EditorAction { action });
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeInsetsChanged(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    bottom_px: jint,
) {
    push_ime_event_for_handle(
        queue_handle,
        AndroidImeEvent::ImeInsetsChanged { bottom_px },
    );
}

#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeReleaseQueue(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
) {
    release_ime_queue_handle_raw(queue_handle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offset_converts_and_clamps() {
        // "aé漢x" - 'a'=1 byte/1 unit, 'é'=2 bytes/1 unit, '漢'=3 bytes/1 unit.
        let text = "a\u{00e9}\u{6f22}x";
        assert_eq!(utf16_offset(text, 0), 0);
        assert_eq!(utf16_offset(text, 1), 1);
        assert_eq!(utf16_offset(text, 3), 2);
        assert_eq!(utf16_offset(text, 6), 3);
        assert_eq!(utf16_offset(text, 7), 4);
        // Past the end clamps to the full length.
        assert_eq!(utf16_offset(text, 100), 4);
        // Mid-character offsets clamp down to the previous boundary.
        assert_eq!(utf16_offset(text, 2), 1);
        assert_eq!(utf16_offset(text, 4), 2);
        // Supplementary-plane characters count as two UTF-16 units.
        let emoji = "\u{1f600}b";
        assert_eq!(utf16_offset(emoji, 4), 2);
        assert_eq!(utf16_offset(emoji, 5), 3);
    }

    #[test]
    fn queue_push_and_drain() {
        let queue = AndroidImeEventQueue::new();
        queue.push(AndroidImeEvent::FinishComposing);
        queue.push(AndroidImeEvent::CommitText {
            text: "hi".to_string(),
            new_cursor_position: 1,
        });
        let events = queue.drain();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], AndroidImeEvent::FinishComposing);
        assert!(queue.drain().is_empty());
    }
}
