//! Android file and folder picker built on the Storage Access Framework.
//!
//! `cranposePickFile` / `cranposePickFolder` on
//! [`CranposeActivity`](https://github.com/samoylenkodmitry/cranpose)
//! launch `ACTION_OPEN_DOCUMENT` / `ACTION_OPEN_DOCUMENT_TREE`, so the user can
//! choose a file or a folder from any document provider the device exposes
//! (local storage, cloud, or a mounted WebDAV share). The Java side reports the
//! chosen `content://` document URIs back through
//! [`Java_dev_cranpose_android_CranposeActivity_nativeOnFilePicked`];
//! nothing is copied. A picked file is read on demand by opening a descriptor
//! from the provider through [`open_content_uri`], so even a multi-gigabyte
//! folder is selected instantly and each track is streamed only when played.
//!
//! The Java callback runs on the Android UI thread while `android_main` runs on
//! its own thread, so results travel through `Send` globals; the picked-entry
//! handle is built on the `android_main` thread when the future is polled.
#![allow(unsafe_code)]

use cranpose_services::{
    set_platform_file_picker, FilePicker, FilePickerError, FilePickerOptions, FolderStream,
    FolderStreamRef, PickedEntry, PickedEntryRef, PickedKind, PickerFuture, ResumedPick,
    SaveFileRequest,
};
use jni::objects::{JClass, JObject, JString, JValue};
use jni::sys::{jboolean, jint, jlong};
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

/// An in-flight ACTION_CREATE_DOCUMENT save awaiting its Java result.
#[derive(Default)]
struct PendingSave {
    /// `(saved, error)` — `saved = false` with no error means cancelled.
    result: Option<(bool, Option<String>)>,
    waker: Option<Waker>,
}

fn pending_saves() -> &'static Mutex<HashMap<i64, PendingSave>> {
    static PENDING: OnceLock<Mutex<HashMap<i64, PendingSave>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Delivers an ACTION_CREATE_DOCUMENT result from the Java UI thread.
pub(crate) fn resolve_pending_save(token: i64, saved: bool, error: Option<String>) {
    let mut registry = pending_saves().lock().expect("save registry poisoned");
    if let Some(slot) = registry.get_mut(&token) {
        slot.result = Some((saved, error));
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
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

    fn pick_folder_streaming(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<FolderStreamRef>, FilePickerError>> {
        present_folder_stream()
    }

    fn take_resumed_picks(&self) -> Vec<ResumedPick> {
        take_resumed_file_picks()
    }

    fn save_file(&self, request: SaveFileRequest) -> PickerFuture<Result<bool, FilePickerError>> {
        present_save(request)
    }
}

fn present_save(request: SaveFileRequest) -> PickerFuture<Result<bool, FilePickerError>> {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    pending_saves()
        .lock()
        .expect("save registry poisoned")
        .insert(token, PendingSave::default());

    let launch = (|| -> Result<(), String> {
        let app = APP
            .get()
            .ok_or_else(|| "Android file picker was not registered".to_string())?;
        crate::android_jni::with_android_activity_env(app, |env, activity| {
            let name = env
                .new_string(&request.file_name)
                .map_err(|error| error.to_string())?;
            let mime = env
                .new_string(&request.mime_type)
                .map_err(|error| error.to_string())?;
            let bytes = env
                .byte_array_from_slice(&request.bytes)
                .map_err(|error| error.to_string())?;
            let name_obj: &JObject = name.as_ref();
            let mime_obj: &JObject = mime.as_ref();
            let bytes_obj: &JObject = bytes.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeSaveFile"),
                jni_sig!("(JLjava/lang/String;Ljava/lang/String;[B)V"),
                &[
                    JValue::Long(token),
                    JValue::Object(name_obj),
                    JValue::Object(mime_obj),
                    JValue::Object(bytes_obj),
                ],
            )
            .map(|_| ())
            .map_err(|error| format!("failed to launch Android save: {error}"))
        })
    })();

    if let Err(error) = launch {
        pending_saves()
            .lock()
            .expect("save registry poisoned")
            .remove(&token);
        return Box::pin(async move { Err(FilePickerError::Failed(error)) });
    }

    Box::pin(SaveFuture { token })
}

/// Future resolved when the Java callback reports a save result for `token`.
struct SaveFuture {
    token: i64,
}

impl Future for SaveFuture {
    type Output = Result<bool, FilePickerError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut registry = pending_saves().lock().expect("save registry poisoned");
        let Some(slot) = registry.get_mut(&self.token) else {
            return Poll::Ready(Ok(false));
        };
        match slot.result.take() {
            Some((saved, error)) => {
                registry.remove(&self.token);
                match error {
                    Some(error) => Poll::Ready(Err(FilePickerError::Failed(error))),
                    None => Poll::Ready(Ok(saved)),
                }
            }
            None => {
                slot.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

// ---- Resume inbox --------------------------------------------------------
//
// Some Android devices destroy and recreate the activity (and with it the
// native app and its composition) when the SAF picker covers it. A pick in
// flight at that moment loses both its Java request token (a fresh activity
// instance starts at token 0) and the composition that was awaiting the
// result. The Java `onActivityResult` still runs on the recreated activity and
// records the granted selection here; the app drains it on its next start via
// [`FilePicker::take_resumed_picks`]. A normal (live) pick clears the inbox on
// resolution, so a selection is never replayed.

/// Mirrors the `FLAG_*` request flags in `CranposeActivity`.
const FLAG_FOLDER: i32 = 1;
const FLAG_STREAMING: i32 = 2;
const FLAG_WRITABLE: i32 = 4;

/// A granted selection recorded for resume after an activity recreation.
struct ResumableEntry {
    flags: i32,
    uri: String,
    name: String,
}

fn resumable() -> &'static Mutex<Vec<ResumableEntry>> {
    static RESUMABLE: OnceLock<Mutex<Vec<ResumableEntry>>> = OnceLock::new();
    RESUMABLE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Discards any recorded selection. Called when a fresh pick is launched and
/// when a live pick resolves, so the inbox only ever holds an *orphaned* result
/// (one whose requesting composition was destroyed before it could consume it).
/// `pub(crate)` so the writable-folder picker, which shares this inbox, can clear
/// it on its own fresh pick / live resolution.
pub(crate) fn clear_resumable() {
    resumable().lock().expect("resume inbox poisoned").clear();
}

/// Drains a writable-folder grant orphaned by an activity recreation, returning
/// its tree URI. The write side of [`take_resumed_file_picks`]: it removes only
/// the `FLAG_WRITABLE` entries (leaving any file/folder grants for the file
/// picker to reclaim) and yields the most recent recovered handle.
pub(crate) fn take_resumed_writable_uri() -> Option<String> {
    let mut inbox = resumable().lock().expect("resume inbox poisoned");
    let mut recovered = None;
    let mut kept = Vec::new();
    for entry in inbox.drain(..) {
        if entry.flags & FLAG_WRITABLE != 0 {
            recovered = Some(entry.uri);
        } else {
            kept.push(entry);
        }
    }
    *inbox = kept;
    recovered
}

/// Drains the file/folder selections orphaned by an activity recreation,
/// turning each into a resumable handle. Writable-folder grants are left in the
/// inbox for the writable-folder picker to reclaim.
fn take_resumed_file_picks() -> Vec<ResumedPick> {
    let orphaned = {
        let mut inbox = resumable().lock().expect("resume inbox poisoned");
        let mut taken = Vec::new();
        let mut kept = Vec::new();
        for entry in inbox.drain(..) {
            if entry.flags & FLAG_WRITABLE != 0 {
                kept.push(entry);
            } else {
                taken.push(entry);
            }
        }
        *inbox = kept;
        taken
    };
    orphaned.into_iter().filter_map(resume_entry).collect()
}

fn resume_entry(entry: ResumableEntry) -> Option<ResumedPick> {
    if entry.flags & (FLAG_FOLDER | FLAG_STREAMING) != 0 {
        resume_folder_stream(entry.uri).map(ResumedPick::Folder)
    } else {
        Some(ResumedPick::File(Rc::new(UriEntry {
            uri: entry.uri,
            name: entry.name,
        })))
    }
}

/// Re-walks an already-granted tree URI as a stream (no picker UI), reusing the
/// streaming registry so the returned handle behaves like a freshly-picked one.
fn resume_folder_stream(uri: String) -> Option<FolderStreamRef> {
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    folder_streaming()
        .lock()
        .expect("folder picker registry poisoned")
        .insert(
            token,
            FolderStreaming {
                picked: true,
                ..FolderStreaming::default()
            },
        );
    if let Err(error) = call_stream_granted_folder(uri, token) {
        folder_streaming()
            .lock()
            .expect("folder picker registry poisoned")
            .remove(&token);
        log::warn!("cranpose: failed to resume folder stream: {error}");
        return None;
    }
    Some(Rc::new(AndroidFolderStream { token }) as FolderStreamRef)
}

fn call_stream_granted_folder(uri: String, token: i64) -> Result<(), String> {
    let app = APP
        .get()
        .ok_or_else(|| "Android file picker was not registered".to_string())?;
    crate::android_jni::with_android_activity_env(app, |env, activity| {
        let uri_arg = env.new_string(&uri).map_err(|error| error.to_string())?;
        let uri_obj: &JObject = uri_arg.as_ref();
        env.call_method(
            &activity,
            jni_str!("cranposeStreamGrantedFolder"),
            jni_sig!("(JLjava/lang/String;)V"),
            &[JValue::Long(token), JValue::Object(uri_obj)],
        )
        .map(|_| ())
        .map_err(|error| format!("failed to start granted folder walk: {error}"))
    })
}

/// Java callback: records a granted selection in the resume inbox (see above).
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeRecordResumablePick<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    flags: jint,
    uri: JString<'local>,
    name: JString<'local>,
) {
    let Some(uri) = read_optional_jstring(&mut env, uri) else {
        return;
    };
    let name = read_optional_jstring(&mut env, name).unwrap_or_default();
    resumable()
        .lock()
        .expect("resume inbox poisoned")
        .push(ResumableEntry { flags, uri, name });
}

/// Which Android picker entry point to launch for a request.
#[derive(Clone, Copy)]
enum PickKind {
    /// `cranposePickFile` — a single document.
    File,
    /// `cranposePickFolder` — a tree, enumerated fully before delivery.
    Folder,
    /// `cranposePickFolderStreaming` — a tree whose files stream in as the
    /// provider discovers them.
    FolderStreaming,
}

fn present(folder: bool) -> PickerFuture<PickResult> {
    // Discard any orphaned selection from an earlier abandoned pick; this fresh
    // request becomes the only thing the resume inbox can hold.
    clear_resumable();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    pending()
        .lock()
        .expect("file picker registry poisoned")
        .insert(token, Pending::default());

    let kind = if folder {
        PickKind::Folder
    } else {
        PickKind::File
    };
    if let Err(error) = call_activity(kind, token) {
        pending()
            .lock()
            .expect("file picker registry poisoned")
            .remove(&token);
        return Box::pin(async move { Err(FilePickerError::Failed(error)) });
    }

    Box::pin(PickFuture { token })
}

fn call_activity(kind: PickKind, token: i64) -> Result<(), String> {
    let app = APP
        .get()
        .ok_or_else(|| "Android file picker was not registered".to_string())?;
    crate::android_jni::with_android_activity_env(app, |env, activity| {
        let method = match kind {
            PickKind::File => jni_str!("cranposePickFile"),
            PickKind::Folder => jni_str!("cranposePickFolder"),
            PickKind::FolderStreaming => jni_str!("cranposePickFolderStreaming"),
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
                // This pick was delivered live; drop the resume copy recorded in
                // `onActivityResult` so it is not replayed on the next start.
                clear_resumable();
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

// ---- Streaming folder discovery ------------------------------------------
//
// `enumerateTree` on the Java side walks the picked tree on a worker thread and
// reports audio files in batches as it finds them. That matters for a slow
// provider (a mounted WebDAV share): instead of blocking until the whole tree
// is walked, the folder selection resolves immediately and files arrive
// incrementally, so the app can show progress and play the first track at once.

/// Per-token state for a streaming folder pick, written by the Java callbacks
/// and drained by the [`AndroidFolderStream`] on the UI thread.
#[derive(Default)]
struct FolderStreaming {
    documents: Vec<PickedDocument>,
    picked: bool,
    cancelled: bool,
    pick_error: Option<String>,
    finished: bool,
    stream_error: Option<String>,
    waker: Option<Waker>,
}

fn folder_streaming() -> &'static Mutex<HashMap<i64, FolderStreaming>> {
    static FOLDER_STREAMING: OnceLock<Mutex<HashMap<i64, FolderStreaming>>> = OnceLock::new();
    FOLDER_STREAMING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn present_folder_stream() -> PickerFuture<Result<Option<FolderStreamRef>, FilePickerError>> {
    clear_resumable();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    folder_streaming()
        .lock()
        .expect("folder picker registry poisoned")
        .insert(token, FolderStreaming::default());

    if let Err(error) = call_activity(PickKind::FolderStreaming, token) {
        folder_streaming()
            .lock()
            .expect("folder picker registry poisoned")
            .remove(&token);
        return Box::pin(async move { Err(FilePickerError::Failed(error)) });
    }

    Box::pin(FolderPickFuture { token })
}

/// Resolves once the user has selected (or cancelled) the folder; the returned
/// [`AndroidFolderStream`] then yields files as enumeration continues.
struct FolderPickFuture {
    token: i64,
}

impl Future for FolderPickFuture {
    type Output = Result<Option<FolderStreamRef>, FilePickerError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut registry = folder_streaming()
            .lock()
            .expect("folder picker registry poisoned");
        let Some(slot) = registry.get_mut(&self.token) else {
            return Poll::Ready(Ok(None));
        };
        if slot.cancelled {
            registry.remove(&self.token);
            clear_resumable();
            return Poll::Ready(Ok(None));
        }
        if let Some(error) = slot.pick_error.take() {
            registry.remove(&self.token);
            clear_resumable();
            return Poll::Ready(Err(FilePickerError::Failed(error)));
        }
        if slot.picked {
            // Delivered live; drop the resume copy recorded in `onActivityResult`.
            clear_resumable();
            return Poll::Ready(Ok(Some(
                Rc::new(AndroidFolderStream { token: self.token }) as FolderStreamRef
            )));
        }
        slot.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

/// Streams files discovered under a picked folder. Dropping it discards the
/// registry slot (the Java enumeration thread checks the slot and stops).
struct AndroidFolderStream {
    token: i64,
}

impl FolderStream for AndroidFolderStream {
    fn take_ready(&self) -> Vec<PickedEntryRef> {
        let mut registry = folder_streaming()
            .lock()
            .expect("folder picker registry poisoned");
        let Some(slot) = registry.get_mut(&self.token) else {
            return Vec::new();
        };
        std::mem::take(&mut slot.documents)
            .into_iter()
            .map(|document| Rc::new(UriEntry::from(document)) as PickedEntryRef)
            .collect()
    }

    fn is_finished(&self) -> bool {
        let registry = folder_streaming()
            .lock()
            .expect("folder picker registry poisoned");
        registry
            .get(&self.token)
            .map(|slot| slot.finished && slot.documents.is_empty())
            .unwrap_or(true)
    }

    fn take_error(&self) -> Option<FilePickerError> {
        let mut registry = folder_streaming()
            .lock()
            .expect("folder picker registry poisoned");
        registry
            .get_mut(&self.token)
            .and_then(|slot| slot.stream_error.take())
            .map(FilePickerError::Failed)
    }
}

impl Drop for AndroidFolderStream {
    fn drop(&mut self) {
        folder_streaming()
            .lock()
            .expect("folder picker registry poisoned")
            .remove(&self.token);
    }
}

/// Java callback: the user picked a folder (or cancelled/failed). Resolves the
/// [`FolderPickFuture`] so streaming can begin.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFolderPicked<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    cancelled: jboolean,
    error: JString<'local>,
) {
    let error = read_optional_jstring(&mut env, error);
    let mut registry = folder_streaming()
        .lock()
        .expect("folder picker registry poisoned");
    if let Some(slot) = registry.get_mut(&token) {
        if cancelled {
            slot.cancelled = true;
        } else if let Some(error) = error {
            slot.pick_error = Some(error);
        } else {
            slot.picked = true;
        }
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

/// Java callback: a batch of newly-discovered files (`uri\tname` rows). Returns
/// `false` (0) once the consumer has dropped the stream, so the Java
/// enumeration thread can stop walking a huge tree it no longer needs.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFolderEntries<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    entries: JString<'local>,
) -> jboolean {
    let documents = read_optional_jstring(&mut env, entries)
        .map(parse_documents)
        .unwrap_or_default();
    let mut registry = folder_streaming()
        .lock()
        .expect("folder picker registry poisoned");
    match registry.get_mut(&token) {
        Some(slot) => {
            slot.documents.extend(documents);
            true
        }
        None => false,
    }
}

/// Java callback: enumeration finished (with an optional error).
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFolderFinished<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    error: JString<'local>,
) {
    let error = read_optional_jstring(&mut env, error);
    let mut registry = folder_streaming()
        .lock()
        .expect("folder picker registry poisoned");
    if let Some(slot) = registry.get_mut(&token) {
        slot.finished = true;
        slot.stream_error = error;
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
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFilePicked<'local>(
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
