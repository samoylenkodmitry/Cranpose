//! iOS persistent writable folder built on `UIDocumentPickerViewController` +
//! security-scoped bookmarks.
//!
//! The user picks a folder through the Files app; the granted security-scoped
//! URL is serialized to a bookmark (hex-encoded) as the durable handle. Reopening
//! resolves that bookmark on any thread and holds its security scope for the
//! duration of each file operation — so scheduled auto-backups keep working
//! across launches without re-prompting.
//!
//! Registered as the platform writable-folder picker + store factory (see
//! [`cranpose_services::set_platform_writable_folder_picker`]) by the iOS backend.
#![allow(unsafe_code)]

use cranpose_services::{
    set_platform_writable_folder_picker, set_writable_folder_store_factory, FilePickerError,
    FolderError, PickerFuture, WritableFolderPicker, WritableFolderStore, WritableFolderStoreRef,
};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{
    NSArray, NSData, NSObject, NSObjectProtocol, NSURLBookmarkCreationOptions,
    NSURLBookmarkResolutionOptions, NSURL,
};
use objc2_ui_kit::{UIDocumentPickerDelegate, UIDocumentPickerViewController};
use objc2_uniform_type_identifiers::{UTType, UTTypeFolder};
use std::cell::RefCell;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// Installs the iOS writable-folder picker and store factory.
pub(crate) fn register() {
    set_platform_writable_folder_picker(Rc::new(IosWritableFolderPicker));
    set_writable_folder_store_factory(Box::new(|handle| {
        Some(Arc::new(BookmarkStore {
            handle: handle.to_owned(),
        }) as WritableFolderStoreRef)
    }));
}

struct IosWritableFolderPicker;

impl WritableFolderPicker for IosWritableFolderPicker {
    fn pick(&self) -> PickerFuture<Result<Option<String>, FilePickerError>> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Box::pin(async {
                Err(FilePickerError::Failed(
                    "folder picker must be presented on the main thread".into(),
                ))
            });
        };
        match present(mtm) {
            Ok(future) => Box::pin(future),
            Err(error) => Box::pin(async move { Err(error) }),
        }
    }
}

type PickResult = Result<Option<String>, FilePickerError>;

#[derive(Default)]
struct PickSlot {
    result: Option<PickResult>,
    waker: Option<Waker>,
}

type SharedSlot = Rc<RefCell<PickSlot>>;

struct PickFuture {
    slot: SharedSlot,
    _delegate: Retained<FolderPickerDelegate>,
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

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeWritableFolderPickerDelegate"]
    #[ivars = SharedSlot]
    struct FolderPickerDelegate;

    unsafe impl NSObjectProtocol for FolderPickerDelegate {}

    unsafe impl UIDocumentPickerDelegate for FolderPickerDelegate {
        #[unsafe(method(documentPicker:didPickDocumentsAtURLs:))]
        fn did_pick(&self, _picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let result = match urls.firstObject() {
                Some(url) => make_handle(&url).map(Some),
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

impl FolderPickerDelegate {
    fn new(slot: SharedSlot, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(slot);
        unsafe { msg_send![super(this), init] }
    }

    fn resolve(&self, result: PickResult) {
        let mut slot = self.ivars().borrow_mut();
        slot.result = Some(result);
        if let Some(waker) = slot.waker.take() {
            waker.wake();
        }
    }
}

fn present(mtm: MainThreadMarker) -> Result<PickFuture, FilePickerError> {
    let root = crate::ios_file_picker::root_view_controller(mtm)
        .ok_or_else(|| FilePickerError::Failed("no root view controller to present from".into()))?;

    // SAFETY: `UTTypeFolder` is an immutable framework constant.
    let folder: &UTType = unsafe { UTTypeFolder };
    let content_types = NSArray::from_slice(&[folder]);
    let picker = UIDocumentPickerViewController::initForOpeningContentTypes(
        UIDocumentPickerViewController::alloc(mtm),
        &content_types,
    );

    let slot: SharedSlot = Rc::new(RefCell::new(PickSlot::default()));
    let delegate = FolderPickerDelegate::new(slot.clone(), mtm);
    picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    root.presentViewController_animated_completion(&picker, true, None);

    Ok(PickFuture {
        slot,
        _delegate: delegate,
    })
}

/// Serialize a picked security-scoped folder URL to a durable, hex-encoded
/// bookmark handle.
fn make_handle(url: &NSURL) -> Result<String, FilePickerError> {
    let accessed = unsafe { url.startAccessingSecurityScopedResource() };
    let data = url.bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
        NSURLBookmarkCreationOptions::empty(),
        None,
        None,
    );
    if accessed {
        unsafe { url.stopAccessingSecurityScopedResource() };
    }
    let data =
        data.map_err(|_| FilePickerError::Failed("could not bookmark the chosen folder".into()))?;
    Ok(hex_encode(&data.to_vec()))
}

/// A writable folder reopened from a security-scoped bookmark. Thread-safe: it
/// holds only the encoded bookmark and re-resolves it (holding the scope) per
/// operation, so background auto-backups can use it off the main thread.
struct BookmarkStore {
    handle: String,
}

impl BookmarkStore {
    fn with_scope<T>(
        &self,
        body: impl FnOnce(&Path) -> std::io::Result<T>,
    ) -> Result<T, FolderError> {
        let bytes = hex_decode(&self.handle)
            .ok_or_else(|| FolderError::Io("malformed folder bookmark".into()))?;
        let data = NSData::with_bytes(&bytes);
        let mut is_stale = Bool::NO;
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::empty(),
                None,
                &mut is_stale,
            )
        }
        .map_err(|_| FolderError::Io("could not resolve folder bookmark".into()))?;

        let accessed = unsafe { url.startAccessingSecurityScopedResource() };
        let result = match url.path() {
            Some(path) => body(Path::new(&path.to_string()))
                .map_err(|error| FolderError::Io(error.to_string())),
            None => Err(FolderError::Io("bookmark resolved to no path".into())),
        };
        if accessed {
            unsafe { url.stopAccessingSecurityScopedResource() };
        }
        result
    }
}

impl WritableFolderStore for BookmarkStore {
    fn write(&self, name: &str, contents: &[u8]) -> Result<(), FolderError> {
        self.with_scope(|dir| std::fs::write(dir.join(name), contents))
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, FolderError> {
        self.with_scope(|dir| std::fs::read(dir.join(name)))
    }

    fn list(&self) -> Result<Vec<String>, FolderError> {
        self.with_scope(|dir| {
            let mut names = Vec::new();
            for child in std::fs::read_dir(dir)? {
                let child = child?;
                if child.path().is_file() {
                    names.push(child.file_name().to_string_lossy().into_owned());
                }
            }
            Ok(names)
        })
    }

    fn remove(&self, name: &str) -> Result<(), FolderError> {
        self.with_scope(|dir| match std::fs::remove_file(dir.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        })
    }

    fn is_writable(&self) -> bool {
        self.with_scope(|dir| Ok(dir.is_dir())).unwrap_or(false)
    }

    fn handle(&self) -> String {
        self.handle.clone()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}
