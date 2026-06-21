//! Desktop file picker backed by `rfd` (native dialogs).
//!
//! On Linux the `xdg-portal` backend surfaces mounted locations served by the
//! desktop portal, including GVFS/WebDAV shares, so a picked folder may live on
//! a remote provider just like on mobile.

use super::{
    FilePickerError, FilePickerOptions, PickedEntry, PickedEntryRef, PickedKind, PickerFuture,
};
use std::path::PathBuf;
use std::rc::Rc;

pub(super) fn pick(
    options: FilePickerOptions,
    kind: PickedKind,
) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
    Box::pin(async move {
        let mut dialog = rfd::AsyncFileDialog::new();
        if let Some(title) = &options.title {
            dialog = dialog.set_title(title);
        }
        for filter in &options.filters {
            let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(filter.label.clone(), &extensions);
        }

        let handle = match kind {
            PickedKind::File => dialog.pick_file().await,
            PickedKind::Folder => dialog.pick_folder().await,
        };

        Ok(handle.map(|handle| {
            Rc::new(FsEntry {
                path: handle.path().to_path_buf(),
                kind,
            }) as PickedEntryRef
        }))
    })
}

/// A picked filesystem path.
struct FsEntry {
    path: PathBuf,
    kind: PickedKind,
}

impl PickedEntry for FsEntry {
    fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    fn kind(&self) -> PickedKind {
        self.kind
    }

    fn display_path(&self) -> String {
        self.path.display().to_string()
    }

    fn read_bytes(&self) -> PickerFuture<Result<Vec<u8>, FilePickerError>> {
        if self.kind != PickedKind::File {
            return Box::pin(async {
                Err(FilePickerError::WrongKind {
                    actual: "folder",
                    expected: "file",
                })
            });
        }
        let path = self.path.clone();
        Box::pin(async move {
            std::fs::read(&path).map_err(|error| FilePickerError::ReadFailed(error.to_string()))
        })
    }

    fn list(&self) -> PickerFuture<Result<Vec<PickedEntryRef>, FilePickerError>> {
        if self.kind != PickedKind::Folder {
            return Box::pin(async {
                Err(FilePickerError::WrongKind {
                    actual: "file",
                    expected: "folder",
                })
            });
        }
        let path = self.path.clone();
        Box::pin(async move {
            let read = std::fs::read_dir(&path)
                .map_err(|error| FilePickerError::ReadFailed(error.to_string()))?;
            let mut entries = Vec::new();
            for entry in read {
                let entry =
                    entry.map_err(|error| FilePickerError::ReadFailed(error.to_string()))?;
                let path = entry.path();
                let kind = if path.is_dir() {
                    PickedKind::Folder
                } else {
                    PickedKind::File
                };
                entries.push(Rc::new(FsEntry { path, kind }) as PickedEntryRef);
            }
            Ok(entries)
        })
    }
}
