mod folder_reference;
mod stream;

use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::UNIX_EPOCH,
};

use super::{
    Content, ContentEntry, ContentError, ContentFolder, ContentFolderRef, ContentFuture,
    ContentHandle, ContentMetadata, ContentReaderRef, ContentSinkRef, ContentStreamRef,
    threaded::{BlockingSink, open_threaded, run_threaded, threaded_sink},
};

fn io_error(path: &Path, error: std::io::Error) -> ContentError {
    match error.kind() {
        std::io::ErrorKind::NotFound => ContentError::NotFound(path.display().to_string()),
        std::io::ErrorKind::PermissionDenied => {
            ContentError::PermissionDenied(path.display().to_string())
        }
        _ => ContentError::Io(format!("{}: {error}", path.display())),
    }
}

fn metadata_for(path: &Path) -> ContentMetadata {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut metadata = ContentMetadata {
        name,
        mime_type: None,
        len: None,
        modified_millis: None,
        identifier: path.display().to_string(),
    };
    if let Ok(stat) = std::fs::metadata(path) {
        if stat.is_file() {
            metadata.len = Some(stat.len());
        }
        metadata.modified_millis = stat
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_millis() as u64);
    }
    metadata
}

struct PathInfo {
    path: PathBuf,
    metadata: Option<ContentMetadata>,
}

impl PathInfo {
    fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            metadata: None,
        }
    }

    fn metadata(&self) -> ContentMetadata {
        self.metadata
            .clone()
            .unwrap_or_else(|| metadata_for(&self.path))
    }
}

/// A file on the local filesystem.
pub struct FileContent {
    source: PathInfo,
    scope: Option<Arc<folder_reference::FolderReference>>,
}

impl FileContent {
    /// Wraps `path` as readable content.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            source: PathInfo::new(path),
            scope: None,
        }
    }

    /// The wrapped path.
    pub fn path(&self) -> &Path {
        &self.source.path
    }
}

impl Content for FileContent {
    fn metadata(&self) -> ContentMetadata {
        self.source.metadata()
    }

    fn media_uri(&self) -> Result<String, ContentError> {
        if let Some(scope) = &self.scope {
            return scope.reference(&self.source.path);
        }
        crate::media_source::media_reference(&self.source.path)
            .map_err(|error| io_error(&self.source.path, error))
    }

    fn media_uri_async(&self) -> ContentFuture<'_, Result<String, ContentError>> {
        let path = self.source.path.clone();
        let scope = self.scope.clone();
        run_threaded(move || match scope {
            Some(scope) => scope.reference(&path),
            None => {
                crate::media_source::media_reference(&path).map_err(|error| io_error(&path, error))
            }
        })
    }

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        let opened = self.source.path.clone();
        let path = self.source.path.clone();
        open_threaded(
            move || File::open(&opened).map_err(|error| io_error(&opened, error)),
            move |error| io_error(&path, error),
        )
    }

    fn read_all(&self) -> ContentFuture<'_, Result<Vec<u8>, ContentError>> {
        let path = self.source.path.clone();
        run_threaded(move || std::fs::read(&path).map_err(|error| io_error(&path, error)))
    }
}

/// A shared handle to the file at `path`.
pub fn file_content(path: impl Into<PathBuf>) -> ContentHandle {
    Rc::new(FileContent::new(path))
}

/// A directory on the local filesystem.
pub struct FileFolder {
    root: PathInfo,
}

impl FileFolder {
    /// Wraps `path` as an enumerable folder.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            root: PathInfo::new(path),
        }
    }

    /// The wrapped path.
    pub fn path(&self) -> &Path {
        &self.root.path
    }
}

impl ContentFolder for FileFolder {
    fn metadata(&self) -> ContentMetadata {
        self.root.metadata()
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        let path = self.root.path.clone();
        Box::pin(async move {
            let children = run_threaded(move || {
                let listing = std::fs::read_dir(&path).map_err(|error| io_error(&path, error))?;
                let mut entries = Vec::new();
                for child in listing {
                    let child = child.map_err(|error| io_error(&path, error))?;
                    let child_path = child.path();
                    let kind = child
                        .file_type()
                        .map_err(|error| io_error(&child_path, error))?;
                    let metadata = metadata_for(&child_path);
                    entries.push((child_path, kind.is_dir(), metadata));
                }
                entries.sort_by(|a, b| a.2.name.cmp(&b.2.name));
                Ok(entries)
            })
            .await?;
            Ok(children
                .into_iter()
                .map(|(path, directory, metadata)| {
                    if directory {
                        ContentEntry::Folder(Rc::new(Self {
                            root: PathInfo {
                                path,
                                metadata: Some(metadata),
                            },
                        }))
                    } else {
                        ContentEntry::File(Rc::new(FileContent {
                            scope: None,
                            source: PathInfo {
                                path,
                                metadata: Some(metadata),
                            },
                        }))
                    }
                })
                .collect())
        })
    }

    fn stream_files(&self) -> Option<ContentStreamRef> {
        Some(stream::open(self.root.path.clone()))
    }
}

/// A shared handle to the directory at `path`.
pub fn file_folder(path: impl Into<PathBuf>) -> ContentFolderRef {
    Rc::new(FileFolder::new(path))
}

/// A file being written through a temporary sibling, renamed on
/// [`ContentSink::finish`](super::ContentSink::finish) so a partial write
/// never replaces a good file. Its [`handle`](FileSink::handle) writes on a
/// thread of its own.
pub struct FileSink {
    destination: PathBuf,
    staging: PathBuf,
    file: Option<File>,
}

impl FileSink {
    /// Opens a staged writer for `destination`.
    pub fn create(destination: impl Into<PathBuf>) -> Result<Self, ContentError> {
        let destination = destination.into();
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        }
        let staging = staging_path(&destination);
        let file = File::create(&staging).map_err(|error| io_error(&staging, error))?;
        Ok(Self {
            destination,
            staging,
            file: Some(file),
        })
    }

    /// A shared handle to this sink, which writes on a thread of its own.
    pub fn handle(self) -> ContentSinkRef {
        threaded_sink(self)
    }

    /// The path this sink commits to.
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

fn staging_path(destination: &Path) -> PathBuf {
    let mut name = destination.file_name().map_or_else(
        || "content".to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    name.push_str(".partial");
    destination.with_file_name(name)
}

impl BlockingSink for FileSink {
    fn write(&mut self, bytes: &[u8]) -> Result<(), ContentError> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| ContentError::Io("sink is already finished".into()))?;
        file.write_all(bytes)
            .map_err(|error| io_error(&self.staging, error))
    }

    fn finish(&mut self) -> Result<(), ContentError> {
        let Some(mut file) = self.file.take() else {
            return Ok(());
        };
        file.flush()
            .map_err(|error| io_error(&self.staging, error))?;
        file.sync_all()
            .map_err(|error| io_error(&self.staging, error))?;
        drop(file);
        std::fs::rename(&self.staging, &self.destination)
            .map_err(|error| io_error(&self.destination, error))
    }
}

impl Drop for FileSink {
    fn drop(&mut self) {
        if self.file.is_some() {
            let _ = std::fs::remove_file(&self.staging);
        }
    }
}

#[cfg(test)]
#[path = "tests/file_tests.rs"]
mod tests;
