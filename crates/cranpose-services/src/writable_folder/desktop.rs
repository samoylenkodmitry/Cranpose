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
            if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
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
    #[cfg(unix)]
    if matches!(error.raw_os_error(), Some(13 | 30)) {
        return FolderError::ReadOnly;
    }
    FolderError::Io(error.to_string())
}

#[cfg(test)]
#[path = "tests/desktop_tests.rs"]
mod tests;
