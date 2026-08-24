//! Desktop writable-folder backend: `rfd` folder picker + `std::fs` I/O.
//!
//! The chosen directory may be a plain folder or a mounted network share
//! (GVFS/rclone/davfs over the same WebDAV the app already reads). Writes use
//! temp-file-then-rename for atomicity and fall back to a direct write when the
//! backing filesystem rejects rename. A permission/EROFS failure maps to
//! [`FolderError::ReadOnly`] so the caller can degrade to receive-only.

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

use super::{
    FolderEntry, FolderError, FolderReader, FolderWriter, WritableFolderStore,
    WritableFolderStoreRef,
};
use crate::content::DEFAULT_CHUNK_LEN;

/// Fixed-name probe used by [`is_writable`](WritableFolderStore::is_writable);
/// created and immediately deleted, so it never accumulates.
const PROBE_NAME: &str = ".cranpose-write-probe";

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

    fn list(&self) -> Result<Vec<FolderEntry>, FolderError> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(map_err(error)),
        };
        let mut listing = Vec::new();
        for entry in entries.flatten() {
            if !entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let stat = entry.metadata().map_err(map_err)?;
            listing.push(FolderEntry {
                name,
                len: stat.len(),
                modified_millis: stat
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|since| since.as_millis() as u64),
            });
        }
        Ok(listing)
    }

    fn remove(&self, name: &str) -> Result<(), FolderError> {
        match std::fs::remove_file(self.dir.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(map_err(error)),
        }
    }

    fn open_read(&self, name: &str) -> Result<Box<dyn FolderReader>, FolderError> {
        let path = self.dir.join(name);
        match File::open(&path) {
            Ok(file) => Ok(Box::new(FileFolderReader { file: Some(file) })),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(FolderError::NotFound(name.to_string()))
            }
            Err(error) => Err(map_err(error)),
        }
    }

    fn open_write(&self, name: &str) -> Result<Box<dyn FolderWriter>, FolderError> {
        ensure_dir(&self.dir)?;
        let target = self.dir.join(name);
        let staging = self.dir.join(format!("{name}.tmp"));
        let file = File::create(&staging).map_err(map_err)?;
        Ok(Box::new(FileFolderWriter {
            file: Some(file),
            staging,
            target,
        }))
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

struct FileFolderReader {
    file: Option<File>,
}

impl FolderReader for FileFolderReader {
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, FolderError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(None);
        };
        let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
        let mut filled = 0;
        while filled < buffer.len() {
            match file.read(&mut buffer[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(map_err(error)),
            }
        }
        if filled == 0 {
            self.file = None;
            return Ok(None);
        }
        buffer.truncate(filled);
        Ok(Some(buffer))
    }
}

struct FileFolderWriter {
    file: Option<File>,
    staging: PathBuf,
    target: PathBuf,
}

impl FolderWriter for FileFolderWriter {
    fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), FolderError> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| FolderError::Io("folder writer is already finished".into()))?;
        file.write_all(bytes).map_err(map_err)
    }

    fn finish(mut self: Box<Self>) -> Result<(), FolderError> {
        let Some(mut file) = self.file.take() else {
            return Ok(());
        };
        file.flush().map_err(map_err)?;
        file.sync_all().map_err(map_err)?;
        drop(file);
        std::fs::rename(&self.staging, &self.target).map_err(map_err)
    }
}

impl Drop for FileFolderWriter {
    fn drop(&mut self) {
        if self.file.is_some() {
            let _ = std::fs::remove_file(&self.staging);
        }
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
    use std::{
        sync::atomic::{AtomicU32, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

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

        let mut names: Vec<String> = store
            .list()
            .expect("list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.bin".to_string(), "b.bin".to_string()]);
        assert_eq!(store.entry("a.bin").expect("entry").len, 5);

        assert_eq!(store.read("a.bin").expect("read"), b"hello");
        assert_eq!(store.handle(), dir.to_string_lossy());

        store.remove("a.bin").expect("remove");
        let remaining: Vec<String> = store
            .list()
            .expect("list")
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(remaining, vec!["b.bin".to_string()]);

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
    fn streams_chunks_in_and_out() {
        let dir = unique_dir("stream");
        let store = open(dir.to_string_lossy().as_ref());
        let payload = vec![9u8; DEFAULT_CHUNK_LEN + 7];

        let mut writer = store.open_write("big.bin").expect("open_write");
        writer.write_chunk(&payload).expect("write_chunk");
        writer.finish().expect("finish");

        let mut reader = store.open_read("big.bin").expect("open_read");
        let mut sizes = Vec::new();
        let mut round_trip = Vec::new();
        while let Some(chunk) = reader.read_chunk().expect("read_chunk") {
            sizes.push(chunk.len());
            round_trip.extend_from_slice(&chunk);
        }
        assert_eq!(sizes, vec![DEFAULT_CHUNK_LEN, 7]);
        assert_eq!(round_trip, payload);

        // An abandoned writer leaves nothing behind.
        drop(store.open_write("abandoned.bin").expect("open_write"));
        assert!(!dir.join("abandoned.bin").exists());
        assert!(!dir.join("abandoned.bin.tmp").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_missing_is_not_found() {
        let dir = unique_dir("missing");
        let store = open(dir.to_string_lossy().as_ref());
        assert!(matches!(store.read("nope"), Err(FolderError::NotFound(_))));
        assert!(matches!(
            store.open_read("nope").err(),
            Some(FolderError::NotFound(_))
        ));
        assert!(store.list().expect("list").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
