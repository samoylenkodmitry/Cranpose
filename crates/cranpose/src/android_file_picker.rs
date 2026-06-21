//! Android file and folder picker built on the Storage Access Framework.
//!
//! `cranposePickFile` / `cranposePickFolder` on
//! [`CranposeFilePickerActivity`](https://github.com/samoylenkodmitry/cranpose)
//! launch `ACTION_OPEN_DOCUMENT` / `ACTION_OPEN_DOCUMENT_TREE`, so the user can
//! choose a file or a folder from any document provider the device exposes
//! (local storage, cloud, or a mounted WebDAV share). The Java side reports the
//! chosen `content://` document URIs back through
//! [`Java_dev_cranpose_android_CranposeFilePickerActivity_nativeOnFilePicked`];
//! nothing is copied. A picked file is read on demand by opening a descriptor
//! from the provider through [`open_content_uri`], so even a multi-gigabyte
//! folder is selected instantly and each track is streamed only when played.
//!
//! The Java callback runs on the Android UI thread while `android_main` runs on
//! its own thread, so results travel through `Send` globals; the picked-entry
//! handle is built on the `android_main` thread when the future is polled.
#![allow(unsafe_code)]

use cranpose_services::{
    set_platform_file_picker, FilePicker, FilePickerError, FilePickerOptions, PickedEntry,
    PickedEntryRef, PickedKind, PickerFuture,
};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jlong};
use jni::{jni_sig, jni_str, EnvUnowned, Outcome};
use std::collections::HashMap;
use std::fs::File;
use std::future::Future;
use std::io::{self, Read};
use std::os::fd::FromRawFd;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::task::{Context, Poll, Waker};

type PickResult = Result<Option<PickedEntryRef>, FilePickerError>;

/// A picked document: its `content://` URI and display name.
struct PickedDocument {
    uri: String,
    name: String,
}

/// The raw, `Send` data delivered from the Java UI-thread callback.
struct RawResult {
    folder: bool,
    documents: Vec<PickedDocument>,
    cancelled: bool,
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
    if raw.cancelled {
        return Ok(None);
    }
    if let Some(error) = raw.error {
        return Err(FilePickerError::Failed(error));
    }
    if raw.folder {
        let children: Vec<PickedEntryRef> = raw
            .documents
            .into_iter()
            .map(|document| Rc::new(UriEntry::from(document)) as PickedEntryRef)
            .collect();
        Ok(Some(Rc::new(FolderEntry { children })))
    } else {
        match raw.documents.into_iter().next() {
            Some(document) => Ok(Some(Rc::new(UriEntry::from(document)))),
            None => Ok(None),
        }
    }
}

/// A picked file addressed by a `content://` URI. It is opened on demand
/// through the provider's descriptor and never copied to the cache.
struct UriEntry {
    uri: String,
    name: String,
}

impl From<PickedDocument> for UriEntry {
    fn from(document: PickedDocument) -> Self {
        UriEntry {
            uri: document.uri,
            name: document.name,
        }
    }
}

impl PickedEntry for UriEntry {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn kind(&self) -> PickedKind {
        PickedKind::File
    }

    fn display_path(&self) -> String {
        self.uri.clone()
    }

    fn read_bytes(&self) -> PickerFuture<Result<Vec<u8>, FilePickerError>> {
        let uri = self.uri.clone();
        Box::pin(async move {
            let mut file = open_content_uri(&uri)
                .map_err(|error| FilePickerError::ReadFailed(error.to_string()))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|error| FilePickerError::ReadFailed(error.to_string()))?;
            Ok(bytes)
        })
    }

    fn list(&self) -> PickerFuture<Result<Vec<PickedEntryRef>, FilePickerError>> {
        Box::pin(async {
            Err(FilePickerError::WrongKind {
                actual: "file",
                expected: "folder",
            })
        })
    }
}

/// A picked folder: its audio descendants enumerated as [`UriEntry`] children
/// without copying anything.
struct FolderEntry {
    children: Vec<PickedEntryRef>,
}

impl PickedEntry for FolderEntry {
    fn name(&self) -> String {
        "folder".to_string()
    }

    fn kind(&self) -> PickedKind {
        PickedKind::Folder
    }

    fn display_path(&self) -> String {
        String::new()
    }

    fn read_bytes(&self) -> PickerFuture<Result<Vec<u8>, FilePickerError>> {
        Box::pin(async {
            Err(FilePickerError::WrongKind {
                actual: "folder",
                expected: "file",
            })
        })
    }

    fn list(&self) -> PickerFuture<Result<Vec<PickedEntryRef>, FilePickerError>> {
        let children = self.children.clone();
        Box::pin(async move { Ok(children) })
    }
}

/// Opens a picked `content://` document for reading, returning a [`File`] backed
/// by the provider's descriptor. Nothing is copied; the descriptor is detached
/// from its `ParcelFileDescriptor` so the returned `File` owns and closes it.
/// Callable from any thread (it attaches to the JVM as needed), so the audio
/// engine can stream a track straight from the provider.
pub fn open_content_uri(uri: &str) -> io::Result<File> {
    let app = APP.get().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            "Android file picker is not registered",
        )
    })?;
    let fd = crate::android_jni::with_android_activity_env(app, |env, activity| {
        let argument = env.new_string(uri).map_err(|error| error.to_string())?;
        let argument: &JObject = argument.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeOpenUri"),
            jni_sig!("(Ljava/lang/String;)I"),
            &[JValue::Object(argument)],
        )
        .and_then(|value| value.i())
        .map_err(|error| error.to_string())
    })
    .map_err(|error| io::Error::other(error))?;
    if fd < 0 {
        return Err(io::Error::other(format!(
            "ContentResolver returned no descriptor for {uri}"
        )));
    }
    // SAFETY: `cranposeOpenUri` detaches the descriptor from its
    // `ParcelFileDescriptor`, transferring ownership to this process; the
    // returned `File` closes it on drop.
    Ok(unsafe { File::from_raw_fd(fd) })
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

/// Java callback delivering a picker result. Runs on a worker thread spawned by
/// the activity. `entries` is newline-separated `uri\tname` rows (one for a
/// file, every audio descendant for a folder).
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeFilePickerActivity_nativeOnFilePicked<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    folder: jboolean,
    entries: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    let documents = read_optional_jstring(&mut env, entries)
        .map(parse_documents)
        .unwrap_or_default();
    let error = read_optional_jstring(&mut env, error);
    deliver(
        token,
        RawResult {
            folder,
            documents,
            cancelled,
            error,
        },
    );
}

fn parse_documents(text: String) -> Vec<PickedDocument> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let uri = parts.next()?;
            if uri.is_empty() {
                return None;
            }
            let name = parts.next().unwrap_or("");
            Some(PickedDocument {
                uri: uri.to_string(),
                name: name.to_string(),
            })
        })
        .collect()
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
