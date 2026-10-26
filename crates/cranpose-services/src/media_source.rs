use std::{fs::File, sync::Arc};

use crate::{media::path_from_uri, registry::ServiceRegistry};

/// A media stream the platform opened, and what it knows about it.
#[derive(Debug)]
pub struct MediaSourceHandle {
    /// The descriptor to read. Seekable for a real file; a pipe for a provider
    /// that streams.
    pub stream: File,
    /// How long the whole thing is, when the platform knows.
    ///
    /// A provider that streams cannot answer `stat` — that is what makes its
    /// descriptor a pipe — but it listed a size for the document all the same,
    /// and a decoder that knows the length can seek by it instead of waiting
    /// for the stream to end to find out where the end is.
    pub len: Option<u64>,
}

/// Opens a media URI the decoder cannot open for itself.
///
/// A `file:` URI is a path and needs nobody, which is why
/// [`open_media_source`] answers those without asking. A `content://` document
/// belongs to an Android provider and only the platform layer can ask that
/// provider for a descriptor, so the platform layer registers this and the
/// decode thread calls it.
///
/// A descriptor rather than a reader because that is what both sides really
/// have: a real file is seekable, a provider that streams hands back a pipe,
/// and the decoder tells them apart by trying to seek.
pub trait MediaSourceOpener: Send + Sync {
    /// Opens `uri` for reading.
    fn open(&self, uri: &str) -> std::io::Result<MediaSourceHandle>;
}

/// Shared handle to the platform media source opener.
pub type MediaSourceOpenerRef = Arc<dyn MediaSourceOpener>;

static PLATFORM_MEDIA_SOURCE: ServiceRegistry<dyn MediaSourceOpener> = ServiceRegistry::new();

/// Installs the platform media source opener, replacing any previous one.
pub fn set_platform_media_source_opener(opener: MediaSourceOpenerRef) {
    PLATFORM_MEDIA_SOURCE.set(opener);
}

/// Removes the platform media source opener.
pub fn clear_platform_media_source_opener() {
    PLATFORM_MEDIA_SOURCE.clear();
}

/// Opens `uri` for decoding.
///
/// `file:` URIs and bare paths are opened here. Anything else is the platform's
/// to answer, and a platform that registered no opener gets
/// [`ErrorKind::Unsupported`](std::io::ErrorKind::Unsupported) rather than a
/// guess.
pub fn open_media_source(uri: &str) -> std::io::Result<MediaSourceHandle> {
    if let Some(path) = path_from_uri(uri) {
        let stream = File::open(path)?;
        let len = stream.metadata().ok().map(|metadata| metadata.len());
        return Ok(MediaSourceHandle { stream, len });
    }
    match PLATFORM_MEDIA_SOURCE.get() {
        Some(opener) => opener.open(uri),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            format!("no platform opener for {uri}"),
        )),
    }
}
