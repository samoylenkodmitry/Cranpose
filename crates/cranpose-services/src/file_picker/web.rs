//! Web file picker backed by `rfd` (the browser file input / File System
//! Access API).
//!
//! Folder picking on the web requires the File System Access API
//! (`showDirectoryPicker`), which is Chromium-only and behind unstable
//! bindings, so it is reported as unsupported here; native platforms (desktop,
//! Android, iOS) provide system-provider folder picking.

use super::{
    FilePickerError, FilePickerOptions, PickedEntry, PickedEntryRef, PickedKind, PickerFuture,
};
use std::rc::Rc;

pub(super) fn pick(
    options: FilePickerOptions,
    kind: PickedKind,
) -> PickerFuture<Result<Option<PickedEntryRef>, FilePickerError>> {
    Box::pin(async move {
        match kind {
            PickedKind::Folder => Err(FilePickerError::UnsupportedPlatform),
            PickedKind::File => {
                let mut dialog = rfd::AsyncFileDialog::new();
                if let Some(title) = &options.title {
                    dialog = dialog.set_title(title);
                }
                for filter in &options.filters {
                    let extensions: Vec<&str> =
                        filter.extensions.iter().map(String::as_str).collect();
                    dialog = dialog.add_filter(filter.label.clone(), &extensions);
                }
                Ok(dialog
                    .pick_file()
                    .await
                    .map(|handle| Rc::new(WebFileEntry { handle }) as PickedEntryRef))
            }
        }
    })
}

/// A file chosen through the browser. The browser exposes only the file bytes,
/// not a path, so reads go through the in-memory handle.
struct WebFileEntry {
    handle: rfd::FileHandle,
}

impl PickedEntry for WebFileEntry {
    fn name(&self) -> String {
        self.handle.file_name()
    }

    fn kind(&self) -> PickedKind {
        PickedKind::File
    }

    fn display_path(&self) -> String {
        self.handle.file_name()
    }

    fn read_bytes(&self) -> PickerFuture<Result<Vec<u8>, FilePickerError>> {
        let handle = self.handle.clone();
        Box::pin(async move { Ok(handle.read().await) })
    }

    fn list(&self) -> PickerFuture<Result<Vec<PickedEntryRef>, FilePickerError>> {
        Box::pin(async {
            Err(FilePickerError::WrongKind {
                actual: "file",
                expected: "folder",
            })
        })
    }
}
