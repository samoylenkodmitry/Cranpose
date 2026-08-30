//! Cross-platform access to a user-chosen **writable** folder.
//!
//! This is the write-side complement of [`crate::file_picker`]: where the
//! choosers *read* content the user selects, this lets an app persist its own
//! data into a folder the user grants — a local directory on desktop, a Storage
//! Access Framework tree on Android — and read it back on later runs. The
//! motivating use is cross-device sync, where each device writes a small
//! document into a shared folder (e.g. on a Tailnet/WebDAV mount) and reads its
//! peers'.
//!
//! Two halves:
//! - [`crate::launcher::rememberWritableFolderLauncher`] presents the system
//!   chooser and hands back the opaque, durable **handle** string the app
//!   stores. The launcher owns the request across host recreation.
//! - [`open_writable_folder`] is synchronous and thread-safe. It rebuilds a
//!   [`WritableFolderStore`] from a stored handle, so a background worker can
//!   read and write without touching the UI thread.
//!
//! Stores expose whole-file operations, [`FolderEntry`] display metadata, and
//! chunked [`FolderReader`]/[`FolderWriter`] streams for payloads that should
//! not be buffered. Read-only or unreachable folders surface as
//! [`FolderError::ReadOnly`] / [`FolderError::Io`] so callers can degrade
//! gracefully. Backends: desktop (`std::fs`), Android (SAF tree URIs), iOS
//! (security-scoped bookmarks). The web has no writable-folder concept and
//! returns [`FolderError::Unsupported`].

use std::sync::{Arc, OnceLock};

/// Errors produced by writable-folder I/O.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
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

/// Display metadata for one file in a writable folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderEntry {
    /// The file name, as stored.
    pub name: String,
    /// Byte length.
    pub len: u64,
    /// Last-modified time in milliseconds since the Unix epoch, when the
    /// provider reports one.
    pub modified_millis: Option<u64>,
}

/// Chunked reader over one file in a writable folder.
///
/// Synchronous and `Send` so a worker thread can drain it.
pub trait FolderReader: Send {
    /// Reads the next chunk, or `None` at end of file.
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, FolderError>;
}

/// Chunked writer over one file in a writable folder.
///
/// The file is only visible under its final name once [`finish`] succeeds;
/// dropping a writer without finishing discards the partial write.
///
/// [`finish`]: FolderWriter::finish
pub trait FolderWriter: Send {
    /// Appends `bytes`.
    fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), FolderError>;

    /// Commits the file under its final name.
    fn finish(self: Box<Self>) -> Result<(), FolderError>;
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

    /// Lists the immediate child *files* with their display metadata
    /// (directories excluded).
    fn list(&self) -> Result<Vec<FolderEntry>, FolderError>;

    /// Removes the file `name`. Succeeds if it is already absent.
    fn remove(&self, name: &str) -> Result<(), FolderError>;

    /// Opens a chunked reader over the file `name`.
    fn open_read(&self, name: &str) -> Result<Box<dyn FolderReader>, FolderError>;

    /// Opens a chunked writer that replaces the file `name` on
    /// [`FolderWriter::finish`].
    fn open_write(&self, name: &str) -> Result<Box<dyn FolderWriter>, FolderError>;

    /// Cheaply probes whether the folder is writable right now (e.g. detects a
    /// read-only WebDAV mount that still granted a write permission).
    fn is_writable(&self) -> bool;

    /// The durable handle (filesystem path / SAF tree URI) used to reopen this
    /// folder with [`open_writable_folder`] on a later run.
    fn handle(&self) -> String;

    /// The folder's name as the user would recognise it. The default derives it
    /// from the last component of [`handle`](WritableFolderStore::handle);
    /// providers that know a nicer name (an SAF display name) override it.
    fn display_name(&self) -> String {
        let handle = self.handle();
        handle
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\', ':'])
            .find(|segment| !segment.is_empty())
            .map(|segment| segment.to_string())
            .unwrap_or(handle)
    }

    /// Display metadata for one file, without reading it.
    ///
    /// The default filters [`list`](WritableFolderStore::list); providers that
    /// can stat a single file override it.
    fn entry(&self, name: &str) -> Result<FolderEntry, FolderError> {
        self.list()?
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| FolderError::NotFound(name.to_string()))
    }
}

/// Shared handle to a [`WritableFolderStore`].
pub type WritableFolderStoreRef = Arc<dyn WritableFolderStore>;

type StoreFactory = Box<dyn Fn(&str) -> Option<WritableFolderStoreRef> + Send + Sync>;
static STORE_FACTORY: OnceLock<StoreFactory> = OnceLock::new();

/// Registers the platform store factory (Android, iOS). Called once at startup.
/// No-op if already set.
pub fn set_writable_folder_store_factory(factory: StoreFactory) {
    let _ = STORE_FACTORY.set(factory);
}

/// Reopens a writable folder from a stored handle. Synchronous and callable from
/// any thread; returns `None` only when writable folders are unsupported here.
pub fn open_writable_folder(handle: &str) -> Option<WritableFolderStoreRef> {
    if let Some(factory) = STORE_FACTORY.get() {
        return factory(handle);
    }
    builtin_open(handle)
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

    struct FlatStore {
        handle: String,
    }

    impl WritableFolderStore for FlatStore {
        fn write(&self, _name: &str, _contents: &[u8]) -> Result<(), FolderError> {
            Ok(())
        }
        fn read(&self, _name: &str) -> Result<Vec<u8>, FolderError> {
            Ok(Vec::new())
        }
        fn list(&self) -> Result<Vec<FolderEntry>, FolderError> {
            Ok(vec![FolderEntry {
                name: "sync.json".into(),
                len: 12,
                modified_millis: Some(5),
            }])
        }
        fn remove(&self, _name: &str) -> Result<(), FolderError> {
            Ok(())
        }
        fn open_read(&self, _name: &str) -> Result<Box<dyn FolderReader>, FolderError> {
            Err(FolderError::Unsupported)
        }
        fn open_write(&self, _name: &str) -> Result<Box<dyn FolderWriter>, FolderError> {
            Err(FolderError::Unsupported)
        }
        fn is_writable(&self) -> bool {
            true
        }
        fn handle(&self) -> String {
            self.handle.clone()
        }
    }

    #[test]
    fn folder_error_messages_are_distinct() {
        assert_eq!(
            FolderError::ReadOnly.to_string(),
            "writable folder is read-only"
        );
        assert!(
            FolderError::NotFound("a.txt".into())
                .to_string()
                .contains("a.txt")
        );
    }

    #[test]
    fn the_default_display_name_is_the_last_handle_segment() {
        let store = FlatStore {
            handle: "/home/user/Shared Sync/".into(),
        };
        assert_eq!(store.display_name(), "Shared Sync");
        let tree = FlatStore {
            handle: "content://com.android.externalstorage.documents/tree/primary%3ASync".into(),
        };
        assert_eq!(tree.display_name(), "primary%3ASync");
    }

    #[test]
    fn the_default_entry_lookup_filters_the_listing() {
        let store = FlatStore {
            handle: "sync-root".into(),
        };
        assert_eq!(store.entry("sync.json").unwrap().len, 12);
        assert_eq!(
            store.entry("absent.json"),
            Err(FolderError::NotFound("absent.json".into()))
        );
    }
}
