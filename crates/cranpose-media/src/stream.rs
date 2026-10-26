use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{Arc, Condvar, Mutex, PoisonError},
    time::Duration,
};

use crate::source::SourceCancel;

const CHUNK_BYTES: usize = 64 * 1024;
const BUFFER_BYTES: usize = 4 * CHUNK_BYTES;
const STALL_TIMEOUT: Duration = Duration::from_secs(20);

struct Buffer {
    bytes: Vec<u8>,
    start: usize,
    len: usize,
    finished: bool,
    stopped: bool,
    error: Option<io::Error>,
}

struct Shared {
    buffer: Mutex<Buffer>,
    ready: Condvar,
}

impl Shared {
    fn stop(&self) {
        self.buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stopped = true;
        self.ready.notify_all();
    }
}

pub(crate) struct Stream {
    shared: Arc<Shared>,
    len: Option<u64>,
}

impl Stream {
    pub(crate) fn start(
        mut source: impl Read + Send + 'static,
        len: Option<u64>,
    ) -> io::Result<(Self, SourceCancel)> {
        let shared = Arc::new(Shared {
            buffer: Mutex::new(Buffer {
                bytes: vec![0; BUFFER_BYTES],
                start: 0,
                len: 0,
                finished: false,
                stopped: false,
                error: None,
            }),
            ready: Condvar::new(),
        });
        let producer = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("cranpose-media-stream".into())
            .spawn(move || {
                let mut chunk = vec![0; CHUNK_BYTES];
                loop {
                    {
                        let mut buffer = producer
                            .buffer
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        while BUFFER_BYTES - buffer.len < CHUNK_BYTES && !buffer.stopped {
                            buffer = producer
                                .ready
                                .wait(buffer)
                                .unwrap_or_else(PoisonError::into_inner);
                        }
                        if buffer.stopped {
                            break;
                        }
                    }
                    let result = source.read(&mut chunk);
                    let mut buffer = producer
                        .buffer
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    if buffer.stopped {
                        break;
                    }
                    match result {
                        Ok(0) => buffer.finished = true,
                        Ok(read) => {
                            let end = (buffer.start + buffer.len) % BUFFER_BYTES;
                            let first = read.min(BUFFER_BYTES - end);
                            buffer.bytes[end..end + first].copy_from_slice(&chunk[..first]);
                            buffer.bytes[..read - first].copy_from_slice(&chunk[first..read]);
                            buffer.len += read;
                        }
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            buffer.error = Some(error);
                            buffer.finished = true;
                        }
                    }
                    producer.ready.notify_all();
                    if buffer.finished {
                        break;
                    }
                }
            })?;
        let stopping = Arc::clone(&shared);
        Ok((
            Self { shared, len },
            SourceCancel::new(move || stopping.stop()),
        ))
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

impl Read for Stream {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let mut buffer = self
            .shared
            .buffer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while buffer.len == 0 && !buffer.finished && !buffer.stopped {
            let (next, timeout) = self
                .shared
                .ready
                .wait_timeout(buffer, STALL_TIMEOUT)
                .unwrap_or_else(PoisonError::into_inner);
            buffer = next;
            if timeout.timed_out() && buffer.len == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the audio provider stopped responding",
                ));
            }
        }
        if buffer.stopped {
            return Ok(0);
        }
        if buffer.len == 0 {
            return buffer.error.take().map_or(Ok(0), Err);
        }
        let read = output.len().min(buffer.len);
        let first = read.min(BUFFER_BYTES - buffer.start);
        output[..first].copy_from_slice(&buffer.bytes[buffer.start..buffer.start + first]);
        output[first..read].copy_from_slice(&buffer.bytes[..read - first]);
        buffer.start = (buffer.start + read) % BUFFER_BYTES;
        buffer.len -= read;
        self.shared.ready.notify_all();
        Ok(read)
    }
}

impl Seek for Stream {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this audio provider does not support seeking",
        ))
    }
}

impl symphonia::core::io::MediaSource for Stream {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}
