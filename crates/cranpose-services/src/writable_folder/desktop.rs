//! Desktop writable-folder backend: `rfd` folder picker + `std::fs` I/O.
//!
//! The chosen directory may be a plain folder or a mounted network share
//! (GVFS/rclone/davfs over the same WebDAV the app already reads). Writes use
//! temp-file-then-rename for atomicity and fall back to a direct write when the
//! backing filesystem rejects rename. A permission/EROFS failure maps to
//! [`FolderError::ReadOnly`] so the caller can degrade to receive-only.

use super::{FolderError, WritableFolderStore, WritableFolderStoreRef};
use crate::file_picker::{FilePickerError, PickerFuture};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Fixed-name probe used by [`is_writable`](WritableFolderStore::is_writable);
/// created and immediately deleted, so it never accumulates.
const PROBE_NAME: &str = ".cranpose-write-probe";

pub(super) fn pick() -> PickerFuture<Result<Option<String>, FilePickerError>> {
    Box::pin(async move {
        let handle = rfd::AsyncFileDialog::new().pick_folder().await;
        Ok(handle.map(|handle| handle.path().to_string_lossy().into_owned()))
    })
}

pub(super) fn open(handle: &str) -> WritableFolderStoreRef {
    Arc::new(DesktopWritableFolder {
        dir: PathBuf::from(handle),
    })
}

struct DesktopWritableFolder {
    dir: PathBuf,
}

impl WritableFolderStore for DesktopWritableFolder {
    fn write(&self, name: &str, contents: &[u8]) -> Result<(), FolderError> {
        ensure_dir(&self.dir)?;
        let target = self.dir.join(name);
        let temp = self.dir.join(format!("{name}.tmp"));
        if std::fs::write(&temp, contents).is_ok() && std::fs::rename(&temp, &target).is_ok() {
            return Ok(());
        }
        let _ = std::fs::remove_file(&temp);
        std::fs::write(&target, contents).map_err(map_err)
    }

    fn read(&self, name: &str) -> Result<Vec<u8>, FolderError> {
        match std::fs::read(self.dir.join(name)) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(FolderError::NotFound(name.to_string()))
            }
            Err(error) => Err(map_err(error)),
        }
    }

    fn list(&self) -> Result<Vec<String>, FolderError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(map_err(error)),
        };
        let mut names = Vec::new();
        for entry in entries.flatten() {
            if entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
        Ok(names)
    }

    fn remove(&self, name: &str) -> Result<(), FolderError> {
        match std::fs::remove_file(self.dir.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(map_err(error)),
        }
    }

    fn is_writable(&self) -> bool {
        if ensure_dir(&self.dir).is_err() {
            return false;
        }
        let probe = self.dir.join(PROBE_NAME);
        let ok = std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }

    fn handle(&self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
}

fn ensure_dir(dir: &Path) -> Result<(), FolderError> {
    std::fs::create_dir_all(dir).map_err(map_err)
}

fn map_err(error: std::io::Error) -> FolderError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        return FolderError::ReadOnly;
    }
    // EACCES (13) / EROFS (30) cover read-only or unwritable mounts on
    // Linux/Android without depending on the newer `ReadOnlyFilesystem` variant.
    #[cfg(unix)]
    if matches!(error.raw_os_error(), Some(13) | Some(30)) {
        return FolderError::ReadOnly;
    }
    FolderError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    // Tests write under the workspace `target/test-output` (never tmpfs), per
    // the workspace source-hygiene policy.
    fn unique_dir(tag: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-output/cranpose-wfolder");
        let _ = std::fs::create_dir_all(&root);
        root.join(format!("{tag}-{nanos}-{n}"))
    }

    #[test]
    fn round_trips_write_list_read_remove() {
        let dir = unique_dir("rw");
        let store = open(dir.to_string_lossy().as_ref());
        assert!(store.is_writable());

        store.write("a.bin", b"hello").expect("write");
        store.write("b.bin", b"world").expect("write");

        let mut names = store.list().expect("list");
        names.sort();
        assert_eq!(names, vec!["a.bin".to_string(), "b.bin".to_string()]);

        assert_eq!(store.read("a.bin").expect("read"), b"hello");
        assert_eq!(store.handle(), dir.to_string_lossy());

        store.remove("a.bin").expect("remove");
        assert_eq!(store.list().expect("list"), vec!["b.bin".to_string()]);

        // No probe/temp files left behind.
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp") || n.contains("write-probe"))
            .collect();
        assert!(leftover.is_empty(), "stray files: {leftover:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_missing_is_not_found() {
        let dir = unique_dir("missing");
        let store = open(dir.to_string_lossy().as_ref());
        assert!(matches!(store.read("nope"), Err(FolderError::NotFound(_))));
        assert!(store.list().expect("list").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
