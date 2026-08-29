use std::{
    cell::RefCell,
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::UNIX_EPOCH,
};

use super::{
    Content, ContentEntry, ContentError, ContentFolder, ContentFolderRef, ContentFuture,
    ContentHandle, ContentMetadata, ContentReader, ContentReaderRef, ContentSink, ContentSinkRef,
    DEFAULT_CHUNK_LEN,
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
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
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

/// A file on the local filesystem.
pub struct FileContent {
    path: PathBuf,
}

impl FileContent {
    /// Wraps `path` as readable content.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The wrapped path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Content for FileContent {
    fn metadata(&self) -> ContentMetadata {
        metadata_for(&self.path)
    }

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let file = File::open(&path).map_err(|error| io_error(&path, error))?;
            Ok(Rc::new(FileReader {
                path,
                file: RefCell::new(Some(file)),
            }) as ContentReaderRef)
        })
    }

    fn read_all(&self) -> ContentFuture<'_, Result<Vec<u8>, ContentError>> {
        let path = self.path.clone();
        Box::pin(async move { std::fs::read(&path).map_err(|error| io_error(&path, error)) })
    }
}

/// A shared handle to the file at `path`.
pub fn file_content(path: impl Into<PathBuf>) -> ContentHandle {
    Rc::new(FileContent::new(path))
}

struct FileReader {
    path: PathBuf,
    file: RefCell<Option<File>>,
}

impl ContentReader for FileReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(async move {
            let mut slot = self.file.borrow_mut();
            let Some(file) = slot.as_mut() else {
                return Ok(None);
            };
            let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
            let mut filled = 0;
            while filled < buffer.len() {
                match file.read(&mut buffer[filled..]) {
                    Ok(0) => break,
                    Ok(read) => filled += read,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(io_error(&self.path, error)),
                }
            }
            if filled == 0 {
                *slot = None;
                return Ok(None);
            }
            buffer.truncate(filled);
            Ok(Some(buffer))
        })
    }
}

/// A directory on the local filesystem.
pub struct FileFolder {
    path: PathBuf,
}

impl FileFolder {
    /// Wraps `path` as an enumerable folder.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The wrapped path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ContentFolder for FileFolder {
    fn metadata(&self) -> ContentMetadata {
        metadata_for(&self.path)
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        let path = self.path.clone();
        Box::pin(async move {
            let listing = std::fs::read_dir(&path).map_err(|error| io_error(&path, error))?;
            let mut entries = Vec::new();
            for child in listing {
                let child = child.map_err(|error| io_error(&path, error))?;
                let child_path = child.path();
                let is_dir = child
                    .file_type()
                    .map(|kind| kind.is_dir())
                    .unwrap_or_else(|_| child_path.is_dir());
                if is_dir {
                    entries.push(ContentEntry::Folder(
                        Rc::new(FileFolder::new(child_path)) as ContentFolderRef
                    ));
                } else {
                    entries.push(ContentEntry::File(
                        Rc::new(FileContent::new(child_path)) as ContentHandle
                    ));
                }
            }
            entries.sort_by_key(|entry| entry.metadata().name);
            Ok(entries)
        })
    }
}

/// A shared handle to the directory at `path`.
pub fn file_folder(path: impl Into<PathBuf>) -> ContentFolderRef {
    Rc::new(FileFolder::new(path))
}

/// A file being written through a temporary sibling, renamed on
/// [`ContentSink::finish`] so a partial write never replaces a good file.
pub struct FileSink {
    destination: PathBuf,
    staging: PathBuf,
    file: RefCell<Option<File>>,
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
            file: RefCell::new(Some(file)),
        })
    }

    /// A shared handle to this sink.
    pub fn handle(self) -> ContentSinkRef {
        Rc::new(self)
    }

    /// The path this sink commits to.
    pub fn destination(&self) -> &Path {
        &self.destination
    }
}

fn staging_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "content".to_string());
    name.push_str(".partial");
    destination.with_file_name(name)
}

impl ContentSink for FileSink {
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            let mut slot = self.file.borrow_mut();
            let file = slot
                .as_mut()
                .ok_or_else(|| ContentError::Io("sink is already finished".into()))?;
            file.write_all(&bytes)
                .map_err(|error| io_error(&self.staging, error))
        })
    }

    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            let Some(mut file) = self.file.borrow_mut().take() else {
                return Ok(());
            };
            file.flush()
                .map_err(|error| io_error(&self.staging, error))?;
            file.sync_all()
                .map_err(|error| io_error(&self.staging, error))?;
            drop(file);
            std::fs::rename(&self.staging, &self.destination)
                .map_err(|error| io_error(&self.destination, error))
        })
    }
}

impl Drop for FileSink {
    fn drop(&mut self) {
        if self.file.borrow().is_some() {
            let _ = std::fs::remove_file(&self.staging);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::{DEFAULT_CHUNK_LEN, collect_stream, folder_files, write_all};

    fn temp_dir(name: &str) -> PathBuf {
        crate::test_scratch_dir(&format!("content-{name}"))
    }

    #[test]
    fn a_file_streams_in_chunks_and_reports_its_length() {
        let root = temp_dir("chunks");
        let path = root.join("payload.bin");
        let payload = vec![3u8; DEFAULT_CHUNK_LEN + 5];
        std::fs::write(&path, &payload).unwrap();

        let content = file_content(&path);
        assert_eq!(content.metadata().name, "payload.bin");
        assert_eq!(content.metadata().len, Some(payload.len() as u64));

        let sizes = pollster::block_on(async {
            let reader = content.open().await.unwrap();
            let mut sizes = Vec::new();
            while let Some(chunk) = reader.read_chunk().await.unwrap() {
                sizes.push(chunk.len());
            }
            sizes
        });
        assert_eq!(sizes, vec![DEFAULT_CHUNK_LEN, 5]);
        assert_eq!(pollster::block_on(content.read_all()).unwrap(), payload);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_missing_file_reports_not_found() {
        let root = temp_dir("missing");
        let content = file_content(root.join("absent.bin"));
        assert!(matches!(
            pollster::block_on(content.read_all()),
            Err(ContentError::NotFound(_))
        ));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_folder_streams_its_whole_tree() {
        let root = temp_dir("tree");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("nested/b.txt"), b"b").unwrap();

        let stream = folder_files(file_folder(&root));
        let mut names: Vec<String> = pollster::block_on(collect_stream(&stream))
            .unwrap()
            .iter()
            .map(|file| file.metadata().name)
            .collect();
        names.sort();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_sink_commits_atomically_and_discards_unfinished_writes() {
        let root = temp_dir("sink");
        let destination = root.join("out/report.bin");

        let sink = FileSink::create(&destination).unwrap().handle();
        pollster::block_on(write_all(&sink, b"committed".to_vec())).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"committed");

        let abandoned = root.join("out/abandoned.bin");
        {
            let sink = FileSink::create(&abandoned).unwrap();
            pollster::block_on(sink.write_chunk(b"partial".to_vec())).unwrap();
        }
        assert!(!abandoned.exists());
        assert!(!staging_path(&abandoned).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
