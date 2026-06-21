//! iOS file and folder picker built on `UIDocumentPickerViewController`.
//!
//! The system document picker surfaces every provider the device exposes
//! through the Files app — local storage, iCloud Drive, and third-party
//! providers such as a mounted WebDAV share — so a picked folder is a
//! security-scoped URL into the chosen provider rather than a private path.
//!
//! The picker is registered as the platform file picker (see
//! [`cranpose_services::set_platform_file_picker`]) by the iOS backend, which
//! runs on the UIKit main thread.
#![allow(unsafe_code)]

use cranpose_services::{
    set_platform_file_picker, FilePicker, FilePickerError, FilePickerOptions, PickedEntry,
    PickedEntryRef, PickedKind, PickerFuture,
};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly, Message};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use objc2_ui_kit::{
    UIApplication, UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController,
    UIWindowScene,
};
use objc2_uniform_type_identifiers::{UTType, UTTypeFolder, UTTypeItem};
use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

/// Installs the iOS picker as the platform file picker.
pub(crate) fn register() {
    set_platform_file_picker(Rc::new(IosFilePicker));
}

struct IosFilePicker;

impl FilePicker for IosFilePicker {
    fn pick_file(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
        present(options, PickedKind::File)
    }

    fn pick_folder(
        &self,
        options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
        present(options, PickedKind::Folder)
    }
}

type PickResult = Result<Option<PickedEntryRef>, FilePickerError>;

/// One-shot slot shared between the picker delegate and the awaiting future.
#[derive(Default)]
struct PickSlot {
    result: Option<PickResult>,
    waker: Option<Waker>,
}

type SharedSlot = Rc<RefCell<PickSlot>>;

/// Future resolved when the delegate receives a selection or cancellation.
/// Holds the delegate alive (the picker keeps only a weak reference to it).
struct PickFuture {
    slot: SharedSlot,
    _delegate: Retained<PickerDelegate>,
}

impl Future for PickFuture {
    type Output = PickResult;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<PickResult> {
        let mut slot = self.slot.borrow_mut();
        if let Some(result) = slot.result.take() {
            Poll::Ready(result)
        } else {
            slot.waker = Some(context.waker().clone());
            Poll::Pending
        }
    }
}

struct DelegateIvars {
    slot: SharedSlot,
    kind: PickedKind,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeDocumentPickerDelegate"]
    #[ivars = DelegateIvars]
    struct PickerDelegate;

    unsafe impl NSObjectProtocol for PickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for PickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let kind = self.ivars().kind;
            let result = match urls.firstObject() {
                Some(url) => entry_from_url(&url, kind).map(Some),
                None => Ok(None),
            };
            self.resolve(result);
        }

        #[unsafe(method(documentPickerWasCancelled:))]
        fn was_cancelled(&self, _picker: &UIDocumentPickerViewController) {
            self.resolve(Ok(None));
        }
    }
);

impl PickerDelegate {
    fn new(slot: SharedSlot, kind: PickedKind, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { slot, kind });
        unsafe { msg_send![super(this), init] }
    }

    fn resolve(&self, result: PickResult) {
        let mut slot = self.ivars().slot.borrow_mut();
        slot.result = Some(result);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

fn present(_options: FilePickerOptions, kind: PickedKind) -> PickerFuture<PickResult> {
    // UIKit work must happen on the main thread; composition runs there.
    let Some(mtm) = MainThreadMarker::new() else {
        return Box::pin(async {
            Err(FilePickerError::Failed(
                "file picker must be presented on the main thread".into(),
            ))
        });
    };

    match present_inner(kind, mtm) {
        Ok(future) => Box::pin(future),
        Err(error) => Box::pin(async move { Err(error) }),
    }
}

fn present_inner(kind: PickedKind, mtm: MainThreadMarker) -> Result<PickFuture, FilePickerError> {
    let root = root_view_controller(mtm)
        .ok_or_else(|| FilePickerError::Failed("no root view controller to present from".into()))?;

    let content_types = content_types(kind);
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes(
        UIDocumentPickerViewController::alloc(mtm),
        &content_types,
    );

    let slot: SharedSlot = Rc::new(RefCell::new(PickSlot::default()));
    let delegate = PickerDelegate::new(slot.clone(), kind, mtm);
    let delegate_proto = ProtocolObject::from_ref(&*delegate);
    picker.setDelegate(Some(delegate_proto));
    root.presentViewController_animated_completion(&picker, true, None);

    Ok(PickFuture {
        slot,
        _delegate: delegate,
    })
}

fn content_types(kind: PickedKind) -> Retained<NSArray<UTType>> {
    // SAFETY: `UTTypeItem`/`UTTypeFolder` are immutable framework constants.
    let ty: &UTType = match kind {
        PickedKind::File => unsafe { UTTypeItem },
        PickedKind::Folder => unsafe { UTTypeFolder },
    };
    NSArray::from_slice(&[ty])
}

fn root_view_controller(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
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

fn entry_from_url(url: &NSURL, kind: PickedKind) -> Result<PickedEntryRef, FilePickerError> {
    let path = url_path(url)
        .ok_or_else(|| FilePickerError::ReadFailed("picked URL has no file path".into()))?;
    Ok(Rc::new(IosEntry {
        scope_url: url.retain(),
        path,
        kind,
    }))
}

fn url_path(url: &NSURL) -> Option<PathBuf> {
    let path = url.path()?;
    Some(PathBuf::from(path.to_string()))
}

/// A picked entry backed by a security-scoped iOS document URL.
///
/// `scope_url` is the originally picked URL whose security scope must be held
/// while reading; children of a picked folder share the same scope URL.
struct IosEntry {
    scope_url: Retained<NSURL>,
    path: PathBuf,
    kind: PickedKind,
}

impl IosEntry {
    /// Runs `body` while the security scope of `scope_url` is held.
    fn with_scope<T>(
        &self,
        body: impl FnOnce(&std::path::Path) -> std::io::Result<T>,
    ) -> Result<T, FilePickerError> {
        let accessed = unsafe { self.scope_url.startAccessingSecurityScopedResource() };
        let result =
            body(&self.path).map_err(|error| FilePickerError::ReadFailed(error.to_string()));
        if accessed {
            unsafe { self.scope_url.stopAccessingSecurityScopedResource() };
        }
        result
    }
}

impl PickedEntry for IosEntry {
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
        let scope_url = self.scope_url.clone();
        let path = self.path.clone();
        let entry = IosEntry {
            scope_url,
            path,
            kind: self.kind,
        };
        Box::pin(async move { entry.with_scope(|path| std::fs::read(path)) })
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
        let scope_url = self.scope_url.clone();
        let path = self.path.clone();
        let entry = IosEntry {
            scope_url: scope_url.clone(),
            path,
            kind: self.kind,
        };
        Box::pin(async move {
            entry.with_scope(|dir| {
                let mut entries: Vec<PickedEntryRef> = Vec::new();
                for child in std::fs::read_dir(dir)? {
                    let child = child?;
                    let child_path = child.path();
                    let kind = if child_path.is_dir() {
                        PickedKind::Folder
                    } else {
                        PickedKind::File
                    };
                    entries.push(Rc::new(IosEntry {
                        scope_url: scope_url.clone(),
                        path: child_path,
                        kind,
                    }));
                }
                Ok(entries)
            })
        })
    }
}
