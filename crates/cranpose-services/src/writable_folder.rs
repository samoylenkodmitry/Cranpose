//! Cross-platform access to a user-chosen **writable** folder.
//!
//! This is the write-side complement of [`crate::file_picker`]: where the picker
//! *reads* files the user selects, this lets an app persist its own data into a
//! folder the user grants — a local directory on desktop, a Storage Access
//! Framework tree on Android — and read it back on later runs. The motivating
//! use is cross-device sync, where each device writes a small document into a
//! shared folder (e.g. on a Tailnet/WebDAV mount) and reads its peers'.
//!
//! Two halves, mirroring the picker:
//! - [`pick_writable_folder`] — asynchronous, UI-thread. Presents the system
//!   folder picker with a *persistent read/write* grant and resolves to an
//!   opaque, durable **handle** string the app stores.
//! - [`open_writable_folder`] — synchronous and thread-safe. Rebuilds a
//!   [`WritableFolderStore`] from a stored handle. Its I/O methods are
//!   synchronous and `Send + Sync`, so a background worker can call them without
//!   touching the UI thread.
//!
//! Read-only or unreachable folders surface as [`FolderError::ReadOnly`] /
//! [`FolderError::Io`] so callers can degrade gracefully. iOS (security-scoped
//! bookmarks) and the web are not supported yet and return
//! [`FolderError::Unsupported`].

use crate::file_picker::{FilePickerError, PickerFuture};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

/// Errors produced by writable-folder I/O.
#[derive(thiserror::Error, Debug, Clone)]
pub enum FolderError {
    /// The folder (or backing store) is read-only.
    #[error("writable folder is read-only")]
    ReadOnly,
    /// The named file does not exist.
    #[error("file not found: {0}")]
    NotFound(String),
    /// Writable folders are not available on this platform/build.
    #[error("writable folders are not supported on this platform")]
    Unsupported,
    /// Any other I/O failure.
    #[error("{0}")]
    Io(String),
}

/// Synchronous, thread-safe access to a user-chosen writable folder.
///
/// Implementations are `Send + Sync` so a background thread can read and write
/// without involving the UI thread. Treat it as a flat store of named files
/// (directories are not exposed by [`list`](WritableFolderStore::list)).
pub trait WritableFolderStore: Send + Sync {
    /// Writes (overwriting) the file `name` with `contents`.
    fn write(&self, name: &str, contents: &[u8]) -> Result<(), FolderError>;

    /// Reads the file `name`.
    fn read(&self, name: &str) -> Result<Vec<u8>, FolderError>;

    /// Lists the immediate child *file* names (directories excluded).
    fn list(&self) -> Result<Vec<String>, FolderError>;

    /// Removes the file `name`. Succeeds if it is already absent.
    fn remove(&self, name: &str) -> Result<(), FolderError>;

    /// Cheaply probes whether the folder is writable right now (e.g. detects a
    /// read-only WebDAV mount that still granted a write permission).
    fn is_writable(&self) -> bool;

    /// The durable handle (filesystem path / SAF tree URI) used to reopen this
    /// folder with [`open_writable_folder`] on a later run.
    fn handle(&self) -> String;
}

/// Shared handle to a [`WritableFolderStore`].
pub type WritableFolderStoreRef = Arc<dyn WritableFolderStore>;

/// Presents the system "pick a writable folder" UI. Platform-provided (Android);
/// desktop/web use the built-in backend.
pub trait WritableFolderPicker {
    /// Resolves to the durable handle string, or `None` if the user cancelled.
    fn pick(&self) -> PickerFuture<Result<Option<String>, FilePickerError>>;
}

/// Shared handle to a [`WritableFolderPicker`].
pub type WritableFolderPickerRef = Rc<dyn WritableFolderPicker>;

thread_local! {
    static PLATFORM_PICKER: RefCell<Option<WritableFolderPickerRef>> = const { RefCell::new(None) };
}

/// Registers the platform writable-folder picker (Android SAF). The cranpose
/// crate's backend calls this during startup; once registered it takes
/// precedence over the built-in desktop picker.
pub fn set_platform_writable_folder_picker(picker: WritableFolderPickerRef) {
    PLATFORM_PICKER.with(|cell| *cell.borrow_mut() = Some(picker));
}

/// Removes any registered platform picker (tests/teardown).
pub fn clear_platform_writable_folder_picker() {
    PLATFORM_PICKER.with(|cell| *cell.borrow_mut() = None);
}

fn registered_picker() -> Option<WritableFolderPickerRef> {
    PLATFORM_PICKER.with(|cell| cell.borrow().clone())
}

/// Factory turning a stored handle into a [`WritableFolderStore`]. Unlike the
/// picker this is thread-safe, so a worker thread can reopen the folder.
type StoreFactory = Box<dyn Fn(&str) -> Option<WritableFolderStoreRef> + Send + Sync>;
static STORE_FACTORY: OnceLock<StoreFactory> = OnceLock::new();

/// Registers the platform store factory (Android). Called once at startup. No-op
/// if already set.
pub fn set_writable_folder_store_factory(factory: StoreFactory) {
    let _ = STORE_FACTORY.set(factory);
}

/// Presents the writable-folder picker and resolves to a durable handle (or
/// `None` if cancelled). Async; drive it from `LaunchedEffectAsync`.
pub fn pick_writable_folder() -> PickerFuture<Result<Option<String>, FilePickerError>> {
    if let Some(picker) = registered_picker() {
        return picker.pick();
    }
    builtin_pick()
}

/// Reopens a writable folder from a stored handle. Synchronous and callable from
/// any thread; returns `None` only when writable folders are unsupported here.
pub fn open_writable_folder(handle: &str) -> Option<WritableFolderStoreRef> {
    if let Some(factory) = STORE_FACTORY.get() {
        return factory(handle);
    }
    builtin_open(handle)
}

fn builtin_pick() -> PickerFuture<Result<Option<String>, FilePickerError>> {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios"),
        feature = "file-picker-native"
    ))]
    {
        return desktop::pick();
    }
    #[allow(unreachable_code)]
    Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
}

fn builtin_open(handle: &str) -> Option<WritableFolderStoreRef> {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios"),
        feature = "file-picker-native"
    ))]
    {
        return Some(desktop::open(handle));
    }
    #[allow(unreachable_code)]
    {
        let _ = handle;
        None
    }
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "file-picker-native"
))]
mod desktop;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_error_messages_are_distinct() {
        assert_eq!(
            FolderError::ReadOnly.to_string(),
            "writable folder is read-only"
        );
        assert!(FolderError::NotFound("a.txt".into())
            .to_string()
            .contains("a.txt"));
    }

    #[test]
    fn picker_registration_round_trips() {
        struct Marker;
        impl WritableFolderPicker for Marker {
            fn pick(&self) -> PickerFuture<Result<Option<String>, FilePickerError>> {
                Box::pin(async { Ok(Some("handle".to_string())) })
            }
        }
        clear_platform_writable_folder_picker();
        assert!(registered_picker().is_none());
        set_platform_writable_folder_picker(Rc::new(Marker));
        assert!(registered_picker().is_some());
        let result = pollster::block_on(pick_writable_folder());
        assert!(matches!(result, Ok(Some(handle)) if handle == "handle"));
        clear_platform_writable_folder_picker();
    }
}
