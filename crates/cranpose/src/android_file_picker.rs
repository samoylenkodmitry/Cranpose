//! Android file and folder picker built on the Storage Access Framework.
//!
//! `cranposePickFile` / `cranposePickFolder` on
//! [`CranposeFilePickerActivity`](https://github.com/samoylenkodmitry/cranpose)
//! launch `ACTION_OPEN_DOCUMENT` / `ACTION_OPEN_DOCUMENT_TREE`, so the user can
//! choose a file or a folder from any document provider the device exposes
//! (local storage, cloud, or a mounted WebDAV share). The Java side copies the
//! selection into the app cache and reports the path back through
//! [`Java_dev_cranpose_android_CranposeFilePickerActivity_nativeOnFilePicked`],
//! which lets the picked entry be read with ordinary filesystem calls.
//!
//! The Java callback runs on the Android UI thread while `android_main` runs on
//! its own thread, so results travel through `Send` globals; the picked-entry
//! handle is built on the `android_main` thread when the future is polled.
#![allow(unsafe_code)]

use cranpose_services::{
    set_platform_file_picker, FilePicker, FilePickerError, FilePickerOptions, PickedEntry,
    PickedEntryRef, PickedKind, PickerFuture,
};
use jni::objects::{JClass, JString, JValue};
use jni::sys::{jboolean, jlong};
use jni::{jni_sig, jni_str, EnvUnowned, Outcome};
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll, Waker};

type PickResult = Result<Option<PickedEntryRef>, FilePickerError>;

/// The raw, `Send` data delivered from the Java UI-thread callback.
struct RawResult {
    path: Option<String>,
    error: Option<String>,
}

#[derive(Default)]
struct Pending {
    result: Option<RawResult>,
    waker: Option<Waker>,
}

static APP: OnceLock<android_activity::AndroidApp> = OnceLock::new();
static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);

fn pending() -> &'static Mutex<HashMap<i64, Pending>> {
    static PENDING: OnceLock<Mutex<HashMap<i64, Pending>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Installs the Android picker as the platform file picker.
pub(crate) fn register(app: android_activity::AndroidApp) {
    let _ = APP.set(app);
    set_platform_file_picker(Rc::new(AndroidFilePicker));
}

struct AndroidFilePicker;

impl FilePicker for AndroidFilePicker {
    fn pick_file(&self, _options: FilePickerOptions) -> PickerFuture<PickResult> {
        present(false)
    }

    fn pick_folder(&self, _options: FilePickerOptions) -> PickerFuture<PickResult> {
        present(true)
    }
}

fn present(folder: bool) -> PickerFuture<PickResult> {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    pending()
        .lock()
        .expect("file picker registry poisoned")
        .insert(token, Pending::default());

    if let Err(error) = call_activity(folder, token) {
        pending()
            .lock()
            .expect("file picker registry poisoned")
            .remove(&token);
        return Box::pin(async move { Err(FilePickerError::Failed(error)) });
    }

    Box::pin(PickFuture { token })
}

fn call_activity(folder: bool, token: i64) -> Result<(), String> {
    let app = APP
        .get()
        .ok_or_else(|| "Android file picker was not registered".to_string())?;
    crate::android_jni::with_android_activity_env(app, |env, activity| {
        let method = if folder {
            jni_str!("cranposePickFolder")
        } else {
            jni_str!("cranposePickFile")
        };
        env.call_method(&activity, method, jni_sig!("(J)V"), &[JValue::Long(token)])
            .map(|_| ())
            .map_err(|error| format!("failed to launch Android picker: {error}"))
    })
}

/// Future resolved when the Java callback reports a result for `token`.
struct PickFuture {
    token: i64,
}

impl Future for PickFuture {
    type Output = PickResult;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<PickResult> {
        let mut registry = pending().lock().expect("file picker registry poisoned");
        let Some(slot) = registry.get_mut(&self.token) else {
            return Poll::Ready(Ok(None));
        };
        match slot.result.take() {
            Some(raw) => {
                registry.remove(&self.token);
                Poll::Ready(build_result(raw))
            }
            None => {
                slot.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

fn build_result(raw: RawResult) -> PickResult {
    if let Some(error) = raw.error {
        return Err(FilePickerError::Failed(error));
    }
    let Some(path) = raw.path else {
        return Ok(None);
    };
    let path = PathBuf::from(path);
    let kind = if path.is_dir() {
        PickedKind::Folder
    } else {
        PickedKind::File
    };
    Ok(Some(Rc::new(CacheEntry { path, kind })))
}

/// A picked entry materialized into the app cache; read through `std::fs`.
struct CacheEntry {
    path: PathBuf,
    kind: PickedKind,
}

impl PickedEntry for CacheEntry {
    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    fn kind(&self) -> PickedKind {
        self.kind
    }

    fn display_path(&self) -> String {
        self.path.display().to_string()
    }

    fn read_bytes(&self) -> PickerFuture<Result<Vec<u8>, FilePickerError>> {
        if self.kind != PickedKind::File {
            return Box::pin(async {
                Err(FilePickerError::WrongKind {
                    actual: "folder",
                    expected: "file",
                })
            });
        }
        let path = self.path.clone();
        Box::pin(async move {
            std::fs::read(&path).map_err(|error| FilePickerError::ReadFailed(error.to_string()))
        })
    }

    fn list(&self) -> PickerFuture<Result<Vec<PickedEntryRef>, FilePickerError>> {
        if self.kind != PickedKind::Folder {
            return Box::pin(async {
                Err(FilePickerError::WrongKind {
                    actual: "file",
                    expected: "folder",
                })
            });
        }
        let path = self.path.clone();
        Box::pin(async move { list_dir(&path) })
    }
}

fn list_dir(path: &Path) -> Result<Vec<PickedEntryRef>, FilePickerError> {
    let read =
        std::fs::read_dir(path).map_err(|error| FilePickerError::ReadFailed(error.to_string()))?;
    let mut entries: Vec<PickedEntryRef> = Vec::new();
    for entry in read {
        let entry = entry.map_err(|error| FilePickerError::ReadFailed(error.to_string()))?;
        let path = entry.path();
        let kind = if path.is_dir() {
            PickedKind::Folder
        } else {
            PickedKind::File
        };
        entries.push(Rc::new(CacheEntry { path, kind }));
    }
    Ok(entries)
}

fn deliver(token: i64, result: RawResult) {
    let mut registry = pending().lock().expect("file picker registry poisoned");
    if let Some(slot) = registry.get_mut(&token) {
        slot.result = Some(result);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// Java callback delivering a picker result. Runs on the Android UI thread.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeFilePickerActivity_nativeOnFilePicked<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    path: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    let path = read_optional_jstring(&mut env, path);
    let error = read_optional_jstring(&mut env, error);
    let path = if cancelled { None } else { path };
    deliver(token, RawResult { path, error });
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
