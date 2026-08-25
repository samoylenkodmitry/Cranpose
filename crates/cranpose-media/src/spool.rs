//! A seekable view over a stream that is not one.
//!
//! A document provider that streams — a cloud mount, a WebDAV share, an rclone
//! remote — hands back a pipe, not a file. A decoder cannot probe a pipe: it
//! reads the container's header, then wants to go back for the audio, and a
//! pipe has no back. Reading the whole track into memory first is not an answer
//! either; a media player that downloads before it makes a sound is not a media
//! player.
//!
//! So the stream is spooled to a temporary file by a background thread and read
//! from that file: playback starts at the front as soon as the first packets
//! land, and a seek waits only until the target offset has been written. The
//! spool is deleted when the last reader is dropped, which is the end of the
//! track — so a terabyte-scale library costs one track's worth of disk, not a
//! library's.
//!
//! The length is taken from the provider rather than discovered by reading to
//! the end, so a container asking where the end is never waits on the whole
//! download. It is not *reported* as the stream's length, though, and that
//! distinction is the difference between a track that plays and one that does
//! not: a decoder told how long a stream is treats it as random-access and
//! reads the tail while probing — the trailing tags, the index — and the tail
//! is the one part of a spool that arrives last. Observed as a 416 MB album
//! that started instantly while the length was withheld and refused to start
//! at all once it was published.
//!
//! And a wait that sees no new bytes at all for [`STALL_TIMEOUT`] gives up with
//! an error, so an item whose provider stopped talking fails and says why
//! instead of the decode thread hanging on a stream that will never deliver
//! another byte.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    time::Duration,
};

/// How much is read from the stream at a time. Large enough that a provider
/// serving over a network is not asked for a few bytes at a time.
const CHUNK_BYTES: usize = 64 * 1024;

/// How long a reader waits with nothing at all arriving before it calls the
/// stream dead.
///
/// The clock restarts whenever a byte lands, so a slow provider is not cut off
/// — only one that has stopped. Generous enough to ride out a network that
/// stalls and recovers, short enough that a track which is never going to play
/// says so rather than sitting there.
const STALL_TIMEOUT: Duration = Duration::from_secs(20);

/// What the downloader publishes and the reader waits on.
struct Progress {
    downloaded: u64,
    /// The stream's length, known only once it has been read to the end.
    total: Option<u64>,
    finished: bool,
    error: Option<String>,
}

struct Shared {
    progress: Mutex<Progress>,
    ready: Condvar,
    cancel: AtomicBool,
    path: PathBuf,
    /// How long a reader waits with nothing arriving before it calls the stream
    /// dead. [`STALL_TIMEOUT`] in an application; short in the tests that
    /// exercise a provider which stops talking.
    stall: Duration,
}

/// Stops a spool's readers waiting for bytes that are not coming.
///
/// The sink holds one for the item it is playing, so ending an item does not
/// wait on a decode thread that is itself waiting on a provider which stopped
/// talking. Cloneable and inert for an item that is not spooled at all.
#[derive(Clone, Default)]
pub(crate) struct SpoolCancel {
    shared: Option<Arc<Shared>>,
}

impl SpoolCancel {
    /// Wakes every reader; each returns what it has, which the decoder reads as
    /// the end of the stream.
    pub(crate) fn cancel(&self) {
        let Some(shared) = self.shared.as_ref() else {
            return;
        };
        shared.cancel.store(true, Ordering::Relaxed);
        shared.ready.notify_all();
    }
}

impl Shared {
    /// Blocks until at least `wanted` bytes are on disk, the download ends, or
    /// it is cancelled; returns how many bytes are available.
    ///
    /// A wait that sees nothing arrive for [`STALL_TIMEOUT`] gives up: a
    /// provider whose descriptor stays open while it has stopped serving would
    /// otherwise block this reader for the life of the process.
    fn wait_for(&self, wanted: u64) -> io::Result<u64> {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(error) = &progress.error {
                return Err(io::Error::other(error.clone()));
            }
            if progress.downloaded >= wanted
                || progress.finished
                || self.cancel.load(Ordering::Relaxed)
            {
                return Ok(progress.downloaded);
            }
            let landed = progress.downloaded;
            let (guard, timeout) = self
                .ready
                .wait_timeout(progress, self.stall)
                .unwrap_or_else(|error| error.into_inner());
            progress = guard;
            if timeout.timed_out() && progress.downloaded == landed {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "the stream stopped delivering after {} bytes",
                        progress.downloaded
                    ),
                ));
            }
        }
    }

    fn fail(&self, message: String) {
        log::error!("cranpose-media spool: {message}");
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        progress.error = Some(message);
        progress.finished = true;
        self.ready.notify_all();
    }
}

/// Stops the downloader and deletes the spool file once every reader is gone.
struct Cleanup {
    shared: Arc<Shared>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        self.shared.ready.notify_all();
        let _ = std::fs::remove_file(&self.shared.path);
    }
}

/// A seekable reader over a stream being spooled to a temporary file.
pub(crate) struct Spool {
    shared: Arc<Shared>,
    file: File,
    position: u64,
    /// What the provider says the whole stream is, when it knows.
    ///
    /// Seeking from the end is answered from this rather than by waiting for
    /// the download to finish. It is deliberately not what [`byte_len`] reports
    /// — see the module comment.
    ///
    /// [`byte_len`]: symphonia::core::io::MediaSource::byte_len
    len: Option<u64>,
    /// Deletes the spool when the last reader goes.
    _cleanup: Arc<Cleanup>,
}

impl Spool {
    /// Starts spooling `source` and returns a reader over what has landed,
    /// together with the handle that stops it waiting.
    ///
    /// Returns as soon as the file exists: nothing has been downloaded yet, and
    /// the first read is what waits.
    pub(crate) fn start(
        source: Box<dyn Read + Send>,
        directory: &Path,
        len: Option<u64>,
    ) -> io::Result<(Spool, SpoolCancel)> {
        Spool::start_with(source, directory, len, STALL_TIMEOUT)
    }

    /// As [`start`](Spool::start), for a caller that states its own tolerance
    /// for a stream going quiet.
    pub(crate) fn start_with(
        source: Box<dyn Read + Send>,
        directory: &Path,
        len: Option<u64>,
        stall: Duration,
    ) -> io::Result<(Spool, SpoolCancel)> {
        std::fs::create_dir_all(directory)?;
        sweep_stale_spools(directory);
        let path = directory.join(next_spool_name());
        let writer = File::create(&path)?;
        let reader = File::open(&path)?;
        let shared = Arc::new(Shared {
            progress: Mutex::new(Progress {
                downloaded: 0,
                total: None,
                finished: false,
                error: None,
            }),
            ready: Condvar::new(),
            cancel: AtomicBool::new(false),
            path,
            stall,
        });
        let cleanup = Arc::new(Cleanup {
            shared: Arc::clone(&shared),
        });
        let download = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("cranpose-media-spool".to_owned())
            .spawn(move || run_download(source, writer, download))?;
        let cancel = SpoolCancel {
            shared: Some(Arc::clone(&shared)),
        };
        Ok((
            Spool {
                shared,
                file: reader,
                position: 0,
                len,
                _cleanup: cleanup,
            },
            cancel,
        ))
    }
}

impl Read for Spool {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.shared.wait_for(self.position + 1)?;
        if self.position >= available {
            return Ok(0);
        }
        self.file.seek(SeekFrom::Start(self.position))?;
        let limit = (available - self.position).min(buffer.len() as u64) as usize;
        let read = self.file.read(&mut buffer[..limit])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Spool {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => offset_from(self.position, delta)?,
            SeekFrom::End(delta) => offset_from(self.end()?, delta)?,
        };
        self.position = target;
        Ok(target)
    }
}

impl Spool {
    /// Where the stream ends.
    ///
    /// From the provider when it stated a length, which is the case that
    /// matters: a container asking where the end is on the first seek would
    /// otherwise have to wait for the whole track to arrive to be told.
    fn end(&self) -> io::Result<u64> {
        match self.len {
            Some(len) => Ok(len),
            None => self.shared.wait_for(u64::MAX),
        }
    }
}

impl symphonia::core::io::MediaSource for Spool {
    /// The whole point: a stream that could not seek can, through the spool.
    fn is_seekable(&self) -> bool {
        true
    }

    /// Nothing, whatever the provider stated.
    ///
    /// A length here is a promise of random access, and a decoder that believes
    /// it reads the tail of the stream while probing — which on a spool means
    /// waiting for the whole track before the first sample. The length is still
    /// known and still used, for [`Seek::seek`] from the end; it is only this
    /// answer that has to be "I cannot tell you". See the module comment.
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

fn offset_from(base: u64, delta: i64) -> io::Result<u64> {
    let target = base as i64 + delta;
    u64::try_from(target).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "attempted to seek before the start of the stream",
        )
    })
}

fn run_download(mut source: Box<dyn Read + Send>, mut writer: File, shared: Arc<Shared>) {
    let mut buffer = vec![0u8; CHUNK_BYTES];
    loop {
        if shared.cancel.load(Ordering::Relaxed) {
            break;
        }
        match source.read(&mut buffer) {
            Ok(0) => {
                let mut progress = shared
                    .progress
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                progress.total = Some(progress.downloaded);
                progress.finished = true;
                shared.ready.notify_all();
                break;
            }
            Ok(read) => {
                if let Err(error) = writer.write_all(&buffer[..read]) {
                    shared.fail(format!("spool write failed: {error}"));
                    break;
                }
                let mut progress = shared
                    .progress
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                progress.downloaded += read as u64;
                shared.ready.notify_all();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                shared.fail(format!("stream read failed: {error}"));
                break;
            }
        }
    }
}

fn next_spool_name() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("stream-{}-{sequence}.tmp", std::process::id())
}

/// Deletes spools a previous run left behind. Once per process: after that
/// every file in the directory belongs to a track this process is playing.
fn sweep_stale_spools(directory: &Path) {
    static SWEPT: std::sync::Once = std::sync::Once::new();
    SWEPT.call_once(|| {
        let Ok(entries) = std::fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    });
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use symphonia::core::io::MediaSource;

    use super::*;

    fn directory(tag: &str) -> PathBuf {
        cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
    }

    /// A reader that yields its bytes a few at a time and cannot seek, which is
    /// what a streaming provider hands back. `stall_after` reproduces the
    /// provider that stops serving without ever closing its pipe.
    struct Trickle {
        bytes: Vec<u8>,
        position: usize,
        step: usize,
        stall_after: Option<usize>,
    }

    impl Read for Trickle {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.stall_after.is_some_and(|limit| self.position >= limit) {
                std::thread::sleep(Duration::from_secs(60));
                return Ok(0);
            }
            if self.position >= self.bytes.len() {
                return Ok(0);
            }
            let take = self
                .step
                .min(buffer.len())
                .min(self.bytes.len() - self.position);
            buffer[..take].copy_from_slice(&self.bytes[self.position..self.position + take]);
            self.position += take;
            std::thread::sleep(Duration::from_millis(1));
            Ok(take)
        }
    }

    fn trickle(bytes: Vec<u8>) -> Box<dyn Read + Send> {
        Box::new(Trickle {
            position: 0,
            step: 7,
            bytes,
            stall_after: None,
        })
    }

    fn payload(len: usize) -> Vec<u8> {
        (0..=255u8).cycle().take(len).collect()
    }

    /// The spool as a provider that states its length hands it over.
    fn spool(bytes: &[u8], tag: &str) -> (Spool, SpoolCancel) {
        Spool::start(
            trickle(bytes.to_vec()),
            &directory(tag),
            Some(bytes.len() as u64),
        )
        .expect("spool starts")
    }

    #[test]
    fn everything_written_comes_back_in_order() {
        let source = payload(4096);
        let (mut spool, _cancel) = spool(&source, "order");
        let mut read = Vec::new();
        spool.read_to_end(&mut read).expect("read to end");
        assert_eq!(read, source);
    }

    #[test]
    fn a_seek_backwards_rereads_what_was_already_spooled() {
        let source = payload(2048);
        let (mut spool, _cancel) = spool(&source, "back");
        let mut head = [0u8; 64];
        spool.read_exact(&mut head).expect("read the head");
        spool.seek(SeekFrom::Start(0)).expect("seek to the start");
        let mut again = [0u8; 64];
        spool.read_exact(&mut again).expect("read the head again");
        assert_eq!(head, again);
    }

    /// The case the whole file exists for: the decoder probes the header, then
    /// asks for an offset the downloader has not reached yet, and gets it.
    #[test]
    fn a_seek_forward_waits_for_the_download_to_reach_it() {
        let source = payload(8192);
        let (mut spool, _cancel) = spool(&source, "forward");
        spool.seek(SeekFrom::Start(8000)).expect("seek forward");
        let mut tail = [0u8; 16];
        spool.read_exact(&mut tail).expect("read the tail");
        assert_eq!(tail, source[8000..8016]);
    }

    /// A container's first seek asks where the end is. Answering it from the
    /// length the provider stated is what stops that question downloading the
    /// whole track before a single seek can happen.
    #[test]
    fn seeking_from_a_stated_end_does_not_wait_for_the_download() {
        let source = payload(1_048_576);
        let (mut spool, _cancel) = spool(&source, "stated-end");
        let started = Instant::now();
        let position = spool.seek(SeekFrom::End(-4)).expect("seek from the end");
        assert_eq!(position, 1_048_572);
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "the seek waited {:?} for a length the provider had already stated",
            started.elapsed()
        );
    }

    /// A spool never states a length, however well it knows one: a decoder told
    /// how long a stream is probes its tail, and on a spool the tail is what
    /// arrives last. See the module comment.
    #[test]
    fn a_spool_never_reports_a_length_even_when_the_provider_stated_one() {
        let (spool, _cancel) = spool(&payload(4096), "no-stated-len");
        assert_eq!(spool.byte_len(), None);
        assert_eq!(
            spool.len,
            Some(4096),
            "but it still knows the one it was told"
        );
    }

    /// A provider that states nothing still has to be read to the end to be
    /// measured, and says so rather than pretending to a length.
    #[test]
    fn a_stream_with_no_stated_length_is_measured_by_reading_it() {
        let source = payload(1024);
        let (mut spool, _cancel) =
            Spool::start(trickle(source.clone()), &directory("unstated-end"), None)
                .expect("spool starts");
        assert_eq!(spool.seek(SeekFrom::End(-4)).expect("seek"), 1020);
        let mut tail = [0u8; 4];
        spool.read_exact(&mut tail).expect("read the tail");
        assert_eq!(tail, source[1020..]);
    }

    #[test]
    fn a_spooled_stream_reports_itself_as_seekable() {
        let (spool, _cancel) = spool(&payload(16), "shape");
        assert!(spool.is_seekable());
    }

    #[test]
    fn the_spool_file_is_deleted_when_the_last_reader_goes() {
        let (spool, _cancel) = spool(&payload(512), "cleanup");
        let path = spool.shared.path.clone();
        assert!(path.exists());
        drop(spool);
        // The downloader may still be writing its last chunk when the reader
        // goes; the deletion happens on the reader's thread either way.
        assert!(!path.exists(), "{} survived its reader", path.display());
    }

    #[test]
    fn seeking_before_the_start_is_an_error_rather_than_a_wrap() {
        let (mut spool, _cancel) = spool(&payload(64), "underflow");
        assert!(spool.seek(SeekFrom::Current(-1)).is_err());
    }

    /// A provider that stops serving without closing its descriptor must not
    /// take the decode thread with it: the read fails, so the item fails and
    /// says why.
    #[test]
    fn a_stream_that_stops_delivering_fails_instead_of_waiting_for_ever() {
        let stalled: Box<dyn Read + Send> = Box::new(Trickle {
            bytes: payload(4096),
            position: 0,
            step: 7,
            stall_after: Some(64),
        });
        let (mut spool, _cancel) = Spool::start_with(
            stalled,
            &directory("stall"),
            Some(4096),
            Duration::from_millis(200),
        )
        .expect("spool starts");
        // Long enough to be past the stall, and past what will ever arrive.
        spool.seek(SeekFrom::Start(2048)).expect("seek");
        let error = spool.read(&mut [0u8; 16]).expect_err("the stream is dead");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    /// Cancelling is what lets a sink end an item without waiting on a decode
    /// thread that is waiting on a provider.
    #[test]
    fn cancelling_ends_a_wait_that_would_otherwise_not_return() {
        let stalled: Box<dyn Read + Send> = Box::new(Trickle {
            bytes: payload(4096),
            position: 0,
            step: 7,
            stall_after: Some(64),
        });
        let (mut spool, cancel) = Spool::start_with(
            stalled,
            &directory("cancel"),
            Some(4096),
            Duration::from_secs(30),
        )
        .expect("spool starts");
        spool.seek(SeekFrom::Start(2048)).expect("seek");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            cancel.cancel();
        });
        let started = Instant::now();
        // Nothing is available at 2048, so the read ends the stream rather than
        // waiting out the stall timeout.
        assert_eq!(spool.read(&mut [0u8; 16]).expect("cancelled"), 0);
        assert!(started.elapsed() < Duration::from_secs(30));
    }
}
