//! Android file, folder and document choosers built on the Storage Access
//! Framework.
//!
//! `cranposePickFile` / `cranposePickFiles` / `cranposePickFolder` /
//! `cranposeCreateDocument` on
//! [`CranposeActivity`](https://github.com/samoylenkodmitry/cranpose)
//! launch `ACTION_OPEN_DOCUMENT` / `ACTION_OPEN_DOCUMENT_TREE` /
//! `ACTION_CREATE_DOCUMENT`, so the user can choose from any document provider
//! the device exposes (local storage, cloud, or a mounted WebDAV share). The
//! Java side reports the chosen `content://` document URIs back through the
//! `native*` callbacks below; nothing is copied. Content is read and written on
//! demand through descriptors opened from the provider, so even a
//! multi-gigabyte folder is chosen instantly and each file streams only when it
//! is used.
//!
//! Java callbacks run on the Android UI thread (or a worker it spawns) while
//! `android_main` runs on its own thread, so results travel through `Send`
//! globals and wake the awaiting future.
#![allow(unsafe_code)]

use std::{
    collections::HashMap,
    fs::File,
    future::Future,
    io::{self, Read, Write},
    os::fd::FromRawFd,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicI64, Ordering},
        Mutex, OnceLock,
    },
    task::{Context, Poll, Waker},
};

use cranpose_services::{
    set_platform_content_resolver, set_platform_file_picker, Content, ContentEntry, ContentError,
    ContentFolder, ContentFolderRef, ContentFuture, ContentHandle, ContentMetadata, ContentReader,
    ContentReaderRef, ContentResolver, ContentSink, ContentSinkRef, ContentStream,
    ContentStreamRef, FilePicker, FilePickerError, FilePickerOptions, PickerFuture, RecoveredPick,
    SaveDocumentRequest, DEFAULT_CHUNK_LEN,
};
use jni::{
    jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::{jboolean, jint, jlong},
    EnvUnowned, Outcome,
};

/// Mirrors the `FLAG_*` request flags in `CranposeActivity`.
const FLAG_FOLDER: i32 = 1;
const FLAG_WRITABLE: i32 = 4;

static APP: OnceLock<android_activity::AndroidApp> = OnceLock::new();
static NEXT_TOKEN: AtomicI64 = AtomicI64::new(1);

/// Installs the Android chooser as the platform file picker and the Storage
/// Access Framework as the platform content resolver.
pub(crate) fn register(app: android_activity::AndroidApp) {
    let _ = APP.set(app);
    set_platform_file_picker(Rc::new(AndroidFilePicker));
    set_platform_content_resolver(Rc::new(AndroidContentResolver));
}

/// Resolves a `content://` URI — a shared item, a document intent, a dropped
/// file — into readable content by asking the provider to describe it.
struct AndroidContentResolver;

impl ContentResolver for AndroidContentResolver {
    fn resolve(&self, uri: &str) -> Option<ContentHandle> {
        if !uri.starts_with("content://") && !uri.starts_with("file://") {
            return None;
        }
        Some(content_of(document_info(uri).unwrap_or_else(|| {
            ContentMetadata::named(
                uri.rsplit('/')
                    .find(|segment| !segment.is_empty())
                    .unwrap_or(uri),
            )
            .with_identifier(uri)
        })))
    }
}

/// Asks the provider to describe one document, as `name\tmime\tsize\tmodified`.
fn document_info(uri: &str) -> Option<ContentMetadata> {
    let row = call_activity(|env, activity| {
        let argument = env.new_string(uri).map_err(|error| error.to_string())?;
        let argument_obj: &JObject = argument.as_ref();
        let value = env
            .call_method(
                &activity,
                jni_str!("cranposeDocumentInfo"),
                jni_sig!("(Ljava/lang/String;)Ljava/lang/String;"),
                &[JValue::Object(argument_obj)],
            )
            .and_then(|value| value.l())
            .map_err(|error| error.to_string())?;
        if value.is_null() {
            return Ok(None);
        }
        JString::cast_local(env, value)
            .map_err(|error| error.to_string())?
            .try_to_string(env)
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .ok()
    .flatten()?;
    parse_document(&format!("{uri}\t{row}"))
}

fn app() -> Result<&'static android_activity::AndroidApp, String> {
    APP.get()
        .ok_or_else(|| "Android file picker was not registered".to_string())
}

// ---- Document rows -------------------------------------------------------
//
// Every Java callback that carries documents uses the same newline-separated
// `uri\tname\tmime\tsize\tmodified` row format, so one parser serves picking,
// folder enumeration and resume.

fn parse_documents(text: &str) -> Vec<ContentMetadata> {
    text.lines().filter_map(parse_document).collect()
}

fn parse_document(row: &str) -> Option<ContentMetadata> {
    let mut fields = row.split('\t');
    let uri = fields.next()?;
    if uri.is_empty() {
        return None;
    }
    let name = fields.next().unwrap_or_default();
    let mime = fields.next().unwrap_or_default();
    let len = fields.next().unwrap_or_default();
    let modified = fields.next().unwrap_or_default();
    Some(ContentMetadata {
        name: if name.is_empty() {
            uri.rsplit('/').next().unwrap_or(uri).to_string()
        } else {
            name.to_string()
        },
        mime_type: (!mime.is_empty()).then(|| mime.to_string()),
        len: len.parse().ok(),
        modified_millis: modified.parse().ok(),
        identifier: uri.to_string(),
    })
}

/// Whether a document row describes a directory rather than a file.
fn is_directory(metadata: &ContentMetadata) -> bool {
    metadata.mime_type.as_deref() == Some("vnd.android.document/directory")
}

fn content_of(metadata: ContentMetadata) -> ContentHandle {
    Rc::new(AndroidDocument { metadata })
}

// ---- Picker --------------------------------------------------------------

struct AndroidFilePicker;

impl FilePicker for AndroidFilePicker {
    fn pick_file(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
        let picked = present_documents(Selection::Single, options);
        Box::pin(async move { Ok(picked.await?.into_iter().next().map(content_of)) })
    }

    fn pick_files(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
        let picked = present_documents(Selection::Multiple, options);
        Box::pin(async move { Ok(picked.await?.into_iter().map(content_of).collect()) })
    }

    fn pick_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
        let granted = present_tree(Grant::ReadOnly);
        Box::pin(async move { Ok(granted.await?.map(folder_of)) })
    }

    fn save_document(
        &self,
        request: SaveDocumentRequest,
    ) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
        let created = present_create_document(request);
        Box::pin(async move {
            Ok(created.await?.map(|uri| {
                Rc::new(AndroidSink {
                    uri,
                    file: std::cell::RefCell::new(None),
                }) as ContentSinkRef
            }))
        })
    }

    fn pick_writable_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<String>, FilePickerError>> {
        present_tree(Grant::Persistent)
    }

    fn take_recovered_pick(&self) -> Option<RecoveredPick> {
        take_recovered()
    }
}

/// Shared slot between a Java callback and the future awaiting it.
struct Slot<T> {
    result: Option<T>,
    waker: Option<Waker>,
}

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self {
            result: None,
            waker: None,
        }
    }
}

impl<T> Slot<T> {
    fn resolve(&mut self, value: T) {
        self.result = Some(value);
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

type Registry<T> = Mutex<HashMap<i64, Slot<T>>>;

fn document_picks() -> &'static Registry<Result<Vec<ContentMetadata>, FilePickerError>> {
    static SLOT: OnceLock<Registry<Result<Vec<ContentMetadata>, FilePickerError>>> =
        OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn tree_picks() -> &'static Registry<Result<Option<String>, FilePickerError>> {
    static SLOT: OnceLock<Registry<Result<Option<String>, FilePickerError>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn created_documents() -> &'static Registry<Result<Option<String>, FilePickerError>> {
    static SLOT: OnceLock<Registry<Result<Option<String>, FilePickerError>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn deliver<T>(registry: &'static Registry<T>, token: i64, value: T) {
    let mut registry = registry.lock().expect("picker registry poisoned");
    if let Some(slot) = registry.get_mut(&token) {
        slot.resolve(value);
    }
}

/// Future resolved when the Java callback reports a result for `token`.
struct SlotFuture<T: 'static> {
    registry: &'static Registry<T>,
    token: i64,
    missing: fn() -> T,
}

impl<T: 'static> Future for SlotFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        let mut registry = self.registry.lock().expect("picker registry poisoned");
        let Some(slot) = registry.get_mut(&self.token) else {
            return Poll::Ready((self.missing)());
        };
        match slot.result.take() {
            Some(value) => {
                registry.remove(&self.token);
                // Delivered live: drop the resume copy recorded in
                // `onActivityResult` so it is never replayed.
                clear_recovered();
                Poll::Ready(value)
            }
            None => {
                slot.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

/// Registers a pending request and launches the Java entry point.
fn begin<T: 'static>(
    registry: &'static Registry<T>,
    launch: impl FnOnce(i64) -> Result<(), String>,
    failed: fn(String) -> T,
    missing: fn() -> T,
) -> PickerFuture<T> {
    // Discard any orphaned selection from an earlier abandoned request; this
    // fresh one becomes the only thing the resume inbox can hold.
    clear_recovered();
    let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
    registry
        .lock()
        .expect("picker registry poisoned")
        .insert(token, Slot::default());
    if let Err(error) = launch(token) {
        registry
            .lock()
            .expect("picker registry poisoned")
            .remove(&token);
        return Box::pin(async move { failed(error) });
    }
    Box::pin(SlotFuture {
        registry,
        token,
        missing,
    })
}

/// How many documents an open chooser accepts.
#[derive(Clone, Copy)]
enum Selection {
    Single,
    Multiple,
}

/// Which tree grant a folder chooser takes.
#[derive(Clone, Copy)]
enum Grant {
    /// Read-only, for browsing a folder's contents.
    ReadOnly,
    /// Persisted read/write, for a folder the app keeps writing to.
    Persistent,
}

fn present_documents(
    selection: Selection,
    options: FilePickerOptions,
) -> PickerFuture<Result<Vec<ContentMetadata>, FilePickerError>> {
    let mime_types = options.mime_types().join("\n");
    begin(
        document_picks(),
        move |token| {
            call_activity(move |env, activity| {
                let types = env
                    .new_string(&mime_types)
                    .map_err(|error| error.to_string())?;
                let types_obj: &JObject = types.as_ref();
                let arguments = [JValue::Long(token), JValue::Object(types_obj)];
                let signature = jni_sig!("(JLjava/lang/String;)V");
                match selection {
                    Selection::Single => env.call_method(
                        &activity,
                        jni_str!("cranposePickFile"),
                        signature,
                        &arguments,
                    ),
                    Selection::Multiple => env.call_method(
                        &activity,
                        jni_str!("cranposePickFiles"),
                        signature,
                        &arguments,
                    ),
                }
                .map(|_| ())
                .map_err(|error| format!("failed to launch the Android chooser: {error}"))
            })
        },
        |error| Err(FilePickerError::Failed(error)),
        || Ok(Vec::new()),
    )
}

fn present_tree(grant: Grant) -> PickerFuture<Result<Option<String>, FilePickerError>> {
    begin(
        tree_picks(),
        move |token| {
            call_activity(move |env, activity| {
                let arguments = [JValue::Long(token)];
                let signature = jni_sig!("(J)V");
                match grant {
                    Grant::ReadOnly => env.call_method(
                        &activity,
                        jni_str!("cranposePickFolder"),
                        signature,
                        &arguments,
                    ),
                    Grant::Persistent => env.call_method(
                        &activity,
                        jni_str!("cranposePickWritableFolder"),
                        signature,
                        &arguments,
                    ),
                }
                .map(|_| ())
                .map_err(|error| format!("failed to launch the Android chooser: {error}"))
            })
        },
        |error| Err(FilePickerError::Failed(error)),
        || Ok(None),
    )
}

fn present_create_document(
    request: SaveDocumentRequest,
) -> PickerFuture<Result<Option<String>, FilePickerError>> {
    begin(
        created_documents(),
        move |token| {
            call_activity(move |env, activity| {
                let name = env
                    .new_string(&request.file_name)
                    .map_err(|error| error.to_string())?;
                let mime = env
                    .new_string(&request.mime_type)
                    .map_err(|error| error.to_string())?;
                let name_obj: &JObject = name.as_ref();
                let mime_obj: &JObject = mime.as_ref();
                env.call_method(
                    &activity,
                    jni_str!("cranposeCreateDocument"),
                    jni_sig!("(JLjava/lang/String;Ljava/lang/String;)V"),
                    &[
                        JValue::Long(token),
                        JValue::Object(name_obj),
                        JValue::Object(mime_obj),
                    ],
                )
                .map(|_| ())
                .map_err(|error| format!("failed to launch the Android chooser: {error}"))
            })
        },
        |error| Err(FilePickerError::Failed(error)),
        || Ok(None),
    )
}

/// Calls into the activity on whatever thread the caller is on, attaching to
/// the JVM as needed.
fn call_activity<T>(
    body: impl FnOnce(&mut jni::Env<'_>, JObject<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let app = app()?;
    crate::android_jni::with_android_activity_env(app, body)
}

// ---- Resume inbox --------------------------------------------------------
//
// Some Android devices destroy and recreate the activity (and with it the
// native app and its composition) when a system chooser covers it. A request in
// flight at that moment loses both its Java request token (a fresh activity
// instance starts at token 0) and the composition that was awaiting the result.
// The Java `onActivityResult` still runs on the recreated activity and records
// the granted selection here; the framework's launchers redeliver it. A live
// resolution clears the inbox, so a selection is never replayed.

/// A granted selection recorded for redelivery after an activity recreation.
struct Recoverable {
    flags: i32,
    entries: String,
}

fn recoverable() -> &'static Mutex<Vec<Recoverable>> {
    static SLOT: OnceLock<Mutex<Vec<Recoverable>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

fn clear_recovered() {
    recoverable().lock().expect("resume inbox poisoned").clear();
}

fn take_recovered() -> Option<RecoveredPick> {
    let entry = recoverable().lock().expect("resume inbox poisoned").pop()?;
    if entry.flags & FLAG_WRITABLE != 0 {
        return Some(RecoveredPick::WritableFolder(entry.entries));
    }
    if entry.flags & FLAG_FOLDER != 0 {
        return Some(RecoveredPick::Folder(folder_of(entry.entries)));
    }
    let documents = parse_documents(&entry.entries);
    match documents.len() {
        0 => None,
        1 => Some(RecoveredPick::File(content_of(
            documents.into_iter().next()?,
        ))),
        _ => Some(RecoveredPick::Files(
            documents.into_iter().map(content_of).collect(),
        )),
    }
}

// ---- Content -------------------------------------------------------------

/// A document addressed by a `content://` URI. It is opened on demand through
/// the provider's descriptor and never copied to the cache.
struct AndroidDocument {
    metadata: ContentMetadata,
}

impl Content for AndroidDocument {
    fn metadata(&self) -> ContentMetadata {
        self.metadata.clone()
    }

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        let uri = self.metadata.identifier.clone();
        Box::pin(async move {
            let file =
                open_content_uri(&uri).map_err(|error| ContentError::Io(error.to_string()))?;
            Ok(Rc::new(DescriptorReader {
                file: std::cell::RefCell::new(Some(file)),
            }) as ContentReaderRef)
        })
    }
}

struct DescriptorReader {
    file: std::cell::RefCell<Option<File>>,
}

impl ContentReader for DescriptorReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(async move {
            let mut slot = self.file.borrow_mut();
            let Some(file) = slot.as_mut() else {
                return Ok(None);
            };
            let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
            let mut filled = 0;
            while filled < buffer.len() {
                match file.read(&mut buffer[filled..]) {
                    Ok(0) => break,
                    Ok(read) => filled += read,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(ContentError::Io(error.to_string())),
                }
            }
            if filled == 0 {
                *slot = None;
                return Ok(None);
            }
            buffer.truncate(filled);
            Ok(Some(buffer))
        })
    }
}

/// A document being written through the provider's descriptor.
struct AndroidSink {
    uri: String,
    file: std::cell::RefCell<Option<File>>,
}

impl AndroidSink {
    fn writer(&self) -> Result<std::cell::RefMut<'_, Option<File>>, ContentError> {
        let mut slot = self.file.borrow_mut();
        if slot.is_none() {
            let fd = call_activity(|env, activity| {
                let uri = env.new_string(&self.uri).map_err(|e| e.to_string())?;
                let uri_obj: &JObject = uri.as_ref();
                env.call_method(
                    &activity,
                    jni_str!("cranposeOpenUriWrite"),
                    jni_sig!("(Ljava/lang/String;)I"),
                    &[JValue::Object(uri_obj)],
                )
                .and_then(|value| value.i())
                .map_err(|error| error.to_string())
            })
            .map_err(ContentError::Io)?;
            if fd < 0 {
                return Err(ContentError::PermissionDenied(self.uri.clone()));
            }
            // SAFETY: `cranposeOpenUriWrite` detaches the descriptor from its
            // `ParcelFileDescriptor`, transferring ownership to this process;
            // the `File` closes it on drop.
            *slot = Some(unsafe { File::from_raw_fd(fd) });
        }
        Ok(slot)
    }
}

impl ContentSink for AndroidSink {
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            let mut slot = self.writer()?;
            let file = slot
                .as_mut()
                .ok_or_else(|| ContentError::Io("sink is already finished".into()))?;
            file.write_all(&bytes)
                .map_err(|error| ContentError::Io(error.to_string()))
        })
    }

    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            // Opening lazily means an empty document still needs a descriptor
            // so the chosen file exists and is truncated.
            let mut slot = self.writer()?;
            let Some(mut file) = slot.take() else {
                return Ok(());
            };
            file.flush()
                .map_err(|error| ContentError::Io(error.to_string()))?;
            file.sync_all()
                .map_err(|error| ContentError::Io(error.to_string()))
        })
    }
}

// ---- Folders -------------------------------------------------------------

fn folder_of(tree_uri: String) -> ContentFolderRef {
    Rc::new(AndroidFolder { tree_uri })
}

/// A granted document tree. Immediate children are queried synchronously;
/// the whole tree is streamed by the Java walker.
struct AndroidFolder {
    tree_uri: String,
}

impl AndroidFolder {
    fn children(&self, document_id: &str) -> Result<Vec<ContentMetadata>, ContentError> {
        let rows = call_activity(|env, activity| {
            let tree = env.new_string(&self.tree_uri).map_err(|e| e.to_string())?;
            let document = env.new_string(document_id).map_err(|e| e.to_string())?;
            let tree_obj: &JObject = tree.as_ref();
            let document_obj: &JObject = document.as_ref();
            let value = env
                .call_method(
                    &activity,
                    jni_str!("cranposeFolderChildren"),
                    jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                    &[JValue::Object(tree_obj), JValue::Object(document_obj)],
                )
                .and_then(|value| value.l())
                .map_err(|error| error.to_string())?;
            if value.is_null() {
                return Ok(None);
            }
            JString::cast_local(env, value)
                .map_err(|error| error.to_string())?
                .try_to_string(env)
                .map(Some)
                .map_err(|error| error.to_string())
        })
        .map_err(ContentError::Io)?;
        let rows = rows.ok_or_else(|| ContentError::Io("folder is not readable".into()))?;
        Ok(parse_documents(&rows))
    }
}

impl ContentFolder for AndroidFolder {
    fn metadata(&self) -> ContentMetadata {
        ContentMetadata::named(
            self.tree_uri
                .rsplit(['/', ':'])
                .find(|segment| !segment.is_empty())
                .unwrap_or(&self.tree_uri)
                .to_string(),
        )
        .with_identifier(self.tree_uri.clone())
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        Box::pin(async move {
            Ok(self
                .children("")?
                .into_iter()
                .map(|metadata| {
                    if is_directory(&metadata) {
                        ContentEntry::Folder(Rc::new(AndroidFolder {
                            tree_uri: metadata.identifier,
                        }))
                    } else {
                        ContentEntry::File(content_of(metadata))
                    }
                })
                .collect())
        })
    }

    fn stream_files(&self) -> Option<ContentStreamRef> {
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        folder_walks()
            .lock()
            .expect("folder walk registry poisoned")
            .insert(token, FolderWalk::default());
        let tree_uri = self.tree_uri.clone();
        let launched = call_activity(move |env, activity| {
            let uri = env.new_string(&tree_uri).map_err(|e| e.to_string())?;
            let uri_obj: &JObject = uri.as_ref();
            env.call_method(
                &activity,
                jni_str!("cranposeStreamFolder"),
                jni_sig!("(JLjava/lang/String;)V"),
                &[JValue::Long(token), JValue::Object(uri_obj)],
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        });
        if let Err(error) = launched {
            let mut registry = folder_walks()
                .lock()
                .expect("folder walk registry poisoned");
            if let Some(walk) = registry.get_mut(&token) {
                walk.finished = true;
                walk.error = Some(error);
            }
        }
        Some(Rc::new(AndroidFolderStream { token }))
    }
}

/// Per-token state for a streaming folder walk, written by the Java callbacks
/// and drained by the awaiting collector.
#[derive(Default)]
struct FolderWalk {
    documents: std::collections::VecDeque<ContentMetadata>,
    finished: bool,
    error: Option<String>,
    produced: usize,
    waker: Option<Waker>,
}

fn folder_walks() -> &'static Mutex<HashMap<i64, FolderWalk>> {
    static SLOT: OnceLock<Mutex<HashMap<i64, FolderWalk>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Streams files discovered under a granted tree. Dropping it discards the
/// registry slot, and the Java walker stops when its next batch is refused.
struct AndroidFolderStream {
    token: i64,
}

impl ContentStream for AndroidFolderStream {
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>> {
        Box::pin(FolderNext { token: self.token })
    }

    fn produced(&self) -> Option<usize> {
        folder_walks()
            .lock()
            .expect("folder walk registry poisoned")
            .get(&self.token)
            .map(|walk| walk.produced)
    }
}

impl Drop for AndroidFolderStream {
    fn drop(&mut self) {
        folder_walks()
            .lock()
            .expect("folder walk registry poisoned")
            .remove(&self.token);
    }
}

struct FolderNext {
    token: i64,
}

impl Future for FolderNext {
    type Output = Result<Option<ContentHandle>, ContentError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut registry = folder_walks()
            .lock()
            .expect("folder walk registry poisoned");
        let Some(walk) = registry.get_mut(&self.token) else {
            return Poll::Ready(Ok(None));
        };
        if let Some(metadata) = walk.documents.pop_front() {
            walk.produced += 1;
            return Poll::Ready(Ok(Some(content_of(metadata))));
        }
        if let Some(error) = walk.error.take() {
            return Poll::Ready(Err(ContentError::Io(error)));
        }
        if walk.finished {
            return Poll::Ready(Ok(None));
        }
        walk.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

/// Opens a `content://` document for reading, returning a [`File`] backed by
/// the provider's descriptor. Nothing is copied; the descriptor is detached
/// from its `ParcelFileDescriptor` so the returned `File` owns and closes it.
/// Callable from any thread (it attaches to the JVM as needed), so a media
/// engine can stream a track straight from the provider.
pub fn open_content_uri(uri: &str) -> io::Result<File> {
    let fd = call_activity(|env, activity| {
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
    .map_err(io::Error::other)?;
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

// ---- Java callbacks ------------------------------------------------------

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

/// Java callback: records a granted selection in the resume inbox (see above).
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeRecordResumablePick<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    flags: jint,
    entries: JString<'local>,
) {
    let Some(entries) = read_optional_jstring(&mut env, entries) else {
        return;
    };
    recoverable()
        .lock()
        .expect("resume inbox poisoned")
        .push(Recoverable { flags, entries });
}

/// Java callback delivering a document chooser result.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFilePicked<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    entries: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    let entries = read_optional_jstring(&mut env, entries);
    let error = read_optional_jstring(&mut env, error);
    let value = if cancelled {
        Ok(Vec::new())
    } else if let Some(error) = error {
        Err(FilePickerError::Failed(error))
    } else {
        Ok(parse_documents(entries.as_deref().unwrap_or_default()))
    };
    deliver(document_picks(), token, value);
}

/// Java callback: a folder (or persistent writable folder) was granted.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFolderPicked<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    uri: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    deliver(
        tree_picks(),
        token,
        tree_result(&mut env, uri, cancelled, error),
    );
}

/// Java callback: a persistent writable folder was granted.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnWritableFolderPicked<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    uri: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    deliver(
        tree_picks(),
        token,
        tree_result(&mut env, uri, cancelled, error),
    );
}

/// Java callback: a save destination was created.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnDocumentCreated<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    uri: JString<'local>,
    cancelled: jboolean,
    error: JString<'local>,
) {
    deliver(
        created_documents(),
        token,
        tree_result(&mut env, uri, cancelled, error),
    );
}

fn tree_result(
    env: &mut EnvUnowned<'_>,
    uri: JString<'_>,
    cancelled: jboolean,
    error: JString<'_>,
) -> Result<Option<String>, FilePickerError> {
    let uri = read_optional_jstring(env, uri);
    let error = read_optional_jstring(env, error);
    if cancelled {
        return Ok(None);
    }
    if let Some(error) = error {
        return Err(FilePickerError::Failed(error));
    }
    Ok(uri)
}

/// Java callback: a batch of newly-discovered files. Returns `false` once the
/// collector has dropped the stream, so the Java walker can stop.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFolderEntries<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    entries: JString<'local>,
) -> jboolean {
    let documents = read_optional_jstring(&mut env, entries)
        .map(|rows| parse_documents(&rows))
        .unwrap_or_default();
    let mut registry = folder_walks()
        .lock()
        .expect("folder walk registry poisoned");
    match registry.get_mut(&token) {
        Some(walk) => {
            walk.documents.extend(documents);
            if let Some(waker) = walk.waker.take() {
                waker.wake();
            }
            true
        }
        None => false,
    }
}

/// Java callback: the walk finished, with an optional error.
#[doc(hidden)]
#[no_mangle]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnFolderFinished<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    token: jlong,
    error: JString<'local>,
) {
    let error = read_optional_jstring(&mut env, error);
    let mut registry = folder_walks()
        .lock()
        .expect("folder walk registry poisoned");
    if let Some(walk) = registry.get_mut(&token) {
        walk.finished = true;
        walk.error = error;
        if let Some(waker) = walk.waker.take() {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_rows_carry_provider_metadata() {
        let rows = "content://docs/1\treport.pdf\tapplication/pdf\t2048\t1700000000000\n\
                    content://docs/2\t\t\t\t";
        let documents = parse_documents(rows);
        assert_eq!(documents.len(), 2);
        assert_eq!(documents[0].name, "report.pdf");
        assert_eq!(documents[0].mime_type.as_deref(), Some("application/pdf"));
        assert_eq!(documents[0].len, Some(2048));
        assert_eq!(documents[0].modified_millis, Some(1_700_000_000_000));
        // A provider that reports nothing still yields a usable display name.
        assert_eq!(documents[1].name, "2");
        assert_eq!(documents[1].len, None);
    }

    #[test]
    fn directory_rows_are_told_apart_from_files() {
        let directory =
            parse_document("content://docs/tree\tMusic\tvnd.android.document/directory\t\t")
                .expect("a row parses");
        assert!(is_directory(&directory));
        let file =
            parse_document("content://docs/3\ta.mp3\taudio/mpeg\t10\t").expect("a row parses");
        assert!(!is_directory(&file));
    }

    #[test]
    fn blank_rows_are_skipped() {
        assert!(parse_documents("\n\t\n").is_empty());
    }
}
