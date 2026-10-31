use std::rc::Rc;

use wasm_bindgen::{JsCast, JsValue, prelude::wasm_bindgen};
use wasm_bindgen_futures::JsFuture;

use super::{FilePickerError, PickerFuture};
use crate::content::{
    ContentEntry, ContentError, ContentFolder, ContentFolderRef, ContentFuture, ContentHandle,
    ContentMetadata, ContentStream, ContentStreamRef, blob::BlobContent,
};

#[wasm_bindgen(module = "/src/file_picker/web_folder.mjs")]
extern "C" {
    #[wasm_bindgen(js_name = pickFolder)]
    fn pick() -> js_sys::Promise;
    #[wasm_bindgen(js_name = folderName)]
    fn name(folder: &JsValue) -> String;
    #[wasm_bindgen(js_name = isFolder)]
    fn is_folder(entry: &JsValue) -> bool;
    fn entries(folder: &JsValue) -> JsValue;
    fn files(folder: &JsValue) -> JsValue;
    #[wasm_bindgen(js_name = nextEntry)]
    fn next(iterator: &JsValue) -> js_sys::Promise;
}

fn error(value: JsValue) -> ContentError {
    ContentError::Io(format!("browser folder: {value:?}"))
}

pub(super) fn pick_folder() -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
    Box::pin(async {
        let folder = JsFuture::from(pick()).await.map_err(error)?;
        Ok((!folder.is_null()).then(|| Rc::new(BrowserFolder(folder)) as ContentFolderRef))
    })
}

struct BrowserFolder(JsValue);

impl ContentFolder for BrowserFolder {
    fn metadata(&self) -> ContentMetadata {
        ContentMetadata::named(name(&self.0))
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        Box::pin(async move {
            let iterator = entries(&self.0);
            let mut children = Vec::new();
            loop {
                let child = JsFuture::from(next(&iterator)).await.map_err(error)?;
                if child.is_null() {
                    return Ok(children);
                }
                children.push(if is_folder(&child) {
                    ContentEntry::Folder(Rc::new(Self(child)))
                } else {
                    ContentEntry::File(BlobContent::handle(child.unchecked_into()))
                });
            }
        })
    }

    fn stream_files(&self) -> Option<ContentStreamRef> {
        Some(Rc::new(BrowserFiles(files(&self.0))))
    }
}

struct BrowserFiles(JsValue);

impl ContentStream for BrowserFiles {
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>> {
        Box::pin(async move {
            let file = JsFuture::from(next(&self.0)).await.map_err(error)?;
            Ok((!file.is_null()).then(|| BlobContent::handle(file.unchecked_into())))
        })
    }
}
