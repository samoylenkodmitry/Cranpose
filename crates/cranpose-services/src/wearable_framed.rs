//! A [`WearableLink`] over a platform link that carries only whole messages,
//! such as WatchConnectivity between an iPhone app and its watch app.
//!
//! Each message the platform carries is one frame, and its first byte says
//! what it holds:
//!
//! - an application message: the path's length (2 bytes, little-endian), the
//!   path, then the data;
//! - the start of a stream: the stream's number (4 bytes, little-endian), then
//!   the path;
//! - the next bytes of a stream: the stream's number, then the bytes;
//! - the end of a stream: the stream's number.
//!
//! A stream writer gathers small writes into frames of up to [`CHUNK_BYTES`],
//! so a microphone that writes every few milliseconds sends a few messages a
//! second.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{Receiver, SyncSender, sync_channel},
    },
};

use parking_lot::Mutex;

use crate::wearable::{
    WearableError, WearableEvent, WearableLink, WearablePeer, WearableStream,
    publish_wearable_event,
};

/// The most bytes of a stream one frame carries. At 16 kHz, 16-bit mono
/// audio this is half a second: two messages a second. WatchConnectivity
/// between simulators fell behind eight a second, and each message wakes the
/// watch's radio.
pub const CHUNK_BYTES: usize = 16 * 1024;

/// How many frames of a stream may wait for its reader before delivery of
/// the next one waits too.
const PENDING_CHUNKS: usize = 256;

const MESSAGE: u8 = 0;
const OPEN: u8 = 1;
const CHUNK: u8 = 2;
const END: u8 = 3;

/// The kind byte and the stream's number.
const STREAM_HEADER: usize = 5;

/// What the platform's message link does for a [`FramedWearableLink`].
pub trait WearableTransport: Send + Sync + 'static {
    /// The connected devices.
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError>;

    /// Delivers one frame to `peer`; frames arrive in the order they were
    /// sent. With `confirm` the call returns once the application on `peer`
    /// has the frame. Without it the call returns once the platform took the
    /// frame, and reports the failure of an earlier frame sent this way.
    fn send(&self, peer: &str, frame: &[u8], confirm: bool) -> Result<(), WearableError>;

    /// Opens `url` on `peer`'s screen.
    fn open_url(&self, peer: &str, url: &str) -> Result<(), WearableError>;
}

/// Application messages and byte streams over a [`WearableTransport`].
///
/// The platform backend installs it with
/// [`set_platform_wearable_link`](crate::set_platform_wearable_link) and hands
/// it every frame the platform receives through [`Self::receive`].
pub struct FramedWearableLink<T> {
    transport: Arc<T>,
    next_stream: AtomicU32,
    incoming: Mutex<HashMap<String, Streams>>,
}

/// The open streams from one device, by their number.
type Streams = HashMap<u32, SyncSender<Vec<u8>>>;

impl<T: WearableTransport> FramedWearableLink<T> {
    /// A link that sends its frames through `transport`.
    pub fn new(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
            next_stream: AtomicU32::new(0),
            incoming: Mutex::new(HashMap::new()),
        }
    }

    /// Handles one frame `peer` sent. A frame for a stream waits while that
    /// stream's reader is far behind.
    pub fn receive(&self, peer: &str, frame: &[u8]) {
        let Some((&kind, body)) = frame.split_first() else {
            log::warn!("wearable: an empty frame from {peer}");
            return;
        };
        if kind == MESSAGE {
            match split_path(body) {
                Some((path, data)) => publish_wearable_event(WearableEvent::Message {
                    peer: peer.to_owned(),
                    path: path.to_owned(),
                    data: data.to_vec(),
                }),
                None => log::warn!("wearable: a message frame without its path from {peer}"),
            }
            return;
        }
        let Some((id, body)) = split_stream(body) else {
            log::warn!("wearable: a stream frame without its number from {peer}");
            return;
        };
        match kind {
            OPEN => self.open_incoming(peer, id, body),
            CHUNK => {
                // A clone, so the lock is free while the reader catches up.
                let sender = self
                    .incoming
                    .lock()
                    .get(peer)
                    .and_then(|streams| streams.get(&id))
                    .cloned();
                if let Some(sender) = sender
                    && sender.send(body.to_vec()).is_err()
                {
                    self.end_incoming(peer, id);
                }
            }
            END => self.end_incoming(peer, id),
            _ => log::warn!("wearable: a frame of unknown kind {kind} from {peer}"),
        }
    }

    /// Ends every stream from `peer`, whose readers then reach the end of
    /// their data. The backend calls it when the device goes out of reach.
    pub fn disconnect(&self, peer: &str) {
        self.incoming.lock().remove(peer);
    }

    fn open_incoming(&self, peer: &str, id: u32, path: &[u8]) {
        let Ok(path) = std::str::from_utf8(path) else {
            log::warn!("wearable: a stream from {peer} with a path that is not UTF-8");
            return;
        };
        let (sender, chunks) = sync_channel(PENDING_CHUNKS);
        // A number the device used before belongs to a stream it lost, which
        // ends here.
        self.incoming
            .lock()
            .entry(peer.to_owned())
            .or_default()
            .insert(id, sender);
        publish_wearable_event(WearableEvent::Stream(WearableStream::new(
            peer.to_owned(),
            path.to_owned(),
            Box::new(ChunkReader {
                chunks,
                chunk: Vec::new(),
                read: 0,
            }),
        )));
    }

    fn end_incoming(&self, peer: &str, id: u32) {
        if let Some(streams) = self.incoming.lock().get_mut(peer) {
            streams.remove(&id);
        }
    }
}

impl<T: WearableTransport> WearableLink for FramedWearableLink<T> {
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError> {
        self.transport.peers()
    }

    fn send_message(&self, peer: &str, path: &str, data: &[u8]) -> Result<(), WearableError> {
        let length = u16::try_from(path.len())
            .map_err(|_| WearableError::Failed(format!("the path {path} is too long")))?;
        let mut frame = Vec::with_capacity(3 + path.len() + data.len());
        frame.push(MESSAGE);
        frame.extend_from_slice(&length.to_le_bytes());
        frame.extend_from_slice(path.as_bytes());
        frame.extend_from_slice(data);
        self.transport.send(peer, &frame, true)
    }

    fn open_stream(&self, peer: &str, path: &str) -> Result<Box<dyn Write + Send>, WearableError> {
        let id = self.next_stream.fetch_add(1, Ordering::Relaxed);
        let mut frame = Vec::with_capacity(STREAM_HEADER + CHUNK_BYTES.max(path.len()));
        frame.push(OPEN);
        frame.extend_from_slice(&id.to_le_bytes());
        frame.extend_from_slice(path.as_bytes());
        self.transport.send(peer, &frame, true)?;
        frame[0] = CHUNK;
        frame.truncate(STREAM_HEADER);
        Ok(Box::new(ChunkWriter {
            transport: Arc::clone(&self.transport),
            peer: peer.to_owned(),
            frame,
        }))
    }

    fn open_url(&self, peer: &str, url: &str) -> Result<(), WearableError> {
        self.transport.open_url(peer, url)
    }
}

fn split_path(body: &[u8]) -> Option<(&str, &[u8])> {
    let (length, rest) = body.split_first_chunk::<2>()?;
    let (path, data) = rest.split_at_checked(usize::from(u16::from_le_bytes(*length)))?;
    Some((std::str::from_utf8(path).ok()?, data))
}

fn split_stream(body: &[u8]) -> Option<(u32, &[u8])> {
    let (id, rest) = body.split_first_chunk::<4>()?;
    Some((u32::from_le_bytes(*id), rest))
}

/// The sending half of a stream: one frame it fills and sends when full, on
/// [`Write::flush`], and when dropped.
struct ChunkWriter<T: WearableTransport> {
    transport: Arc<T>,
    peer: String,
    frame: Vec<u8>,
}

impl<T: WearableTransport> ChunkWriter<T> {
    fn send_chunk(&mut self) -> io::Result<()> {
        if self.frame.len() == STREAM_HEADER {
            return Ok(());
        }
        let sent = self.transport.send(&self.peer, &self.frame, false);
        self.frame.truncate(STREAM_HEADER);
        sent.map_err(io::Error::other)
    }
}

impl<T: WearableTransport> Write for ChunkWriter<T> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let room = STREAM_HEADER + CHUNK_BYTES - self.frame.len();
        let taken = data.len().min(room);
        self.frame.extend_from_slice(&data[..taken]);
        if self.frame.len() == STREAM_HEADER + CHUNK_BYTES {
            self.send_chunk()?;
        }
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.send_chunk()
    }
}

impl<T: WearableTransport> Drop for ChunkWriter<T> {
    fn drop(&mut self) {
        if let Err(error) = self.send_chunk() {
            log::warn!(
                "wearable: the last bytes of a stream to {}: {error}",
                self.peer
            );
        }
        self.frame[0] = END;
        if let Err(error) = self.transport.send(&self.peer, &self.frame, false) {
            log::warn!("wearable: the end of a stream to {}: {error}", self.peer);
        }
    }
}

/// The receiving half of a stream.
struct ChunkReader {
    chunks: Receiver<Vec<u8>>,
    chunk: Vec<u8>,
    read: usize,
}

impl Read for ChunkReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.read == self.chunk.len() {
            match self.chunks.recv() {
                Ok(chunk) => {
                    self.chunk = chunk;
                    self.read = 0;
                }
                // The sender ended the stream, or the link lost the device.
                Err(_) => return Ok(0),
            }
        }
        let count = out.len().min(self.chunk.len() - self.read);
        out[..count].copy_from_slice(&self.chunk[self.read..self.read + count]);
        self.read += count;
        Ok(count)
    }
}

#[cfg(test)]
#[path = "tests/wearable_framed_tests.rs"]
mod tests;
