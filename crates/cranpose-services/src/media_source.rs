use std::{fs::File, sync::Arc};

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
/// [`crate::media::open_media_source`] answers those without asking. A `content://` document
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
