#![allow(unsafe_code)]

use std::{
    cell::RefCell,
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
};

use cranpose_services::{
    Content, ContentEntry, ContentError, ContentFolder, ContentFolderRef, ContentFuture,
    ContentHandle, ContentMetadata, ContentReader, ContentReaderRef, ContentSink, ContentSinkRef,
    DEFAULT_CHUNK_LEN, FilePicker, FilePickerError, FilePickerOptions, PickerFuture,
    SaveDocumentRequest, set_platform_file_picker,
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
                file: RefCell::new(Some(file)),
            }) as ContentSinkRef))
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
                Some(url) => crate::ios_writable_folder::bookmark_handle(&url).map(Some),
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

type SharedSlot = crate::ios_pick_future::SharedPickSlot<PickResult>;

type PickFuture = crate::ios_pick_future::PickFuture<PickResult, PickerDelegate>;

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
        crate::ios_pick_future::new_pick_delegate(slot, mtm)
    }

    fn resolve(&self, result: PickResult) {
        crate::ios_pick_future::resolve_pick_slot(self.ivars(), result);
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
    let delegate = PickerDelegate::new(slot.clone(), mtm);
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
    for scene in scenes.iter() {
        let Ok(window_scene) = scene.downcast::<UIWindowScene>() else {
            continue;
        };
        let windows = window_scene.windows();
        for window in windows.iter() {
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

    fn enter<T>(&self, body: impl FnOnce() -> std::io::Result<T>) -> Result<T, ContentError> {
        let accessed = unsafe { self.url.startAccessingSecurityScopedResource() };
        let result = body().map_err(map_io);
        if accessed {
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
        result
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
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
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

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        let scope = Rc::clone(&self.scope);
        let path = self.path.clone();
        Box::pin(async move {
            let file = scope.enter(|| std::fs::File::open(&path))?;
            Ok(Rc::new(IosReader {
                scope,
                file: RefCell::new(Some(file)),
            }) as ContentReaderRef)
        })
    }

    fn read_all(&self) -> ContentFuture<'_, Result<Vec<u8>, ContentError>> {
        let scope = Rc::clone(&self.scope);
        let path = self.path.clone();
        Box::pin(async move { scope.enter(|| std::fs::read(&path)) })
    }
}

struct IosReader {
    scope: Rc<SecurityScope>,
    file: RefCell<Option<std::fs::File>>,
}

impl ContentReader for IosReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(async move {
            let mut slot = self.file.borrow_mut();
            let Some(file) = slot.as_mut() else {
                return Ok(None);
            };
            let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
            let filled = self.scope.enter(|| {
                let mut filled = 0;
                while filled < buffer.len() {
                    match file.read(&mut buffer[filled..]) {
                        Ok(0) => break,
                        Ok(read) => filled += read,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => return Err(error),
                    }
                }
                Ok(filled)
            })?;
            if filled == 0 {
                *slot = None;
                return Ok(None);
            }
            buffer.truncate(filled);
            Ok(Some(buffer))
        })
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
                        ContentEntry::Folder(Rc::new(IosFolder {
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

struct ExportSink {
    staging: PathBuf,
    file: RefCell<Option<std::fs::File>>,
}

impl ContentSink for ExportSink {
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            let mut slot = self.file.borrow_mut();
            let file = slot
                .as_mut()
                .ok_or_else(|| ContentError::Io("sink is already finished".into()))?;
            file.write_all(&bytes).map_err(map_io)
        })
    }

    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            let Some(mut file) = self.file.borrow_mut().take() else {
                return Ok(());
            };
            file.flush().map_err(map_io)?;
            file.sync_all().map_err(map_io)?;
            drop(file);

            let staging = self.staging.clone();
            let exported = present(move |mtm| {
                let url = NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(
                    &staging.to_string_lossy(),
                ));
                UIDocumentPickerViewController::initForExportingURLs_asCopy(
                    UIDocumentPickerViewController::alloc(mtm),
                    &NSArray::from_slice(&[&*url]),
                    true,
                )
            })
            .await;
            let _ = std::fs::remove_file(&self.staging);
            exported
                .map(|_| ())
                .map_err(|error| ContentError::Io(error.to_string()))
        })
    }
}

impl Drop for ExportSink {
    fn drop(&mut self) {
        if self.file.borrow().is_some() {
            let _ = std::fs::remove_file(&self.staging);
        }
    }
}
