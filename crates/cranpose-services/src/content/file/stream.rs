use std::{
    cell::Cell,
    fs::ReadDir,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

use super::{FileContent, PathInfo, io_error, metadata_for};
use crate::content::{
    ContentError, ContentFuture, ContentHandle, ContentMetadata, ContentStream, ContentStreamRef,
    run_threaded,
};

pub(super) fn open(root: PathBuf) -> ContentStreamRef {
    Rc::new(Stream {
        scope: Arc::new(super::folder_reference::FolderReference::new(root.clone())),
        walk: Arc::new(Mutex::new(Walk {
            pending: vec![root],
            current: None,
        })),
        produced: Cell::new(0),
    })
}

struct Stream {
    scope: Arc<super::folder_reference::FolderReference>,
    walk: Arc<Mutex<Walk>>,
    produced: Cell<usize>,
}

impl ContentStream for Stream {
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>> {
        let walk = Arc::clone(&self.walk);
        Box::pin(async move {
            let entry = run_threaded(move || {
                let mut walk = walk
                    .lock()
                    .map_err(|_| ContentError::Io("folder scan failed".into()))?;
                walk.next()
            })
            .await?;
            Ok(entry.map(|(path, metadata)| {
                self.produced.set(self.produced.get() + 1);
                Rc::new(FileContent {
                    scope: Some(Arc::clone(&self.scope)),
                    source: PathInfo {
                        path,
                        metadata: Some(metadata),
                    },
                }) as ContentHandle
            }))
        })
    }

    fn produced(&self) -> Option<usize> {
        Some(self.produced.get())
    }
}

struct Walk {
    pending: Vec<PathBuf>,
    current: Option<(PathBuf, ReadDir)>,
}

impl Walk {
    fn next(&mut self) -> Result<Option<(PathBuf, ContentMetadata)>, ContentError> {
        loop {
            if self.current.is_none() {
                let Some(path) = self.pending.pop() else {
                    return Ok(None);
                };
                let entries = std::fs::read_dir(&path).map_err(|error| io_error(&path, error))?;
                self.current = Some((path, entries));
            }
            let (directory, entries) = self.current.as_mut().expect("directory opened");
            let Some(entry) = entries.next() else {
                self.current = None;
                continue;
            };
            let entry = entry.map_err(|error| io_error(directory, error))?;
            let path = entry.path();
            let kind = entry.file_type().map_err(|error| io_error(&path, error))?;
            if kind.is_dir() {
                self.pending.push(path);
            } else {
                let metadata = metadata_for(&path);
                return Ok(Some((path, metadata)));
            }
        }
    }
}
