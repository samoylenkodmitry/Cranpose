use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

const CHUNK_BYTES: usize = 64 * 1024;

const STALL_TIMEOUT: Duration = Duration::from_secs(20);

struct Progress {
    downloaded: u64,
    total: Option<u64>,
    finished: bool,
    error: Option<String>,
}

struct Shared {
    progress: Mutex<Progress>,
    ready: Condvar,
    cancel: AtomicBool,
    path: PathBuf,
    stall: Duration,
}

#[derive(Clone, Default)]
pub(crate) struct SpoolCancel {
    shared: Option<Arc<Shared>>,
}

impl SpoolCancel {
    pub(crate) fn cancel(&self) {
        let Some(shared) = self.shared.as_ref() else {
            return;
        };
        shared.cancel.store(true, Ordering::Relaxed);
        shared.ready.notify_all();
    }
}

impl Shared {
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

pub(crate) struct Spool {
    shared: Arc<Shared>,
    file: File,
    position: u64,
    len: Option<u64>,
    _cleanup: Arc<Cleanup>,
}

impl Spool {
    pub(crate) fn start(
        source: Box<dyn Read + Send>,
        directory: &Path,
        len: Option<u64>,
    ) -> io::Result<(Spool, SpoolCancel)> {
        Spool::start_with(source, directory, len, STALL_TIMEOUT)
    }

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
    fn end(&self) -> io::Result<u64> {
        match self.len {
            Some(len) => Ok(len),
            None => self.shared.wait_for(u64::MAX),
        }
    }
}

impl symphonia::core::io::MediaSource for Spool {
    fn is_seekable(&self) -> bool {
        true
    }

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

    #[test]
    fn a_seek_forward_waits_for_the_download_to_reach_it() {
        let source = payload(8192);
        let (mut spool, _cancel) = spool(&source, "forward");
        spool.seek(SeekFrom::Start(8000)).expect("seek forward");
        let mut tail = [0u8; 16];
        spool.read_exact(&mut tail).expect("read the tail");
        assert_eq!(tail, source[8000..8016]);
    }

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
        assert!(!path.exists(), "{} survived its reader", path.display());
    }

    #[test]
    fn seeking_before_the_start_is_an_error_rather_than_a_wrap() {
        let (mut spool, _cancel) = spool(&payload(64), "underflow");
        assert!(spool.seek(SeekFrom::Current(-1)).is_err());
    }

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
        spool.seek(SeekFrom::Start(2048)).expect("seek");
        let error = spool.read(&mut [0u8; 16]).expect_err("the stream is dead");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

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
        assert_eq!(spool.read(&mut [0u8; 16]).expect("cancelled"), 0);
        assert!(started.elapsed() < Duration::from_secs(30));
    }
}
