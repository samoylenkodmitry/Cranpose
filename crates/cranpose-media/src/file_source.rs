use std::io::{self, Read, Seek, SeekFrom};

use cranpose_services::{MediaError, MediaSourceHandle};
use symphonia::core::io::MediaSource;

use crate::{source::SourceCancel, stream::Stream};

struct FileSource(MediaSourceHandle);

pub(crate) fn open(uri: &str) -> Result<(Box<dyn MediaSource>, SourceCancel), MediaError> {
    let mut handle = cranpose_services::open_media_source(uri).map_err(|error| {
        if error.kind() == io::ErrorKind::Unsupported {
            MediaError::UnsupportedSource(uri.to_owned())
        } else {
            MediaError::Failed(format!("{uri}: {error}"))
        }
    })?;
    if handle.stream_position().is_ok() {
        return Ok((Box::new(FileSource(handle)), SourceCancel::default()));
    }
    let len = handle.byte_len();
    let (source, cancel) = Stream::start(handle, len)
        .map_err(|error| MediaError::Failed(format!("{uri}: {error}")))?;
    Ok((Box::new(source), cancel))
}

impl Read for FileSource {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.0.read(output)
    }
}

impl Seek for FileSource {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.0.seek(position)
    }
}

impl MediaSource for FileSource {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        self.0.byte_len()
    }
}
