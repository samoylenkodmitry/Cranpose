use std::{rc::Rc, sync::Arc};

use super::{
    item::{WebDavItem, metadata},
    listing::{self, EntryKind},
};
use crate::{
    ContentEntry, ContentError, ContentFolder, ContentFolderRef, ContentFuture, ContentMetadata,
    HttpClientRef, HttpControl, HttpMethod, HttpRequest,
};

const MAX_LISTING_BYTES: usize = 16 * 1024 * 1024;
const PROPERTIES: &str = r#"<?xml version="1.0"?><d:propfind xmlns:d="DAV:"><d:prop><d:resourcetype/><d:getcontentlength/><d:getcontenttype/></d:prop></d:propfind>"#;

/// A WebDAV folder whose files retain their original HTTP playback references.
pub struct WebDavFolder {
    url: url::Url,
    client: HttpClientRef,
}

impl WebDavFolder {
    /// Opens a folder address through the supplied HTTP client. No request is sent yet.
    pub fn new(address: &str, client: HttpClientRef) -> Result<Self, ContentError> {
        let mut url = url::Url::parse(address).map_err(|e| ContentError::Io(e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
            return Err(ContentError::Unsupported("non-HTTP WebDAV folders"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(ContentError::Unsupported("credentials inside folder URLs"));
        }
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        url.set_fragment(None);
        Ok(Self { url, client })
    }

    /// Converts this folder to the shared content interface.
    pub fn handle(self) -> ContentFolderRef {
        Rc::new(self)
    }
}

impl ContentFolder for WebDavFolder {
    fn metadata(&self) -> ContentMetadata {
        metadata(&self.url)
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        Box::pin(async move {
            let request = HttpRequest::get(self.url.as_str())
                .method(HttpMethod::Propfind)
                .header("Depth", "1")
                .header("Content-Type", "application/xml; charset=utf-8")
                .body(PROPERTIES);
            let control = HttpControl::new();
            let response = self
                .client
                .send(&request, control.clone())
                .await
                .map_err(|e| ContentError::Io(e.to_string()))?;
            if response.status != 207 {
                return Err(ContentError::Io(format!(
                    "WebDAV folder returned HTTP {}",
                    response.status
                )));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response
                .read_chunk()
                .await
                .map_err(|e| ContentError::Io(e.to_string()))?
            {
                if chunk.len() > MAX_LISTING_BYTES - bytes.len() {
                    control.cancel();
                    return Err(ContentError::Io(
                        "WebDAV directory listing exceeds 16 MiB".into(),
                    ));
                }
                bytes.extend_from_slice(&chunk);
            }
            let parent = self.url.clone();
            #[cfg(not(target_arch = "wasm32"))]
            let entries = cranpose_core::withBlocking(move || listing::parse(&bytes, &parent))
                .await
                .map_err(|e| ContentError::Io(e.to_string()))??;
            #[cfg(target_arch = "wasm32")]
            let entries = listing::parse(&bytes, &parent)?;
            Ok(entries
                .into_iter()
                .map(|entry| match entry.kind {
                    EntryKind::Folder => ContentEntry::Folder(
                        Self {
                            url: entry.url,
                            client: Arc::clone(&self.client),
                        }
                        .handle(),
                    ),
                    EntryKind::File { len, mime_type } => ContentEntry::File(Rc::new(
                        WebDavItem::new(entry.url, len, mime_type, Arc::clone(&self.client)),
                    )),
                })
                .collect())
        })
    }
}
