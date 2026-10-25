#![expect(unsafe_code, reason = "Objective-C messages to UIKit")]

use std::{
    cell::{Cell, RefCell},
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
};

use cranpose_services::{
    BlockingSink, Content, ContentEntry, ContentError, ContentFolder, ContentFolderRef,
    ContentFuture, ContentHandle, ContentMetadata, ContentReader, ContentReaderRef, ContentSink,
    ContentSinkRef, FilePicker, FilePickerError, FilePickerOptions, PickerFuture,
    SaveDocumentRequest, open_threaded, set_platform_file_picker, threaded_sink,
};
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use objc2_ui_kit::{
    UIApplication, UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController,
    UIWindowScene,
};
use objc2_uniform_type_identifiers::{UTType, UTTypeDirectory, UTTypeItem};

pub(crate) fn register() {
    set_platform_file_picker(Rc::new(IosFilePicker));
}

struct IosFilePicker;

impl FilePicker for IosFilePicker {
    fn pick_file(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
        let picked = present_open(Kind::File, false);
        Box::pin(async move {
            Ok(picked
                .await?
                .into_iter()
                .next()
                .map(|url| scoped_file(url.clone(), url_path(&url))))
        })
    }

    fn pick_files(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
        let picked = present_open(Kind::File, true);
        Box::pin(async move {
            Ok(picked
                .await?
                .into_iter()
                .map(|url| scoped_file(url.clone(), url_path(&url)))
                .collect())
        })
    }

    fn pick_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
        let picked = present_open(Kind::Folder, false);
        Box::pin(async move {
            Ok(picked.await?.into_iter().next().map(|url| {
                let path = url_path(&url);
                Rc::new(IosFolder {
                    scope: Rc::new(SecurityScope::new(url)),
                    path,
                }) as ContentFolderRef
            }))
        })
    }

    fn save_document(
        &self,
        request: SaveDocumentRequest,
    ) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
        Box::pin(async move {
            let directories = cranpose_services::application_directories()
                .map_err(|error| ContentError::Io(error.to_string()))?;
            std::fs::create_dir_all(&directories.temporary).map_err(|error| {
                ContentError::Io(format!("{}: {error}", directories.temporary.display()))
            })?;
            let staging = directories
                .temporary
                .join(format!("cranpose-export-{}", request.file_name));
            let file = std::fs::File::create(&staging)
                .map_err(|error| ContentError::Io(format!("{}: {error}", staging.display())))?;
            Ok(Some(Rc::new(ExportSink {
                staging,
                writer: threaded_sink(Staged { file: Some(file) }),
                finished: Cell::new(false),
            }) as ContentSinkRef))
        })
    }

    /// The export chooser takes the file itself, so nothing is copied before
    /// it: a hard link under the requested name stands in for the file when
    /// the names differ.
    fn export_file(
        &self,
        path: PathBuf,
        request: SaveDocumentRequest,
    ) -> PickerFuture<Result<bool, FilePickerError>> {
        let link = exported_link(&path, &request.file_name);
        let exported = link.as_deref().unwrap_or(&path).to_path_buf();
        let presented = present(move |mtm| export_picker(mtm, &exported));
        Box::pin(async move {
            let picked = presented.await;
            if let Some(link) = link {
                let _ = std::fs::remove_file(link);
            }
            Ok(!picked?.is_empty())
        })
    }

    fn pick_writable_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<String>, FilePickerError>> {
        let picked = present_open(Kind::Folder, false);
        Box::pin(async move {
            match picked.await?.into_iter().next() {
                None => Ok(None),
                Some(url) => super::writable_folder::bookmark_handle(&url).map(Some),
            }
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Folder,
}

type PickResult = Result<Vec<Retained<NSURL>>, FilePickerError>;

type SharedSlot = super::pick_future::SharedPickSlot<PickResult>;

type PickFuture = super::pick_future::PickFuture<PickResult, PickerDelegate>;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeDocumentPickerDelegate"]
    #[ivars = SharedSlot]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            self.resolve(Ok(urls.iter().map(|url| url.retain()).collect()));
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn was_cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.resolve(Ok(Vec::new()));
        }
    }
);

impl PickerDelegate {
    fn new(slot: SharedSlot, mtm: MainThreadMarker) -> Retained<Self> {
        super::pick_future::new_pick_delegate(slot, mtm)
    }

    fn resolve(&self, result: PickResult) {
        super::pick_future::resolve_pick_slot(self.ivars(), result);
    }
}

fn present(
    build: impl FnOnce(MainThreadMarker) -> Retained<UIDocumentPickerViewController>,
) -> PickerFuture<PickResult> {
    let Some(mtm) = MainThreadMarker::new() else {
        return Box::pin(async {
            Err(FilePickerError::Failed(
                "a chooser must be presented on the main thread".into(),
            ))
        });
    };
    let Some(root) = root_view_controller(mtm) else {
        return Box::pin(async {
            Err(FilePickerError::Failed(
                "no root view controller to present from".into(),
            ))
        });
    };

    let picker = build(mtm);
    let slot: SharedSlot = Rc::new(RefCell::new(Default::default()));
    let delegate = PickerDelegate::new(Rc::clone(&slot), mtm);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    root.presentViewController_animated_completion(&picker, true, None);

    Box::pin(PickFuture {
        slot,
        _delegate: delegate,
    })
}

fn present_open(kind: Kind, multiple: bool) -> PickerFuture<PickResult> {
    present(move |mtm| {
        // SAFETY: `UTTypeItem`/`UTTypeDirectory` are immutable framework
        // constants.
        let ty: &UTType = match kind {
            Kind::File => unsafe { UTTypeItem },
            Kind::Folder => unsafe { UTTypeDirectory },
        };
        let picker = UIDocumentPickerViewController::initForOpeningContentTypes(
            UIDocumentPickerViewController::alloc(mtm),
            &NSArray::from_slice(&[ty]),
        );
        picker.setAllowsMultipleSelection(multiple);
        picker
    })
}

pub(crate) fn root_view_controller(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
    let app = UIApplication::sharedApplication(mtm);
    let scenes = app.connectedScenes();
    for scene in &scenes {
        let Ok(window_scene) = scene.downcast::<UIWindowScene>() else {
            continue;
        };
        let windows = window_scene.windows();
        for window in &windows {
            if let Some(controller) = window.rootViewController() {
                return Some(controller);
            }
        }
    }
    None
}

fn url_path(url: &NSURL) -> PathBuf {
    url.path()
        .map(|path| PathBuf::from(path.to_string()))
        .unwrap_or_default()
}

struct SecurityScope {
    url: Retained<NSURL>,
}

impl SecurityScope {
    fn new(url: Retained<NSURL>) -> Self {
        Self { url }
    }

    /// Opens the scope until the returned access is dropped.
    fn access(self: &Rc<Self>) -> ScopeAccess {
        let started = unsafe { self.url.startAccessingSecurityScopedResource() };
        ScopeAccess {
            scope: Rc::clone(self),
            started,
        }
    }

    fn enter<T>(&self, body: impl FnOnce() -> std::io::Result<T>) -> Result<T, ContentError> {
        let accessed = unsafe { self.url.startAccessingSecurityScopedResource() };
        let result = body().map_err(map_io);
        if accessed {
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
        result
    }
}

/// A security scope held open.
struct ScopeAccess {
    scope: Rc<SecurityScope>,
    started: bool,
}

impl Drop for ScopeAccess {
    fn drop(&mut self) {
        if self.started {
            unsafe { self.scope.url.stopAccessingSecurityScopedResource() };
        }
    }
}

fn map_io(error: std::io::Error) -> ContentError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ContentError::NotFound(error.to_string()),
        std::io::ErrorKind::PermissionDenied => ContentError::PermissionDenied(error.to_string()),
        _ => ContentError::Io(error.to_string()),
    }
}

fn scoped_file(url: Retained<NSURL>, path: PathBuf) -> ContentHandle {
    Rc::new(IosFile {
        scope: Rc::new(SecurityScope::new(url)),
        path,
    })
}

fn metadata_for(scope: &SecurityScope, path: &Path) -> ContentMetadata {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut metadata = ContentMetadata::named(name).with_identifier(path.display().to_string());
    let stat = scope.enter(|| std::fs::metadata(path));
    if let Ok(stat) = stat {
        if stat.is_file() {
            metadata.len = Some(stat.len());
        }
        metadata.modified_millis = stat
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_millis() as u64);
    }
    metadata
}

struct IosFile {
    scope: Rc<SecurityScope>,
    path: PathBuf,
}

impl Content for IosFile {
    fn metadata(&self) -> ContentMetadata {
        metadata_for(&self.scope, &self.path)
    }

    /// The file opens inside its security scope, which stays open while the
    /// reader lives; the reads run on a thread of their own.
    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        let scope = Rc::clone(&self.scope);
        let path = self.path.clone();
        Box::pin(async move {
            let access = scope.access();
            let file = std::fs::File::open(&path).map_err(map_io)?;
            let reader = open_threaded(move || Ok(file), map_io).await?;
            Ok(Rc::new(IosReader {
                _access: access,
                reader,
            }) as ContentReaderRef)
        })
    }
}

struct IosReader {
    _access: ScopeAccess,
    reader: ContentReaderRef,
}

impl ContentReader for IosReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        self.reader.read_chunk()
    }
}

struct IosFolder {
    scope: Rc<SecurityScope>,
    path: PathBuf,
}

impl ContentFolder for IosFolder {
    fn metadata(&self) -> ContentMetadata {
        metadata_for(&self.scope, &self.path)
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        let scope = Rc::clone(&self.scope);
        let path = self.path.clone();
        Box::pin(async move {
            let children = scope.enter(|| {
                let mut children = Vec::new();
                for child in std::fs::read_dir(&path)? {
                    let child = child?;
                    children.push((child.path(), child.path().is_dir()));
                }
                Ok(children)
            })?;
            Ok(children
                .into_iter()
                .map(|(path, is_dir)| {
                    if is_dir {
                        ContentEntry::Folder(Rc::new(Self {
                            scope: Rc::clone(&scope),
                            path,
                        }))
                    } else {
                        ContentEntry::File(Rc::new(IosFile {
                            scope: Rc::clone(&scope),
                            path,
                        }))
                    }
                })
                .collect())
        })
    }
}

/// A document written to a staging file on a thread of its own, then handed
/// to the export chooser.
struct ExportSink {
    staging: PathBuf,
    writer: ContentSinkRef,
    finished: Cell<bool>,
}

impl ContentSink for ExportSink {
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>> {
        self.writer.write_chunk(bytes)
    }

    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            if self.finished.replace(true) {
                return Ok(());
            }
            self.writer.finish().await?;
            let staging = self.staging.clone();
            let exported = present(move |mtm| export_picker(mtm, &staging)).await;
            let _ = std::fs::remove_file(&self.staging);
            exported
                .map(|_| ())
                .map_err(|error| ContentError::Io(error.to_string()))
        })
    }
}

impl Drop for ExportSink {
    fn drop(&mut self) {
        if !self.finished.get() {
            let _ = std::fs::remove_file(&self.staging);
        }
    }
}

/// The staging file of an [`ExportSink`].
struct Staged {
    file: Option<std::fs::File>,
}

impl BlockingSink for Staged {
    fn write(&mut self, bytes: &[u8]) -> Result<(), ContentError> {
        self.file
            .as_mut()
            .ok_or_else(|| ContentError::Io("sink is already finished".into()))?
            .write_all(bytes)
            .map_err(map_io)
    }

    fn finish(&mut self) -> Result<(), ContentError> {
        let Some(mut file) = self.file.take() else {
            return Ok(());
        };
        file.flush().map_err(map_io)?;
        file.sync_all().map_err(map_io)
    }
}

/// The export chooser for the file at `path`, which it copies.
fn export_picker(mtm: MainThreadMarker, path: &Path) -> Retained<UIDocumentPickerViewController> {
    let url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(
        &path.to_string_lossy(),
    ));
    UIDocumentPickerViewController::initForExportingURLs_asCopy(
        UIDocumentPickerViewController::alloc(mtm),
        &NSArray::from_slice(&[&*url]),
        true,
    )
}

/// A hard link to `path` named `name` in the temporary folder, when `name`
/// differs from the file's own; `None` exports the file under its own name.
fn exported_link(path: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() || path.file_name().is_some_and(|own| own == name) {
        return None;
    }
    let folder = cranpose_services::application_directories().ok()?.temporary;
    std::fs::create_dir_all(&folder).ok()?;
    let link = folder.join(name);
    let _ = std::fs::remove_file(&link);
    std::fs::hard_link(path, &link).ok().map(|()| link)
}
