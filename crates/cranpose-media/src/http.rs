use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use cranpose_services::{
    HttpClientRef, HttpControl, HttpRequest, HttpResponse, MediaError, default_http_client,
};
use parking_lot::Mutex;
use symphonia::core::io::MediaSource;

use crate::source::SourceCancel;

const FORWARD_SKIP_LIMIT: u64 = 1 << 20;

pub(crate) fn is_http_uri(uri: &str) -> bool {
    matches!(
        uri.split_once("://"),
        Some((scheme, rest))
            if !rest.is_empty()
                && (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
    )
}

#[derive(Clone, Default)]
struct Abort {
    stopped: Arc<AtomicBool>,
    live: Arc<Mutex<Option<HttpControl>>>,
}

impl Abort {
    fn stop(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(control) = self.live.lock().take() {
            control.cancel();
        }
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn arm(&self, control: HttpControl) {
        *self.live.lock() = Some(control);
    }

    fn disarm(&self) {
        if let Some(control) = self.live.lock().take() {
            control.cancel();
        }
    }
}

struct Stream {
    response: HttpResponse,
    next: u64,
    pending: Vec<u8>,
    taken: usize,
    ended: bool,
}

impl Stream {
    fn ready(&mut self) -> io::Result<usize> {
        while self.taken >= self.pending.len() {
            if self.ended {
                return Ok(0);
            }
            match pollster::block_on(self.response.read_chunk()) {
                Ok(Some(chunk)) => {
                    self.pending = chunk;
                    self.taken = 0;
                }
                Ok(None) => {
                    self.ended = true;
                    return Ok(0);
                }
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        Ok(self.pending.len() - self.taken)
    }

    fn take(&mut self, count: usize) -> &[u8] {
        let from = self.taken;
        self.taken += count;
        self.next += count as u64;
        &self.pending[from..from + count]
    }
}

pub(crate) struct HttpSource {
    client: HttpClientRef,
    url: String,
    len: Option<u64>,
    ranged: bool,
    position: u64,
    stream: Option<Stream>,
    abort: Abort,
}

impl HttpSource {
    pub(crate) fn open(url: &str) -> Result<(HttpSource, SourceCancel), MediaError> {
        HttpSource::open_with(url, default_http_client())
    }

    pub(crate) fn open_with(
        url: &str,
        client: HttpClientRef,
    ) -> Result<(HttpSource, SourceCancel), MediaError> {
        let abort = Abort::default();
        let opened = Opened::request(&client, url, 0, &abort)
            .map_err(|error| MediaError::Failed(format!("{url}: {error}")))?;
        let stopping = abort.clone();
        Ok((
            HttpSource {
                client,
                url: url.to_owned(),
                len: opened.len,
                ranged: opened.ranged,
                position: 0,
                stream: Some(opened.stream),
                abort,
            },
            SourceCancel::new(move || stopping.stop()),
        ))
    }

    fn aligned(&mut self) -> io::Result<()> {
        let target = self.position;
        if let Some(stream) = self.stream.as_mut() {
            if stream.next == target {
                return Ok(());
            }
            if target > stream.next && target - stream.next <= FORWARD_SKIP_LIMIT {
                while stream.next < target {
                    let available = stream.ready()?;
                    if available == 0 {
                        break;
                    }
                    let step = ((target - stream.next) as usize).min(available);
                    stream.take(step);
                }
                if stream.next == target {
                    return Ok(());
                }
            }
        }
        self.reopen()
    }

    fn reopen(&mut self) -> io::Result<()> {
        self.stream = None;
        self.abort.disarm();
        if self.position != 0 && !self.ranged {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("{} does not serve byte ranges", self.url),
            ));
        }
        let opened = Opened::request(&self.client, &self.url, self.position, &self.abort)?;
        if self.len.is_none() {
            self.len = opened.len;
        }
        self.stream = Some(opened.stream);
        Ok(())
    }
}

struct Opened {
    stream: Stream,
    len: Option<u64>,
    ranged: bool,
}

impl Opened {
    fn request(
        client: &HttpClientRef,
        url: &str,
        offset: u64,
        abort: &Abort,
    ) -> Result<Opened, io::Error> {
        let request = HttpRequest::get(url).resume_from(offset);
        let control = HttpControl::new();
        abort.arm(control.clone());
        let response = pollster::block_on(client.send(&request, control))
            .and_then(HttpResponse::error_for_status)
            .map_err(io::Error::other)?;
        if offset > 0 && !response.resumed {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!("the server answered {url} without the byte range it was asked for"),
            ));
        }
        let len = response.content_length.map(|remaining| offset + remaining);
        let ranged = response.resumed || serves_ranges(&response);
        Ok(Opened {
            stream: Stream {
                response,
                next: offset,
                pending: Vec::new(),
                taken: 0,
                ended: false,
            },
            len,
            ranged,
        })
    }
}

fn serves_ranges(response: &HttpResponse) -> bool {
    response
        .header("accept-ranges")
        .is_some_and(|value| value.split(',').any(|unit| unit.trim() == "bytes"))
}

fn offset_from(base: u64, delta: i64) -> io::Result<u64> {
    base.checked_add_signed(delta).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "attempted to seek before the start of the stream",
        )
    })
}

impl Read for HttpSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.abort.is_stopped() {
            return Ok(0);
        }
        if self.len.is_some_and(|len| self.position >= len) {
            return Ok(0);
        }
        self.aligned()?;
        let Some(stream) = self.stream.as_mut() else {
            return Ok(0);
        };
        let available = stream.ready()?;
        if available == 0 {
            return Ok(0);
        }
        let count = available.min(buffer.len());
        buffer[..count].copy_from_slice(stream.take(count));
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => offset_from(self.position, delta)?,
            SeekFrom::End(delta) => {
                let len = self.len.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::Unsupported,
                        format!("{} states no length to seek from the end of", self.url),
                    )
                })?;
                offset_from(len, delta)?
            }
        };
        self.position = target;
        Ok(target)
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        self.ranged
    }

    fn byte_len(&self) -> Option<u64> {
        self.len
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        self.stream = None;
        self.abort.disarm();
    }
}

#[cfg(test)]
#[path = "tests/http_tests.rs"]
mod tests;
