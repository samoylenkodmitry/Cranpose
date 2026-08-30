#![allow(unsafe_code)]

use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::UNIX_EPOCH,
};

use cranpose_services::{
    DEFAULT_CHUNK_LEN, FilePickerError, FolderEntry, FolderError, FolderReader, FolderWriter,
    WritableFolderStore, WritableFolderStoreRef, set_writable_folder_store_factory,
};
use objc2::runtime::Bool;
use objc2_foundation::{
    NSData, NSURL, NSURLBookmarkCreationOptions, NSURLBookmarkResolutionOptions,
};

pub(crate) fn register() {
    set_writable_folder_store_factory(Box::new(|handle| {
        Some(Arc::new(BookmarkStore {
            handle: handle.to_owned(),
        }) as WritableFolderStoreRef)
    }));
}

pub(crate) fn bookmark_handle(url: &NSURL) -> Result<String, FilePickerError> {
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

    fn list(&self) -> Result<Vec<FolderEntry>, FolderError> {
        self.with_scope(|dir| {
            let mut listing = Vec::new();
            for child in std::fs::read_dir(dir)? {
                let child = child?;
                let stat = child.metadata()?;
                if !stat.is_file() {
                    continue;
                }
                listing.push(FolderEntry {
                    name: child.file_name().to_string_lossy().into_owned(),
                    len: stat.len(),
                    modified_millis: stat
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|since| since.as_millis() as u64),
                });
            }
            Ok(listing)
        })
    }

    fn remove(&self, name: &str) -> Result<(), FolderError> {
        self.with_scope(|dir| match std::fs::remove_file(dir.join(name)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        })
    }

    fn open_read(&self, name: &str) -> Result<Box<dyn FolderReader>, FolderError> {
        let owned = name.to_string();
        let (path, file) = self.with_scope(move |dir| {
            let path = dir.join(&owned);
            let file = File::open(&path)?;
            Ok((path, file))
        })?;
        Ok(Box::new(BookmarkReader {
            handle: self.handle.clone(),
            path,
            file: Some(file),
        }))
    }

    fn open_write(&self, name: &str) -> Result<Box<dyn FolderWriter>, FolderError> {
        let owned = name.to_string();
        let (target, staging, file) = self.with_scope(move |dir| {
            let target = dir.join(&owned);
            let staging = dir.join(format!("{owned}.tmp"));
            let file = File::create(&staging)?;
            Ok((target, staging, file))
        })?;
        Ok(Box::new(BookmarkWriter {
            handle: self.handle.clone(),
            target,
            staging,
            file: Some(file),
        }))
    }

    fn is_writable(&self) -> bool {
        self.with_scope(|dir| Ok(dir.is_dir())).unwrap_or(false)
    }

    fn handle(&self) -> String {
        self.handle.clone()
    }
}

struct BookmarkReader {
    handle: String,
    path: PathBuf,
    file: Option<File>,
}

impl FolderReader for BookmarkReader {
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, FolderError> {
        let Some(file) = self.file.as_mut() else {
            return Ok(None);
        };
        let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
        let store = BookmarkStore {
            handle: self.handle.clone(),
        };
        let path = self.path.clone();
        let filled = store.with_scope(|_| {
            let _ = &path;
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
            self.file = None;
            return Ok(None);
        }
        buffer.truncate(filled);
        Ok(Some(buffer))
    }
}

struct BookmarkWriter {
    handle: String,
    target: PathBuf,
    staging: PathBuf,
    file: Option<File>,
}

impl FolderWriter for BookmarkWriter {
    fn write_chunk(&mut self, bytes: &[u8]) -> Result<(), FolderError> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| FolderError::Io("folder writer is already finished".into()))?;
        let store = BookmarkStore {
            handle: self.handle.clone(),
        };
        store.with_scope(|_| file.write_all(bytes))
    }

    fn finish(mut self: Box<Self>) -> Result<(), FolderError> {
        let Some(mut file) = self.file.take() else {
            return Ok(());
        };
        let store = BookmarkStore {
            handle: self.handle.clone(),
        };
        let staging = self.staging.clone();
        let target = self.target.clone();
        store.with_scope(move |_| {
            file.flush()?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&staging, &target)
        })
    }
}

impl Drop for BookmarkWriter {
    fn drop(&mut self) {
        if self.file.is_some() {
            let store = BookmarkStore {
                handle: self.handle.clone(),
            };
            let staging = self.staging.clone();
            let _ = store.with_scope(move |_| std::fs::remove_file(&staging));
        }
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
