use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

use crate::media::PLATFORM_MEDIA_SOURCE;

/// A media stream the platform opened, and what it knows about it.
pub struct MediaSourceHandle {
    stream: File,
    len: Option<u64>,
    _access: Option<Arc<dyn Send + Sync>>,
}

impl MediaSourceHandle {
    /// Wraps a file descriptor and its reported length.
    pub fn new(stream: File, len: Option<u64>) -> Self {
        Self {
            stream,
            len,
            _access: None,
        }
    }

    /// Keeps a platform access grant alive until the stream closes.
    pub fn with_access(self, access: impl Send + Sync + 'static) -> Self {
        Self {
            _access: Some(Arc::new(access)),
            ..self
        }
    }

    /// Returns the provider's reported byte length, without reading the stream.
    pub fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

impl std::fmt::Debug for MediaSourceHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MediaSourceHandle")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl Read for MediaSourceHandle {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.stream.read(output)
    }
}

impl Seek for MediaSourceHandle {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.stream.seek(position)
    }
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
    fn open(&self, uri: &str) -> io::Result<MediaSourceHandle>;

    /// Retains the platform access grant needed to reopen a selected file.
    fn reference(&self, path: &Path) -> io::Result<String> {
        Ok(crate::media::uri_for_path(path))
    }
}

/// Shared handle to the platform media source opener.
pub type MediaSourceOpenerRef = Arc<dyn MediaSourceOpener>;

pub(crate) fn media_reference(path: &Path) -> io::Result<String> {
    match PLATFORM_MEDIA_SOURCE.get() {
        Some(opener) => opener.reference(path),
        None => Ok(crate::media::uri_for_path(path)),
    }
}
