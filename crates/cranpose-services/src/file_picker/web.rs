use std::{cell::RefCell, rc::Rc};

use wasm_bindgen::JsCast;

use super::{FilePickerError, FilePickerOptions, PickerFuture, SaveDocumentRequest};
use crate::content::{
    BytesContent, ContentError, ContentFolderRef, ContentFuture, ContentHandle, ContentMetadata,
    ContentSink, ContentSinkRef,
};

fn dialog(options: &FilePickerOptions) -> rfd::AsyncFileDialog {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(title) = &options.title {
        dialog = dialog.set_title(title);
    }
    for filter in &options.filters {
        let extensions: Vec<&str> = filter.extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(filter.label.clone(), &extensions);
    }
    dialog
}

async fn content_of(handle: rfd::FileHandle) -> ContentHandle {
    let name = handle.file_name();
    let bytes = handle.read().await;
    BytesContent::new(ContentMetadata::named(name), bytes).handle()
}

pub(super) fn pick_file(
    options: FilePickerOptions,
) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
    Box::pin(async move {
        match dialog(&options).pick_file().await {
            None => Ok(None),
            Some(handle) => Ok(Some(content_of(handle).await)),
        }
    })
}

pub(super) fn pick_files(
    options: FilePickerOptions,
) -> PickerFuture<Result<Vec<ContentHandle>, FilePickerError>> {
    Box::pin(async move {
        let handles = dialog(&options).pick_files().await.unwrap_or_default();
        let mut picked = Vec::with_capacity(handles.len());
        for handle in handles {
            picked.push(content_of(handle).await);
        }
        Ok(picked)
    })
}

pub(super) fn pick_folder(
    _options: FilePickerOptions,
) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
    Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
}

pub(super) fn pick_writable_folder(
    _options: FilePickerOptions,
) -> PickerFuture<Result<Option<String>, FilePickerError>> {
    Box::pin(async { Err(FilePickerError::UnsupportedPlatform) })
}

pub(super) fn save_document(
    request: SaveDocumentRequest,
) -> PickerFuture<Result<Option<ContentSinkRef>, FilePickerError>> {
    Box::pin(async move {
        Ok(Some(Rc::new(DownloadSink {
            file_name: request.file_name,
            mime_type: request.mime_type,
            buffered: RefCell::new(Vec::new()),
        }) as ContentSinkRef))
    })
}

struct DownloadSink {
    file_name: String,
    mime_type: String,
    buffered: RefCell<Vec<u8>>,
}

impl DownloadSink {
    fn download(&self) -> Result<(), ContentError> {
        let window = web_sys::window()
            .ok_or_else(|| ContentError::Io("the document has no window".into()))?;
        let document = window
            .document()
            .ok_or_else(|| ContentError::Io("the window has no document".into()))?;

        let buffered = self.buffered.borrow();
        let bytes = js_sys::Uint8Array::from(buffered.as_slice());
        let parts = js_sys::Array::new();
        parts.push(&bytes.buffer());
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(&self.mime_type);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
            .map_err(|error| ContentError::Io(format!("blob failed: {error:?}")))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob)
            .map_err(|error| ContentError::Io(format!("object URL failed: {error:?}")))?;

        let anchor = document
            .create_element("a")
            .map_err(|error| ContentError::Io(format!("anchor failed: {error:?}")))?;
        let _ = anchor.set_attribute("href", &url);
        let _ = anchor.set_attribute("download", &self.file_name);
        if let Some(html_anchor) = anchor.dyn_ref::<web_sys::HtmlElement>() {
            html_anchor.click();
        }
        let _ = web_sys::Url::revoke_object_url(&url);
        Ok(())
    }
}

impl ContentSink for DownloadSink {
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move {
            self.buffered.borrow_mut().extend_from_slice(&bytes);
            Ok(())
        })
    }

    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>> {
        Box::pin(async move { self.download() })
    }
}
