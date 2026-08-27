//! Native, cross-platform file, folder and document choosers.
//!
//! Every chooser resolves to the streaming content model: a file becomes a
//! [`ContentHandle`], a folder becomes a [`ContentFolderRef`], and a save
//! destination becomes a [`ContentSinkRef`]. None of them is a filesystem path.
//! That is deliberate — on Android (Storage Access Framework `content://`
//! trees), iOS (`UIDocumentPicker` security-scoped URLs) and the web (File
//! System Access handles) the user can choose locations served by *system*
//! document providers, such as a mounted WebDAV share or cloud storage, which
//! have no local path.
//!
//! Applications do not call this trait. They compose the launchers in
//! [`crate::launcher`], which own the request across host recreation and hand
//! the result back through a callback.

use std::{cell::RefCell, future::Future, pin::Pin, rc::Rc};

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOfWithPolicy};
use cranpose_macros::composable;

use crate::content::{ContentError, ContentFolderRef, ContentHandle, ContentSinkRef};

/// Errors produced while presenting a chooser.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum FilePickerError {
    /// Presenting the chooser failed.
    #[error("file picker failed: {0}")]
    Failed(String),
    /// Reading or writing the chosen content failed.
    #[error(transparent)]
    Content(#[from] ContentError),
    /// The chooser requires a cranpose-services feature that is not enabled.
    #[error("{operation} requires cranpose-services feature `{feature}`")]
    UnsupportedFeature {
        /// The attempted operation.
        operation: &'static str,
        /// The feature that enables it.
        feature: &'static str,
    },
    /// No chooser is available on this platform/build.
    #[error("file picking is not available on this platform")]
    UnsupportedPlatform,
}

/// A `'static` future returned by chooser operations, polled on the UI thread.
pub type PickerFuture<T> = Pin<Box<dyn Future<Output = T>>>;

/// A named filter limiting the file types a chooser offers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileFilter {
    /// Human-readable group name, for example `"Audio"`.
    pub label: String,
    /// Accepted extensions without the leading dot, for example `["mp3", "flac"]`.
    pub extensions: Vec<String>,
    /// Accepted MIME types. Android's Storage Access Framework and the web
    /// filter by MIME rather than extension; backends that only understand
    /// extensions ignore this.
    pub mime_types: Vec<String>,
}

impl FileFilter {
    /// Creates a filter from a label and a set of extensions.
    pub fn new(label: impl Into<String>, extensions: &[&str]) -> Self {
        Self {
            label: label.into(),
            extensions: extensions.iter().map(|ext| (*ext).to_string()).collect(),
            mime_types: Vec::new(),
        }
    }

    /// Adds the MIME types this filter accepts.
    pub fn with_mime_types(mut self, mime_types: &[&str]) -> Self {
        self.mime_types = mime_types.iter().map(|mime| (*mime).to_string()).collect();
        self
    }
}

/// Options controlling a chooser request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilePickerOptions {
    /// Dialog title.
    pub title: Option<String>,
    /// File-type filters (ignored by folder choosers and some platforms).
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

    /// Every MIME type across the filters, for backends that filter by MIME.
    pub fn mime_types(&self) -> Vec<String> {
        self.filters
            .iter()
            .flat_map(|filter| filter.mime_types.iter().cloned())
            .collect()
    }
}

/// A request for a user-named destination to stream a document into.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SaveDocumentRequest {
    /// Suggested file name (including extension).
    pub file_name: String,
    /// MIME type, used by backends that need one (Android's
    /// `ACTION_CREATE_DOCUMENT`, the web download).
    pub mime_type: String,
    /// Dialog title.
    pub title: Option<String>,
}

impl SaveDocumentRequest {
    /// Creates a request for `file_name` of `mime_type`.
    pub fn new(file_name: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            mime_type: mime_type.into(),
            title: None,
        }
    }

    /// Sets the dialog title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// A chooser result the host recovered after the composition that requested it
/// was destroyed.
///
/// Android can destroy and recreate the activity — and with it the native app —
/// while the system chooser is in front. The platform backend records the
/// granted selection and the framework's launchers redeliver it. This type is
/// the framework's own transport; applications never construct or drain it.
#[doc(hidden)]
pub enum RecoveredPick {
    /// A single recovered file.
    File(ContentHandle),
    /// Recovered files from a multi-selection.
    Files(Vec<ContentHandle>),
    /// A recovered folder grant.
    Folder(ContentFolderRef),
    /// A recovered persistent writable-folder grant, as its durable handle.
    WritableFolder(String),
}

/// Presents the system's file, folder and document choosers.
///
/// Implemented by the platform backends and consumed by [`crate::launcher`].
pub trait FilePicker {
    /// Presents a single-file chooser. Resolves to `None` if cancelled.
    fn pick_file(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>>;

    /// Presents a multi-file chooser. Resolves to an empty vector if cancelled.
    ///
    /// The default presents the single-file chooser, for backends whose system
    /// chooser has no multi-selection mode.
    fn pick_files(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
        let single = self.pick_file(options);
        Box::pin(async move { Ok(single.await?.into_iter().collect()) })
    }

    /// Presents a folder chooser. Resolves to `None` if cancelled.
    fn pick_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>>;

    /// Presents a save-destination chooser and opens a sink on the chosen
    /// document. Resolves to `None` if cancelled.
    fn save_document(
        &self,
        request: SaveDocumentRequest,
    ) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
        let _ = request;
        Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
    }

    /// Presents a chooser for a folder the app may keep writing to across runs,
    /// resolving to the durable handle accepted by
    /// [`crate::writable_folder::open_writable_folder`].
    fn pick_writable_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<String>, FilePickerError>> {
        let _ = options;
        Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
    }

    /// Hands back a selection the host recovered after the requesting
    /// composition was destroyed. Framework-internal; the launchers call it.
    /// Backends that never lose a result keep the default.
    #[doc(hidden)]
    fn take_recovered_pick(&self) -> Option<RecoveredPick> {
        None
    }
}

/// Shared handle to a [`FilePicker`].
pub type FilePickerRef = Rc<dyn FilePicker>;

thread_local! {
    static PLATFORM_FILE_PICKER: RefCell<Option<FilePickerRef>> = const { RefCell::new(None) };
}

/// Registers the platform-provided chooser (Android SAF / iOS UIDocumentPicker).
///
/// The cranpose crate's Android and iOS backends call this during startup, when
/// they have access to the Activity / root view controller. Once registered it
/// takes precedence over the built-in desktop/web choosers.
pub fn set_platform_file_picker(picker: FilePickerRef) {
    PLATFORM_FILE_PICKER.with(|cell| *cell.borrow_mut() = Some(picker));
}

/// Removes any registered platform chooser (used in tests and teardown).
pub fn clear_platform_file_picker() {
    PLATFORM_FILE_PICKER.with(|cell| *cell.borrow_mut() = None);
}

fn registered_platform_file_picker() -> Option<FilePickerRef> {
    PLATFORM_FILE_PICKER.with(|cell| cell.borrow().clone())
}

/// The chooser installed by [`ProvideFilePicker`]: a registered platform
/// chooser if present, otherwise the built-in backend for this target.
struct PlatformFilePicker;

impl FilePicker for PlatformFilePicker {
    fn pick_file(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
        match registered_platform_file_picker() {
            Some(picker) => picker.pick_file(options),
            None => builtin::pick_file(options),
        }
    }

    fn pick_files(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
        match registered_platform_file_picker() {
            Some(picker) => picker.pick_files(options),
            None => builtin::pick_files(options),
        }
    }

    fn pick_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
        match registered_platform_file_picker() {
            Some(picker) => picker.pick_folder(options),
            None => builtin::pick_folder(options),
        }
    }

    fn save_document(
        &self,
        request: SaveDocumentRequest,
    ) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
        match registered_platform_file_picker() {
            Some(picker) => picker.save_document(request),
            None => builtin::save_document(request),
        }
    }

    fn pick_writable_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<String>, FilePickerError>> {
        match registered_platform_file_picker() {
            Some(picker) => picker.pick_writable_folder(options),
            None => builtin::pick_writable_folder(options),
        }
    }

    fn take_recovered_pick(&self) -> Option<RecoveredPick> {
        registered_platform_file_picker().and_then(|picker| picker.take_recovered_pick())
    }
}

/// The default chooser (the platform backend).
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

mod builtin {

    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios"),
        feature = "file-picker-native"
    ))]
    pub(super) use super::desktop::{
        pick_file, pick_files, pick_folder, pick_writable_folder, save_document,
    };
    #[cfg(all(target_arch = "wasm32", feature = "file-picker-web"))]
    pub(super) use super::web::{
        pick_file, pick_files, pick_folder, pick_writable_folder, save_document,
    };

    #[cfg(not(any(
        all(
            not(target_arch = "wasm32"),
            not(target_os = "android"),
            not(target_os = "ios"),
            feature = "file-picker-native"
        ),
        all(target_arch = "wasm32", feature = "file-picker-web")
    )))]
    mod unsupported {
        use crate::{
            content::{ContentFolderRef, ContentHandle, ContentSinkRef},
            file_picker::{FilePickerError, FilePickerOptions, PickerFuture, SaveDocumentRequest},
        };

        pub(in crate::file_picker) fn pick_file(
            _options: FilePickerOptions,
        ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
            Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
        }

        pub(in crate::file_picker) fn pick_files(
            _options: FilePickerOptions,
        ) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
            Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
        }

        pub(in crate::file_picker) fn pick_folder(
            _options: FilePickerOptions,
        ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
            Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
        }

        pub(in crate::file_picker) fn save_document(
            _request: SaveDocumentRequest,
        ) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
            Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
        }

        pub(in crate::file_picker) fn pick_writable_folder(
            _options: FilePickerOptions,
        ) -> PickerFuture<Result<Option<String>, FilePickerError>> {
            Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
        }
    }

    #[cfg(not(any(
        all(
            not(target_arch = "wasm32"),
            not(target_os = "android"),
            not(target_os = "ios"),
            feature = "file-picker-native"
        ),
        all(target_arch = "wasm32", feature = "file-picker-web")
    )))]
    pub(super) use unsupported::{
        pick_file, pick_files, pick_folder, pick_writable_folder, save_document,
    };
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
            .with_filter(FileFilter::new("Audio", &["mp3", "flac"]).with_mime_types(&["audio/*"]));
        assert_eq!(options.title.as_deref(), Some("Pick audio"));
        assert_eq!(options.filters.len(), 1);
        assert_eq!(options.filters[0].extensions, vec!["mp3", "flac"]);
        assert_eq!(options.mime_types(), vec!["audio/*"]);
    }

    #[test]
    fn default_picker_is_created() {
        let picker = default_file_picker();
        assert_eq!(Rc::strong_count(&picker), 1);
    }

    struct Marker;

    impl FilePicker for Marker {
        fn pick_file(
            &self,
            _options: FilePickerOptions,
        ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
            Box::pin(async {
                Ok(Some(
                    crate::content::BytesContent::named("marker.txt", b"marker".to_vec()).handle(),
                ))
            })
        }

        fn pick_folder(
            &self,
            _options: FilePickerOptions,
        ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
            Box::pin(async { Ok(None) })
        }
    }

    #[test]
    fn registered_platform_picker_takes_precedence() {
        clear_platform_file_picker();
        assert!(registered_platform_file_picker().is_none());
        set_platform_file_picker(Rc::new(Marker));
        assert!(registered_platform_file_picker().is_some());

        let picked =
            pollster::block_on(default_file_picker().pick_file(FilePickerOptions::default()))
                .expect("the marker picker resolves")
                .expect("the marker picker picks a file");
        assert_eq!(picked.metadata().name, "marker.txt");
        clear_platform_file_picker();
    }

    #[test]
    fn multi_selection_falls_back_to_the_single_chooser() {
        clear_platform_file_picker();
        set_platform_file_picker(Rc::new(Marker));
        let picked =
            pollster::block_on(default_file_picker().pick_files(FilePickerOptions::default()))
                .expect("the marker picker resolves");
        assert_eq!(picked.len(), 1);
        clear_platform_file_picker();
    }

    #[test]
    fn unsupported_operations_report_the_platform_gap() {
        clear_platform_file_picker();
        set_platform_file_picker(Rc::new(Marker));
        let saved = pollster::block_on(
            default_file_picker().save_document(SaveDocumentRequest::new("a.txt", "text/plain")),
        );
        let Err(error) = saved else {
            panic!("the marker picker offers no save destination");
        };
        assert_eq!(error, FilePickerError::UnsupportedPlatform);
        clear_platform_file_picker();
    }
}
