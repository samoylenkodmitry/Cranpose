use std::rc::Rc;

use crate::{
    Content, ContentError, ContentFuture, ContentMetadata, ContentReader, ContentReaderRef,
    HttpClientRef, HttpControl, HttpRequest, HttpResponse, percent_decode_lossy,
};

pub(super) fn metadata(url: &url::Url) -> ContentMetadata {
    let name = url
        .path()
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    ContentMetadata::named(percent_decode_lossy(name)).with_identifier(url.as_str())
}

pub(super) struct WebDavItem {
    url: url::Url,
    len: Option<u64>,
    mime_type: Option<String>,
    client: HttpClientRef,
}

impl WebDavItem {
    pub(super) fn new(
        url: url::Url,
        len: Option<u64>,
        mime_type: Option<String>,
        client: HttpClientRef,
    ) -> Self {
        Self {
            url,
            len,
            mime_type,
            client,
        }
    }
}

impl Content for WebDavItem {
    fn metadata(&self) -> ContentMetadata {
        ContentMetadata {
            len: self.len,
            mime_type: self.mime_type.clone(),
            ..metadata(&self.url)
        }
    }

    fn media_uri(&self) -> Result<String, ContentError> {
        Ok(self.url.to_string())
    }

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        Box::pin(async move {
            let request = HttpRequest::get(self.url.as_str());
            let response = self
                .client
                .send(&request, HttpControl::new())
                .await
                .and_then(HttpResponse::error_for_status)
                .map_err(|e| ContentError::Io(e.to_string()))?;
            Ok(Rc::new(Reader(response)) as ContentReaderRef)
        })
    }
}

struct Reader(HttpResponse);

impl ContentReader for Reader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(async move {
            self.0
                .read_chunk()
                .await
                .map_err(|e| ContentError::Io(e.to_string()))
        })
    }
}
