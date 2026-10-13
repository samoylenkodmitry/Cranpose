//! Blocking reads and writes on a thread of their own.
//!
//! A file, a Storage Access Framework document or a security-scoped file is
//! read and written with blocking calls. Behind the async [`ContentReader`]
//! and [`ContentSink`] those calls run here, on a thread per reader or sink,
//! and the thread that awaits them, usually the UI thread, only hands chunks
//! over: copying a gigabyte leaves the screen drawing.

use std::{
    io::{self, ErrorKind, Read},
    rc::Rc,
    sync::mpsc::{Sender, channel},
};

use super::{
    Content, ContentError, ContentFuture, ContentReader, ContentReaderRef, ContentSink,
    ContentSinkRef, DEFAULT_CHUNK_LEN,
};
use crate::async_io::{ChunkChannel, ChunkStream, Signal};

/// Opens a source with `open` on a thread of its own and reads it there,
/// [`DEFAULT_CHUNK_LEN`] bytes at a time and at most
/// [`MAX_PENDING_CHUNKS`](crate::async_io::MAX_PENDING_CHUNKS) chunks ahead of
/// the reader. The future resolves once the source is open; `map_error`
/// describes a read that fails later.
pub fn open_threaded<R, O, M>(
    open: O,
    map_error: M,
) -> ContentFuture<'static, Result<ContentReaderRef, ContentError>>
where
    R: Read,
    O: FnOnce() -> Result<R, ContentError> + Send + 'static,
    M: Fn(io::Error) -> ContentError + Send + 'static,
{
    let (chunks, stream) = ChunkChannel::new();
    let opened = Signal::new();
    let reported = opened.clone();
    let spawned = std::thread::Builder::new()
        .name("cranpose-content-read".into())
        .spawn(move || match open() {
            Ok(source) => {
                reported.set(Ok(()));
                read_into(source, &chunks, &map_error);
            }
            Err(error) => reported.set(Err(error)),
        });
    if let Err(error) = spawned {
        opened.set(Err(not_started(&error)));
    }
    Box::pin(async move {
        opened.wait().await.unwrap_or_else(|| Err(gone()))?;
        Ok(Rc::new(ThreadedReader { stream }) as ContentReaderRef)
    })
}

/// Runs `work` on a thread of its own and resolves with what it returns.
pub fn run_threaded<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ContentError> + Send + 'static,
) -> ContentFuture<'static, Result<T, ContentError>> {
    let done = Signal::new();
    let reported = done.clone();
    let spawned = std::thread::Builder::new()
        .name("cranpose-content-work".into())
        .spawn(move || reported.set(work()));
    if let Err(error) = spawned {
        done.set(Err(not_started(&error)));
    }
    Box::pin(async move { done.wait().await.unwrap_or_else(|| Err(gone())) })
}

/// What a [`threaded_sink`] writes into, on the sink's thread.
pub trait BlockingSink: Send + 'static {
    /// Appends `bytes`.
    fn write(&mut self, bytes: &[u8]) -> Result<(), ContentError>;

    /// Commits what was written. Dropping the sink without it discards what
    /// the destination allows to discard.
    fn finish(&mut self) -> Result<(), ContentError>;
}

/// A [`ContentSink`] that writes into `sink` on a thread of its own; each
/// write resolves once `sink` took the bytes.
pub fn threaded_sink(sink: impl BlockingSink) -> ContentSinkRef {
    let (jobs, taken) = channel::<Job>();
    let spawned = std::thread::Builder::new()
        .name("cranpose-content-write".into())
        .spawn(move || {
            let mut sink = sink;
            for job in taken {
                match job {
                    Job::Write(bytes, done) => done.set(sink.write(&bytes)),
                    Job::Finish(done) => {
                        done.set(sink.finish());
                        return;
                    }
                }
            }
        });
    if let Err(error) = spawned {
        log::warn!("cranpose: {}", not_started(&error));
    }
    Rc::new(ThreadedSink { jobs })
}

/// Reads from `source` until `buffer` is full or the source ends, and returns
/// how many bytes came: fewer than the buffer holds only at the end.
pub fn read_full(source: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match source.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

/// Copies `from` into `to` and commits it; `progress` hears the bytes copied
/// so far after each chunk. Resolves with the bytes copied.
pub async fn copy_content(
    from: &dyn Content,
    to: &ContentSinkRef,
    mut progress: impl FnMut(u64),
) -> Result<u64, ContentError> {
    let reader = from.open().await?;
    let mut copied = 0u64;
    while let Some(chunk) = reader.read_chunk().await? {
        copied += chunk.len() as u64;
        to.write_chunk(chunk).await?;
        progress(copied);
    }
    to.finish().await?;
    Ok(copied)
}

fn read_into(
    mut source: impl Read,
    chunks: &ChunkChannel<ContentError>,
    map_error: &impl Fn(io::Error) -> ContentError,
) {
    loop {
        let mut buffer = vec![0u8; DEFAULT_CHUNK_LEN];
        let filled = match read_full(&mut source, &mut buffer) {
            Ok(filled) => filled,
            Err(error) => {
                chunks.fail(map_error(error));
                return;
            }
        };
        // A chunk that is not full ended at the end of the source.
        let ended = filled < DEFAULT_CHUNK_LEN;
        if filled > 0 {
            buffer.truncate(filled);
            if !chunks.push(buffer) {
                return;
            }
        }
        if ended {
            chunks.finish();
            return;
        }
    }
}

struct ThreadedReader {
    stream: ChunkStream<ContentError>,
}

impl ContentReader for ThreadedReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(self.stream.next())
    }
}

enum Job {
    Write(Vec<u8>, Signal<Result<(), ContentError>>),
    Finish(Signal<Result<(), ContentError>>),
}

struct ThreadedSink {
    jobs: Sender<Job>,
}

impl ThreadedSink {
    fn submit(
        &self,
        job: impl FnOnce(Signal<Result<(), ContentError>>) -> Job,
    ) -> ContentFuture<'_, Result<(), ContentError>> {
        let done = Signal::new();
        let sent = self.jobs.send(job(done.clone()));
        Box::pin(async move {
            sent.map_err(|_| gone())?;
            done.wait().await.unwrap_or_else(|| Err(gone()))
        })
    }
}

impl ContentSink for ThreadedSink {
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>> {
        self.submit(|done| Job::Write(bytes, done))
    }

    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>> {
        self.submit(Job::Finish)
    }
}

fn not_started(error: &io::Error) -> ContentError {
    ContentError::Io(format!("the content thread did not start: {error}"))
}

fn gone() -> ContentError {
    ContentError::Io("the content thread stopped".into())
}

#[cfg(test)]
#[path = "tests/threaded_tests.rs"]
mod tests;
