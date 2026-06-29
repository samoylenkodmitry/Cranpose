//! Android writable-folder backend built on the Storage Access Framework.
//!
//! The write-side complement of [`crate::android_file_picker`]. The user grants a
//! persistent read/write tree via `cranposePickWritableFolder` on
//! [`CranposeFilePickerActivity`]; the chosen tree URI comes back through
//! [`Java_dev_cranpose_android_CranposeFilePickerActivity_nativeOnWritableFolderPicked`].
//! Document I/O (`cranposeFolderWrite`/`List`/`Read`/`Remove`/`Writable`) runs
//! synchronously over JNI and is safe to call from a background worker thread —
//! [`crate::android_jni::with_android_activity_env`] attaches the calling thread.
#![allow(unsafe_code)]

use android_activity::AndroidApp;
use cranpose_services::{
    set_platform_writable_folder_picker, set_writable_folder_store_factory, FilePickerError,
    FolderError, PickerFuture, WritableFolderPicker, WritableFolderStore, WritableFolderStoreRef,
};
use jni::objects::{JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jlong};
use jni::{jni_sig, jni_str, Env, EnvUnowned, Outcome};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Waker};

static APP: OnceLock<AndroidApp> = OnceLock::new();
static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);

/// Registers the Android writable-folder picker and store factory. Called once
/// at startup from [`crate::android::run`].
pub(crate) fn register(app: AndroidApp) {
    let _ = APP.set(app);
    set_platform_writable_folder_picker(Rc::new(AndroidWritableFolderPicker));
    set_writable_folder_store_factory(Box::new(|handle| {
        Some(Arc::new(AndroidWritableFolder {
            tree: handle.to_string(),
        }) as WritableFolderStoreRef)
    }));
}

fn app() -> Result<&'static AndroidApp, String> {
    APP.get()
        .ok_or_else(|| "Android writable folder backend is not registered".to_string())
}

fn with_env<T, F>(f: F) -> Result<T, String>
where
    F: for<'local> FnOnce(&mut Env<'local>, JObject<'local>) -> Result<T, String>,
{
    crate::android_jni::with_android_activity_env(app()?, f)
}

fn string_err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

// ---------------------------------------------------------------------------
// Store: synchronous SAF document I/O over JNI
// ---------------------------------------------------------------------------

struct AndroidWritableFolder {
    /// The persisted SAF tree URI (`content://…`).
    tree: String,
}

impl WritableFolderStore for AndroidWritableFolder {
    fn write(&self, name: &str, contents: &[u8]) -> Result<(), FolderError> {
        match call_folder_write(&self.tree, name, contents).map_err(FolderError::Io)? {
            0 => Ok(()),
            1 => Err(FolderError::ReadOnly),
            _ => Err(FolderError::Io("SAF write failed".to_string())),
        }
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, FolderError> {
        match call_folder_read(&self.tree, name).map_err(FolderError::Io)? {
            Some(bytes) => Ok(bytes),
            None => Err(FolderError::NotFound(name.to_string())),
        }
    }

    fn list(&self) -> Result<Vec<String>, FolderError> {
        match call_folder_list(&self.tree).map_err(FolderError::Io)? {
            Some(text) => Ok(text
                .lines()
                .filter(|line| !line.is_empty())
                .map(|line| line.to_string())
                .collect()),
            None => Err(FolderError::Io("SAF list failed".to_string())),
        }
    }

    fn remove(&self, name: &str) -> Result<(), FolderError> {
        match call_folder_remove(&self.tree, name).map_err(FolderError::Io)? {
            0 => Ok(()),
            _ => Err(FolderError::Io("SAF remove failed".to_string())),
        }
    }

    fn is_writable(&self) -> bool {
        call_folder_writable(&self.tree).unwrap_or(false)
    }

    fn handle(&self) -> String {
        self.tree.clone()
    }
}

fn call_folder_write(tree: &str, name: &str, contents: &[u8]) -> Result<i32, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let bytes = env.byte_array_from_slice(contents).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        let bytes_obj: &JObject = bytes.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderWrite"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;[B)I"),
            &[
                JValue::Object(tree_obj),
                JValue::Object(name_obj),
                JValue::Object(bytes_obj),
            ],
        )
        .and_then(|value| value.i())
        .map_err(string_err)
    })
}

fn call_folder_read(tree: &str, name: &str) -> Result<Option<Vec<u8>>, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        let result = env
            .call_method(
                &activity,
                jni_str!("cranposeFolderRead"),
                jni_sig!("(Ljava/lang/String;Ljava/lang/String;)[B"),
                &[JValue::Object(tree_obj), JValue::Object(name_obj)],
            )
            .and_then(|value| value.l())
            .map_err(string_err)?;
        if result.is_null() {
            return Ok(None);
        }
        let array = JByteArray::cast_local(env, result).map_err(string_err)?;
        Ok(Some(env.convert_byte_array(&array).map_err(string_err)?))
    })
}

fn call_folder_list(tree: &str) -> Result<Option<String>, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let result = env
            .call_method(
                &activity,
                jni_str!("cranposeFolderList"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(tree_obj)],
            )
            .and_then(|value| value.l())
            .map_err(string_err)?;
        if result.is_null() {
            return Ok(None);
        }
        let text = JString::cast_local(env, result)
            .map_err(string_err)?
            .try_to_string(env)
            .map_err(string_err)?;
        Ok(Some(text))
    })
}

fn call_folder_remove(tree: &str, name: &str) -> Result<i32, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let name = env.new_string(name).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        let name_obj: &JObject = name.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderRemove"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)I"),
            &[JValue::Object(tree_obj), JValue::Object(name_obj)],
        )
        .and_then(|value| value.i())
        .map_err(string_err)
    })
}

fn call_folder_writable(tree: &str) -> Result<bool, String> {
    with_env(|env, activity| {
        let tree = env.new_string(tree).map_err(string_err)?;
        let tree_obj: &JObject = tree.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeFolderWritable"),
            jni_sig!("(Ljava/lang/String;)Z"),
            &[JValue::Object(tree_obj)],
        )
        .and_then(|value| value.z())
        .map_err(string_err)
    })
}

// ---------------------------------------------------------------------------
// Picker: async folder pick, token registry + JNI callback (mirrors the file
// picker). The Java callback runs on the UI thread; the future is polled on the
// android_main thread, so results travel through a `Send` global.
// ---------------------------------------------------------------------------

#[derive(Default)]
struct PickPending {
    result: Option<Result<Option<String>, String>>,
    waker: Option<Waker>,
}

fn pick_pending() -> &'static Mutex<HashMap<i64, PickPending>> {
    static PENDING: OnceLock<Mutex<HashMap<i64, PickPending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

struct AndroidWritableFolderPicker;

impl WritableFolderPicker for AndroidWritableFolderPicker {
    fn pick(&self) -> PickerFuture<Result<Option<String>, FilePickerError>> {
        // Discard any orphaned grant from an earlier abandoned pick; this fresh
        // request becomes the only thing the shared resume inbox can hold.
        crate::android_file_picker::clear_resumable();
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        pick_pending()
            .lock()
            .expect("writable folder registry poisoned")
            .insert(token, PickPending::default());

        if let Err(error) = call_pick_writable(token) {
            pick_pending()
                .lock()
                .expect("writable folder registry poisoned")
                .remove(&token);
            return Box::pin(async move { Err(FilePickerError::Failed(error)) });
        }
        Box::pin(PickFuture { token })
    }

    fn take_resumed_pick(&self) -> Option<String> {
        crate::android_file_picker::take_resumed_writable_uri()
    }
}

fn call_pick_writable(token: i64) -> Result<(), String> {
    with_env(|env, activity| {
        env.call_method(
            &activity,
            jni_str!("cranposePickWritableFolder"),
            jni_sig!("(J)V"),
            &[JValue::Long(token)],
        )
        .map(|_| ())
        .map_err(string_err)
    })
}

struct PickFuture {
    token: i64,
}

impl Future for PickFuture {
    type Output = Result<Option<String>, FilePickerError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut registry = pick_pending()
            .lock()
            .expect("writable folder registry poisoned");
        let Some(slot) = registry.get_mut(&self.token) else {
            return Poll::Ready(Ok(None));
        };
        match slot.result.take() {
            Some(Ok(handle)) => {
                registry.remove(&self.token);
                // Delivered live; drop the resume copy recorded in
                // `onActivityResult` so it is not replayed on the next start.
                crate::android_file_picker::clear_resumable();
                Poll::Ready(Ok(handle))
            }
            Some(Err(message)) => {
                registry.remove(&self.token);
                crate::android_file_picker::clear_resumable();
                Poll::Ready(Err(FilePickerError::Failed(message)))
            }
            None => {
                slot.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Java callback delivering the picked writable tree URI (or cancellation/error).
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeFilePickerActivity_nativeOnWritableFolderPicked<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    uri: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    let uri = read_optional_jstring(&mut env, uri);
    let error = read_optional_jstring(&mut env, error);
    let result = if cancelled {
        Ok(None)
    } else if let Some(error) = error {
        Err(error)
    } else {
        Ok(uri)
    };
    let mut registry = pick_pending()
        .lock()
        .expect("writable folder registry poisoned");
    if let Some(slot) = registry.get_mut(&token) {
        slot.result = Some(result);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

fn read_optional_jstring(env: &mut EnvUnowned<'_>, value: JString<'_>) -> Option<String> {
    if value.is_null() {
        return None;
    }
    match env
        .with_env(|env| -> jni::errors::Result<String> { value.try_to_string(env) })
        .into_outcome()
    {
        Outcome::Ok(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}
