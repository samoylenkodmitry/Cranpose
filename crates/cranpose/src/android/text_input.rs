#![expect(unsafe_code, reason = "JNI and NDK calls into the Android system")]

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use android_activity::AndroidAppWaker;
use cranpose_app_shell::ImeEditorState;
use cranpose_ui::{ContentType, ImeAction, KeyboardCapitalization, KeyboardOptions, KeyboardType};
use jni::{
    EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::{jint, jlong},
};

use super::jni::{
    clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
};

const TEXT_INPUT_CLASS: &str = "dev/cranpose/android/CranposeTextInput";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AndroidImeEvent {
    CommitText {
        text: String,
        new_cursor_position: i32,
    },
    SetComposingText {
        text: String,
        cursor_bytes: usize,
    },
    SetComposingRegion {
        start_bytes: usize,
        end_bytes: usize,
    },
    SetSelection {
        start_bytes: usize,
        end_bytes: usize,
    },
    FinishComposing,
    DeleteSurrounding {
        before_bytes: usize,
        after_bytes: usize,
    },
    Key {
        action: i32,
        key_code: i32,
        meta_state: i32,
        unicode_char: i32,
    },
    EditorAction {
        action: i32,
    },
    ImeInsetsChanged {
        bottom_px: i32,
    },
}

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
        self.events.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

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

fn utf16_offset(text: &str, byte_offset: usize) -> i32 {
    let mut offset = byte_offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    text[..offset].encode_utf16().count() as i32
}

const IME_ACTION_UNSPECIFIED: i32 = 0;
const IME_ACTION_NONE: i32 = 1;
const IME_ACTION_GO: i32 = 2;
const IME_ACTION_SEARCH: i32 = 3;
const IME_ACTION_SEND: i32 = 4;
const IME_ACTION_NEXT: i32 = 5;
const IME_ACTION_DONE: i32 = 6;
const IME_ACTION_PREVIOUS: i32 = 7;

/// The action an `InputConnection.performEditorAction` code names;
/// `IME_ACTION_UNSPECIFIED` and unknown codes stand for the field's own.
pub(crate) fn ime_action_from_android(code: i32) -> ImeAction {
    match code {
        IME_ACTION_NONE => ImeAction::None,
        IME_ACTION_GO => ImeAction::Go,
        IME_ACTION_SEARCH => ImeAction::Search,
        IME_ACTION_SEND => ImeAction::Send,
        IME_ACTION_NEXT => ImeAction::Next,
        IME_ACTION_DONE => ImeAction::Done,
        IME_ACTION_PREVIOUS => ImeAction::Previous,
        _ => ImeAction::Default,
    }
}

/// `EditorInfo.inputType` and the action part of `imeOptions` for a field's
/// keyboard, as Compose's Android text input sets them.
fn android_editor_info(options: &KeyboardOptions, single_line: bool) -> (i32, i32) {
    const TYPE_MASK_CLASS: i32 = 0xf;
    const TYPE_TEXT_FLAG_AUTO_CORRECT: i32 = 0x8000;
    const TYPE_TEXT_FLAG_MULTI_LINE: i32 = 0x20000;
    const IME_FLAG_NO_ENTER_ACTION: i32 = 0x4000_0000;

    let (mut input_type, mut ime_options) = android_input_class(options.keyboard_type);
    ime_options |= android_ime_action(options.ime_action, single_line);
    if input_type & TYPE_MASK_CLASS == TYPE_CLASS_TEXT {
        if !single_line {
            input_type |= TYPE_TEXT_FLAG_MULTI_LINE;
            if options.ime_action == ImeAction::Default {
                ime_options |= IME_FLAG_NO_ENTER_ACTION;
            }
        }
        input_type |= android_capitalization(options.capitalization);
        if options.corrects() {
            input_type |= TYPE_TEXT_FLAG_AUTO_CORRECT;
        }
    }
    (input_type, ime_options)
}

const TYPE_CLASS_TEXT: i32 = 0x1;

/// The input class and variation of a keyboard type, and the `imeOptions`
/// flags it needs.
fn android_input_class(keyboard_type: KeyboardType) -> (i32, i32) {
    const TYPE_CLASS_NUMBER: i32 = 0x2;
    const TYPE_CLASS_PHONE: i32 = 0x3;
    const TYPE_TEXT_VARIATION_URI: i32 = 0x10;
    const TYPE_TEXT_VARIATION_EMAIL_ADDRESS: i32 = 0x20;
    const TYPE_TEXT_VARIATION_PASSWORD: i32 = 0x80;
    const TYPE_NUMBER_VARIATION_PASSWORD: i32 = 0x10;
    const TYPE_NUMBER_FLAG_DECIMAL: i32 = 0x2000;
    const IME_FLAG_FORCE_ASCII: i32 = i32::MIN;

    match keyboard_type {
        KeyboardType::Text => (TYPE_CLASS_TEXT, 0),
        KeyboardType::Ascii => (TYPE_CLASS_TEXT, IME_FLAG_FORCE_ASCII),
        KeyboardType::Number => (TYPE_CLASS_NUMBER, 0),
        KeyboardType::Phone => (TYPE_CLASS_PHONE, 0),
        KeyboardType::Uri => (TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_URI, 0),
        KeyboardType::Email => (TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_EMAIL_ADDRESS, 0),
        KeyboardType::Password => (TYPE_CLASS_TEXT | TYPE_TEXT_VARIATION_PASSWORD, 0),
        KeyboardType::NumberPassword => (TYPE_CLASS_NUMBER | TYPE_NUMBER_VARIATION_PASSWORD, 0),
        KeyboardType::Decimal => (TYPE_CLASS_NUMBER | TYPE_NUMBER_FLAG_DECIMAL, 0),
    }
}

fn android_ime_action(action: ImeAction, single_line: bool) -> i32 {
    match action {
        ImeAction::Default if single_line => IME_ACTION_DONE,
        ImeAction::Default => IME_ACTION_UNSPECIFIED,
        ImeAction::None => IME_ACTION_NONE,
        ImeAction::Go => IME_ACTION_GO,
        ImeAction::Search => IME_ACTION_SEARCH,
        ImeAction::Send => IME_ACTION_SEND,
        ImeAction::Previous => IME_ACTION_PREVIOUS,
        ImeAction::Next => IME_ACTION_NEXT,
        ImeAction::Done => IME_ACTION_DONE,
    }
}

fn android_capitalization(capitalization: KeyboardCapitalization) -> i32 {
    const TYPE_TEXT_FLAG_CAP_CHARACTERS: i32 = 0x1000;
    const TYPE_TEXT_FLAG_CAP_WORDS: i32 = 0x2000;
    const TYPE_TEXT_FLAG_CAP_SENTENCES: i32 = 0x4000;

    match capitalization {
        KeyboardCapitalization::None => 0,
        KeyboardCapitalization::Characters => TYPE_TEXT_FLAG_CAP_CHARACTERS,
        KeyboardCapitalization::Words => TYPE_TEXT_FLAG_CAP_WORDS,
        KeyboardCapitalization::Sentences => TYPE_TEXT_FLAG_CAP_SENTENCES,
    }
}

/// The autofill hint of a content type: a `View.AUTOFILL_HINT_*` value, or
/// one of androidx `HintConstants` where the platform names none.
fn android_autofill_hint(content_type: ContentType) -> &'static str {
    match content_type {
        ContentType::Username => "username",
        ContentType::Password => "password",
        ContentType::NewUsername => "newUsername",
        ContentType::NewPassword => "newPassword",
        ContentType::EmailAddress => "emailAddress",
        ContentType::PhoneNumber => "phone",
        ContentType::PersonFullName => "name",
        ContentType::PostalAddress => "postalAddress",
        ContentType::PostalCode => "postalCode",
        ContentType::CreditCardNumber => "creditCardNumber",
        ContentType::SmsOtpCode => "smsOTPCode",
    }
}

pub(crate) fn show_android_text_input(
    app: &android_activity::AndroidApp,
    queue: &Arc<AndroidImeEventQueue>,
    state: &ImeEditorState,
) -> Result<(), String> {
    let handle = retain_ime_queue_handle(queue);
    let sel_start = utf16_offset(&state.text, state.selection_start);
    let sel_end = utf16_offset(&state.text, state.selection_end);
    let (input_type, ime_options) = android_editor_info(&state.keyboard_options, state.single_line);

    let result = with_android_activity_env(app, |env, activity| {
        let class = load_cranpose_java_class(env, &activity, TEXT_INPUT_CLASS)?;
        let text = env.new_string(&state.text).map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to create Android IME text string: {error}")
        })?;
        let text = JObject::from(text);
        let autofill_hint = match state.keyboard_options.content_type {
            Some(content_type) => JObject::from(
                env.new_string(android_autofill_hint(content_type))
                    .map_err(|error| {
                        clear_pending_android_jni_exception(env);
                        format!("failed to create Android autofill hint string: {error}")
                    })?,
            ),
            None => JObject::null(),
        };
        env.call_static_method(
            class,
            jni_str!("show"),
            jni_sig!("(Landroid/app/Activity;JLjava/lang/String;IIIILjava/lang/String;)I"),
            &[
                JValue::Object(&activity),
                JValue::Long(handle.raw()),
                JValue::Object(&text),
                JValue::Int(sel_start),
                JValue::Int(sel_end),
                JValue::Int(input_type),
                JValue::Int(ime_options),
                JValue::Object(&autofill_hint),
            ],
        )
        .and_then(jni::JValueOwned::i)
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

/// Moves the editor view over the focused field, at `[left, top, width,
/// height]` in content-relative physical pixels, so autofill anchors its
/// suggestions to the field.
pub(crate) fn place_android_text_input(
    app: &android_activity::AndroidApp,
    [left, top, width, height]: [i32; 4],
) -> Result<(), String> {
    with_android_activity_env(app, |env, activity| {
        let class = load_cranpose_java_class(env, &activity, TEXT_INPUT_CLASS)?;
        env.call_static_method(
            class,
            jni_str!("place"),
            jni_sig!("(Landroid/app/Activity;IIII)V"),
            &[
                JValue::Object(&activity),
                JValue::Int(left),
                JValue::Int(top),
                JValue::Int(width),
                JValue::Int(height),
            ],
        )
        .map_err(|error| {
            clear_pending_android_jni_exception(env);
            format!("failed to place Android text input: {error}")
        })?;
        Ok(())
    })
}

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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeFinishComposing(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
) {
    push_ime_event_for_handle(queue_handle, AndroidImeEvent::FinishComposing);
}

#[doc(hidden)]
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImePerformEditorAction(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
    action: jint,
) {
    push_ime_event_for_handle(queue_handle, AndroidImeEvent::EditorAction { action });
}

#[doc(hidden)]
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeTextInput_nativeImeReleaseQueue(
    _env: EnvUnowned<'_>,
    _class: JClass<'_>,
    queue_handle: jlong,
) {
    release_ime_queue_handle_raw(queue_handle);
}

#[cfg(test)]
#[path = "tests/text_input_tests.rs"]
mod tests;
