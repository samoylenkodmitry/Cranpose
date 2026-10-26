use std::{cell::Cell, rc::Rc};

use wasm_bindgen_futures::JsFuture;

use super::{
    Content, ContentError, ContentFuture, ContentHandle, ContentMetadata, ContentReader,
    ContentReaderRef, DEFAULT_CHUNK_LEN,
};

pub(crate) struct BlobContent {
    file: web_sys::File,
}

impl BlobContent {
    pub(crate) fn handle(file: web_sys::File) -> ContentHandle {
        Rc::new(Self { file })
    }
}

impl Content for BlobContent {
    fn metadata(&self) -> ContentMetadata {
        ContentMetadata {
            name: self.file.name(),
            mime_type: Some(self.file.type_()),
            len: Some(self.file.size() as u64),
            modified_millis: Some(self.file.last_modified() as u64),
            identifier: self.file.name(),
        }
    }

    fn media_uri(&self) -> Result<String, ContentError> {
        web_sys::Url::create_object_url_with_blob(&self.file).map_err(|error| {
            ContentError::Io(format!("cannot reference the selected file: {error:?}"))
        })
    }

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        Box::pin(async move {
            Ok(Rc::new(BlobReader {
                file: self.file.clone(),
                position: Cell::new(0),
            }) as ContentReaderRef)
        })
    }
}

struct BlobReader {
    file: web_sys::File,
    position: Cell<u64>,
}

impl ContentReader for BlobReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(async move {
            let start = self.position.get();
            if start >= self.file.size() as u64 {
                return Ok(None);
            }
            let end = (start + DEFAULT_CHUNK_LEN as u64).min(self.file.size() as u64);
            let blob = self
                .file
                .slice_with_f64_and_f64(start as f64, end as f64)
                .map_err(|error| {
                    ContentError::Io(format!("cannot read the selected file: {error:?}"))
                })?;
            let bytes = JsFuture::from(blob.array_buffer()).await.map_err(|error| {
                ContentError::Io(format!("cannot read the selected file: {error:?}"))
            })?;
            self.position.set(end);
            Ok(Some(js_sys::Uint8Array::new(&bytes).to_vec()))
        })
    }
}
