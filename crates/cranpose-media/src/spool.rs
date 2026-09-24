use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::source::SourceCancel;

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

impl Shared {
    fn wait_for(&self, wanted: u64) -> io::Result<u64> {
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
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
                .unwrap_or_else(PoisonError::into_inner);
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
        let mut progress = self.progress.lock().unwrap_or_else(PoisonError::into_inner);
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
    ) -> io::Result<(Spool, SourceCancel)> {
        Spool::start_with(source, directory, len, STALL_TIMEOUT)
    }

    pub(crate) fn start_with(
        source: Box<dyn Read + Send>,
        directory: &Path,
        len: Option<u64>,
        stall: Duration,
    ) -> io::Result<(Spool, SourceCancel)> {
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
        let stopping = Arc::clone(&shared);
        let cancel = SourceCancel::new(move || {
            stopping.cancel.store(true, Ordering::Relaxed);
            stopping.ready.notify_all();
        });
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
                    .unwrap_or_else(PoisonError::into_inner);
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
                    .unwrap_or_else(PoisonError::into_inner);
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
#[path = "tests/spool_tests.rs"]
mod tests;
