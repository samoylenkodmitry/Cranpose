//! Native, cross-platform file and folder picker.
//!
//! Applications pick through the [`FilePicker`] obtained from
//! [`local_file_picker`] and receive an opaque [`PickedEntry`] handle, not a
//! filesystem path. This is deliberate: on Android (Storage Access Framework
//! `content://` trees), iOS (`UIDocumentPicker` security-scoped URLs) and the
//! web (File System Access handles) the user can choose folders served by the
//! *system* document providers — a mounted WebDAV share, cloud storage, etc. —
//! which do not map to a local path. [`PickedEntry::read_bytes`] and
//! [`PickedEntry::list`] read through the originating provider on every
//! platform.
//!
//! The picker is asynchronous and must run on the UI thread, so callers drive
//! it from [`cranpose_core::LaunchedEffectAsync`].

use cranpose_core::compositionLocalOfWithPolicy;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

/// Errors produced while presenting a picker or reading a picked entry.
#[derive(thiserror::Error, Debug, Clone)]
pub enum FilePickerError {
    /// Presenting the picker failed.
    #[error("file picker failed: {0}")]
    Failed(String),
    /// Reading or listing a picked entry failed.
    #[error("reading picked entry failed: {0}")]
    ReadFailed(String),
    /// `list` was called on a file, or `read_bytes` on a folder.
    #[error("picked entry is a {actual}, expected a {expected}")]
    WrongKind {
        /// The kind the entry actually is.
        actual: &'static str,
        /// The kind the operation expected.
        expected: &'static str,
    },
    /// The picker requires a cranpose-services feature that is not enabled.
    #[error("{operation} requires cranpose-services feature `{feature}`")]
    UnsupportedFeature {
        /// The attempted operation.
        operation: &'static str,
        /// The feature that enables it.
        feature: &'static str,
    },
    /// No picker is available on this platform/build.
    #[error("file picking is not available on this platform")]
    UnsupportedPlatform,
}

/// A `'static` future returned by picker operations, polled on the UI thread.
pub type PickerFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// Whether a [`PickedEntry`] is a single file or a folder/tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickedKind {
    /// A single file.
    File,
    /// A folder (directory tree).
    Folder,
}

/// A named filter limiting the file types offered by the picker.
#[derive(Clone, Debug, Default)]
pub struct FileFilter {
    /// Human-readable group name, for example `"Audio"`.
    pub label: String,
    /// Accepted extensions without the leading dot, for example `["mp3", "flac"]`.
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// Creates a filter from a label and a set of extensions.
    pub fn new(label: impl Into<String>, extensions: &[&str]) -> Self {
        Self {
            label: label.into(),
            extensions: extensions.iter().map(|ext| (*ext).to_string()).collect(),
        }
    }
}

/// Options controlling a pick request.
#[derive(Clone, Debug, Default)]
pub struct FilePickerOptions {
    /// Dialog title.
    pub title: Option<String>,
    /// File-type filters (ignored by folder pickers and some platforms).
    pub filters: Vec<FileFilter>,
}

impl FilePickerOptions {
    /// Sets the dialog title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a file-type filter.
    pub fn with_filter(mut self, filter: FileFilter) -> Self {
        self.filters.push(filter);
        self
    }
}

/// An opaque handle to a picked file or folder.
///
/// This is **not** guaranteed to be a filesystem path. Use [`read_bytes`] to
/// read a file and [`list`] to enumerate a folder's immediate children; both
/// go through the originating system provider.
///
/// [`read_bytes`]: PickedEntry::read_bytes
/// [`list`]: PickedEntry::list
pub trait PickedEntry {
    /// The display name (last path/URI component).
    fn name(&self) -> String;

    /// Whether this entry is a file or a folder.
    fn kind(&self) -> PickedKind;

    /// A user-facing identifier (a path or a `content://`/`file://` URI). Not
    /// guaranteed to be a usable filesystem path.
    fn display_path(&self) -> String;

    /// Reads the full contents of a [`PickedKind::File`] entry.
    fn read_bytes(&self) -> PickerFuture<Result<Vec<u8>, FilePickerError>>;

    /// Lists the immediate children of a [`PickedKind::Folder`] entry.
    fn list(&self) -> PickerFuture<Result<Vec<PickedEntryRef>, FilePickerError>>;
}

/// Shared handle to a [`PickedEntry`].
pub type PickedEntryRef = Rc<dyn PickedEntry>;

/// A folder being enumerated, yielding its files as the provider discovers
/// them.
///
/// Walking a deep tree from a slow provider (cloud storage, a mounted WebDAV
/// share) can take a long time, so the files are delivered incrementally rather
/// than all at once: poll [`take_ready`] each frame to drain newly-found files,
/// and [`is_finished`] to know when the walk is done. Callers can show the
/// running count and start playing the first file without waiting for the rest.
///
/// [`take_ready`]: FolderStream::take_ready
/// [`is_finished`]: FolderStream::is_finished
pub trait FolderStream {
    /// Files discovered since the previous call (drains the ready queue).
    fn take_ready(&self) -> Vec<PickedEntryRef>;

    /// Whether enumeration has finished (no more files will be discovered).
    fn is_finished(&self) -> bool;

    /// Takes the error that ended enumeration early, if any.
    fn take_error(&self) -> Option<FilePickerError>;
}

/// Shared handle to a [`FolderStream`].
pub type FolderStreamRef = Rc<dyn FolderStream>;

/// Recursively collects every [`PickedKind::File`] under `entry`.
fn collect_files(entry: PickedEntryRef) -> PickerFuture<Vec<PickedEntryRef>> {
    Box::pin(async move {
        match entry.kind() {
            PickedKind::File => vec![entry],
            PickedKind::Folder => {
                let mut files = Vec::new();
                if let Ok(children) = entry.list().await {
                    for child in children {
                        files.extend(collect_files(child).await);
                    }
                }
                files
            }
        }
    })
}

/// A [`FolderStream`] whose files were all gathered up front (the default for
/// providers that enumerate eagerly, e.g. the local filesystem). It yields
/// everything on the first poll and is immediately finished.
struct ReadyFolderStream {
    ready: RefCell<Vec<PickedEntryRef>>,
}

impl FolderStream for ReadyFolderStream {
    fn take_ready(&self) -> Vec<PickedEntryRef> {
        std::mem::take(&mut self.ready.borrow_mut())
    }

    fn is_finished(&self) -> bool {
        true
    }

    fn take_error(&self) -> Option<FilePickerError> {
        None
    }
}

/// Wraps an eager [`FilePicker::pick_folder`] as a [`FolderStream`] by walking
/// the whole tree up front. Used as the default for every backend that does not
/// stream natively.
fn eager_folder_stream(
    folder: PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>>,
) -> PickerFuture<Result<Option<FolderStreamRef>, FilePickerError>> {
    Box::pin(async move {
        match folder.await? {
            None => Ok(None),
            Some(entry) => {
                let files = collect_files(entry).await;
                Ok(Some(Rc::new(ReadyFolderStream {
                    ready: RefCell::new(files),
                }) as FolderStreamRef))
            }
        }
    })
}

/// Presents native file and folder pickers.
pub trait FilePicker {
    /// Presents a single-file picker. Resolves to `None` if cancelled.
    fn pick_file(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>>;

    /// Presents a folder/tree picker. Resolves to `None` if cancelled.
    fn pick_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>>;

    /// Presents a folder picker and streams the tree's files as they are
    /// discovered (see [`FolderStream`]).
    ///
    /// The default walks the picked folder eagerly via [`pick_folder`] and
    /// yields every file at once; backends served by a slow provider (Android's
    /// Storage Access Framework) override this to stream during the walk.
    /// Resolves to `None` if cancelled.
    ///
    /// [`pick_folder`]: FilePicker::pick_folder
    fn pick_folder_streaming(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<FolderStreamRef>, FilePickerError>> {
        eager_folder_stream(self.pick_folder(options))
    }
}

/// Shared handle to a [`FilePicker`].
pub type FilePickerRef = Rc<dyn FilePicker>;

thread_local! {
    static PLATFORM_FILE_PICKER: RefCell<Option<FilePickerRef>> = const { RefCell::new(None) };
}

/// Registers the platform-provided picker (Android SAF / iOS UIDocumentPicker).
///
/// The cranpose crate's Android and iOS backends call this during startup, when
/// they have access to the Activity / root view controller. Once registered it
/// takes precedence over the built-in desktop/web pickers.
pub fn set_platform_file_picker(picker: FilePickerRef) {
    PLATFORM_FILE_PICKER.with(|cell| *cell.borrow_mut() = Some(picker));
}

/// Removes any registered platform picker (used in tests and teardown).
pub fn clear_platform_file_picker() {
    PLATFORM_FILE_PICKER.with(|cell| *cell.borrow_mut() = None);
}

fn registered_platform_file_picker() -> Option<FilePickerRef> {
    PLATFORM_FILE_PICKER.with(|cell| cell.borrow().clone())
}

/// The picker installed by [`ProvideFilePicker`]: a registered platform picker
/// if present, otherwise the built-in backend for this target.
struct PlatformFilePicker;

impl FilePicker for PlatformFilePicker {
    fn pick_file(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
        if let Some(picker) = registered_platform_file_picker() {
            return picker.pick_file(options);
        }
        builtin_pick(options, PickedKind::File)
    }

    fn pick_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
        if let Some(picker) = registered_platform_file_picker() {
            return picker.pick_folder(options);
        }
        builtin_pick(options, PickedKind::Folder)
    }

    fn pick_folder_streaming(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<FolderStreamRef>, FilePickerError>> {
        if let Some(picker) = registered_platform_file_picker() {
            return picker.pick_folder_streaming(options);
        }
        eager_folder_stream(builtin_pick(options, PickedKind::Folder))
    }
}

#[cfg_attr(
    not(any(feature = "file-picker-native", feature = "file-picker-web")),
    allow(unused_variables)
)]
fn builtin_pick(
    options: FilePickerOptions,
    kind: PickedKind,
) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios"),
        feature = "file-picker-native"
    ))]
    {
        return desktop::pick(options, kind);
    }

    #[cfg(all(target_arch = "wasm32", feature = "file-picker-web"))]
    {
        return web::pick(options, kind);
    }

    #[allow(unreachable_code)]
    Box::pin(async move { Err(FilePickerError::UnsupportedPlatform) })
}

/// The default picker (the platform backend).
pub fn default_file_picker() -> FilePickerRef {
    Rc::new(PlatformFilePicker)
}

/// The [`CompositionLocal`] carrying the active [`FilePicker`].
pub fn local_file_picker() -> CompositionLocal<FilePickerRef> {
    thread_local! {
        static LOCAL_FILE_PICKER: RefCell<Option<CompositionLocal<FilePickerRef>>> = const { RefCell::new(None) };
    }

    LOCAL_FILE_PICKER.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_file_picker, Rc::ptr_eq))
            .clone()
    })
}

/// Provides the default [`FilePicker`] to descendant composables.
#[allow(non_snake_case)]
#[composable]
pub fn ProvideFilePicker(content: impl FnOnce()) {
    let picker = cranpose_core::remember(default_file_picker).with(|state| state.clone());
    let picker_local = local_file_picker();

    CompositionLocalProvider(vec![picker_local.provides(picker)], move || {
        content();
    });
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "file-picker-native"
))]
mod desktop;

#[cfg(all(target_arch = "wasm32", feature = "file-picker-web"))]
mod web;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_builder_sets_title_and_filters() {
        let options = FilePickerOptions::default()
            .with_title("Pick audio")
            .with_filter(FileFilter::new("Audio", &["mp3", "flac"]));
        assert_eq!(options.title.as_deref(), Some("Pick audio"));
        assert_eq!(options.filters.len(), 1);
        assert_eq!(options.filters[0].extensions, vec!["mp3", "flac"]);
    }

    #[test]
    fn default_picker_is_created() {
        let picker = default_file_picker();
        assert_eq!(Rc::strong_count(&picker), 1);
    }

    #[test]
    fn registered_platform_picker_takes_precedence() {
        struct Marker;
        impl FilePicker for Marker {
            fn pick_file(
                &self,
                _options: FilePickerOptions,
            ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
                Box::pin(async { Err(FilePickerError::Failed("marker".into())) })
            }
            fn pick_folder(
                &self,
                _options: FilePickerOptions,
            ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
                Box::pin(async { Err(FilePickerError::Failed("marker".into())) })
            }
        }

        clear_platform_file_picker();
        assert!(registered_platform_file_picker().is_none());
        set_platform_file_picker(Rc::new(Marker));
        assert!(registered_platform_file_picker().is_some());

        let result =
            pollster::block_on(default_file_picker().pick_file(FilePickerOptions::default()));
        assert!(matches!(result, Err(FilePickerError::Failed(_))));
        clear_platform_file_picker();
    }
}
