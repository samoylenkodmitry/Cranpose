//! Web file picker backed by `rfd` (the browser file input / File System
//! Access API).
//!
//! Folder picking on the web requires the File System Access API
//! (`showDirectoryPicker`), which is Chromium-only and behind unstable
//! bindings, so it is reported as unsupported here; native platforms (desktop,
//! Android, iOS) provide system-provider folder picking.

use super::{
    FilePickerError, FilePickerOptions, PickedEntry, PickedEntryRef, PickedKind, PickerFuture,
    SaveFileRequest,
};
use std::rc::Rc;
use wasm_bindgen::JsCast;

/// "Saving" on the web is a browser download: the bytes become a Blob object
/// URL clicked through a transient anchor. The user's browser decides the
/// destination, so this resolves `Ok(true)` once the download is handed off.
pub(super) fn save(request: SaveFileRequest) -> PickerFuture<Result<bool, FilePickerError>> {
    Box::pin(async move {
        let window =
            web_sys::window().ok_or_else(|| FilePickerError::Failed("no window".to_string()))?;
        let document = window
            .document()
            .ok_or_else(|| FilePickerError::Failed("no document".to_string()))?;

        let bytes = js_sys::Uint8Array::from(request.bytes.as_slice());
        let parts = js_sys::Array::new();
        parts.push(&bytes.buffer());
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(&request.mime_type);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
            .map_err(|error| FilePickerError::Failed(format!("blob failed: {error:?}")))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|error| FilePickerError::Failed(format!("object URL failed: {error:?}")))?;

        let anchor = document
            .create_element("a")
            .map_err(|error| FilePickerError::Failed(format!("anchor failed: {error:?}")))?;
        let _ = anchor.set_attribute("href", &url);
        let _ = anchor.set_attribute("download", &request.file_name);
        if let Some(html_anchor) = anchor.dyn_ref::<web_sys::HtmlElement>() {
            html_anchor.click();
        }
        let _ = web_sys::Url::revoke_object_url(&url);
        Ok(true)
    })
}

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
