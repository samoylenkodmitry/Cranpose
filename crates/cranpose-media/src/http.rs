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
mod tests {
    use cranpose_services::{BytesBody, HttpError, HttpResponse, StubHttpClient, http_body_ref};

    use super::*;

    fn payload(len: usize) -> Vec<u8> {
        (0..=255u8).cycle().take(len).collect()
    }

    fn range_of(request: &HttpRequest) -> u64 {
        request.resume_from.unwrap_or(0)
    }

    fn ranged_server(body: Vec<u8>) -> HttpClientRef {
        let requests = Arc::new(Mutex::new(Vec::new()));
        served(body, requests)
    }

    fn served(body: Vec<u8>, requests: Arc<Mutex<Vec<u64>>>) -> HttpClientRef {
        Arc::new(StubHttpClient::new(move |request| {
            let offset = range_of(request);
            requests.lock().push(offset);
            if offset as usize > body.len() {
                return Err(HttpError::HttpStatus {
                    url: request.url.clone(),
                    status: 416,
                });
            }
            let rest = body[offset as usize..].to_vec();
            let length = rest.len() as u64;
            Ok(
                HttpResponse::new(request.url.clone(), if offset > 0 { 206 } else { 200 }, {
                    http_body_ref(BytesBody::chunked(rest, 64))
                })
                .with_headers(vec![("accept-ranges".to_owned(), "bytes".to_owned())])
                .with_content_length(Some(length))
                .with_resumed(offset > 0),
            )
        }))
    }

    fn unranged_server(body: Vec<u8>) -> HttpClientRef {
        Arc::new(StubHttpClient::new(move |request| {
            let length = body.len() as u64;
            Ok(HttpResponse::new(
                request.url.clone(),
                200,
                http_body_ref(BytesBody::chunked(body.clone(), 64)),
            )
            .with_content_length(Some(length)))
        }))
    }

    fn source(client: HttpClientRef) -> HttpSource {
        HttpSource::open_with("https://host/track.mp3", client)
            .expect("the stub answers")
            .0
    }

    #[test]
    fn only_http_and_https_uris_are_claimed() {
        assert!(is_http_uri("https://host/track.mp3"));
        assert!(is_http_uri("HTTP://host/track.mp3"));
        assert!(!is_http_uri("file:///music/track.mp3"));
        assert!(!is_http_uri("content://media/audio/1"));
        assert!(!is_http_uri("/music/track.mp3"));
        assert!(!is_http_uri("https://"));
    }

    #[test]
    fn the_whole_body_comes_back_in_order() {
        let body = payload(4096);
        let mut source = source(ranged_server(body.clone()));

        let mut read = Vec::new();
        source.read_to_end(&mut read).expect("read to end");

        assert_eq!(read, body);
    }

    #[test]
    fn the_stated_length_is_reported_before_anything_is_read() {
        let source = source(ranged_server(payload(9001)));

        assert_eq!(source.byte_len(), Some(9001));
        assert!(source.is_seekable());
    }

    #[test]
    fn a_seek_forward_asks_the_server_for_that_offset() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let body = payload(1 << 22);
        let mut source = source(served(body.clone(), Arc::clone(&requests)));

        source.seek(SeekFrom::Start(3_000_000)).expect("seek");
        let mut head = [0u8; 8];
        source.read_exact(&mut head).expect("read after the seek");

        assert_eq!(head, body[3_000_000..3_000_008]);
        assert_eq!(*requests.lock(), vec![0, 3_000_000]);
    }

    #[test]
    fn a_seek_backwards_asks_the_server_again() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let body = payload(4096);
        let mut source = source(served(body.clone(), Arc::clone(&requests)));

        let mut head = [0u8; 2048];
        source.read_exact(&mut head).expect("read the head");
        source.seek(SeekFrom::Start(16)).expect("seek back");
        let mut again = [0u8; 8];
        source.read_exact(&mut again).expect("read the head again");

        assert_eq!(again, body[16..24]);
        assert_eq!(*requests.lock(), vec![0, 16]);
    }

    #[test]
    fn a_short_hop_forward_stays_on_the_open_response() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let body = payload(8192);
        let mut source = source(served(body.clone(), Arc::clone(&requests)));

        source.seek(SeekFrom::Start(4096)).expect("seek");
        let mut tail = [0u8; 8];
        source.read_exact(&mut tail).expect("read after the hop");

        assert_eq!(tail, body[4096..4104]);
        assert_eq!(
            *requests.lock(),
            vec![0],
            "a hop inside the skip limit re-requested the body"
        );
    }

    #[test]
    fn seeking_from_the_end_uses_the_stated_length() {
        let body = payload(2048);
        let mut source = source(ranged_server(body.clone()));

        assert_eq!(source.seek(SeekFrom::End(-4)).expect("seek"), 2044);
        let mut tail = [0u8; 4];
        source.read_exact(&mut tail).expect("read the tail");

        assert_eq!(tail, body[2044..]);
    }

    #[test]
    fn reading_past_the_stated_end_reports_the_end_of_the_stream() {
        let mut source = source(ranged_server(payload(512)));

        source.seek(SeekFrom::Start(512)).expect("seek");

        assert_eq!(source.read(&mut [0u8; 16]).expect("read"), 0);
    }

    #[test]
    fn seeking_before_the_start_is_an_error_rather_than_a_wrap() {
        let mut source = source(ranged_server(payload(64)));

        assert!(source.seek(SeekFrom::Current(-1)).is_err());
    }

    #[test]
    fn a_server_without_ranges_plays_forward_and_refuses_to_seek() {
        let body = payload(4096);
        let mut source = source(unranged_server(body.clone()));

        assert!(!source.is_seekable());
        let mut head = [0u8; 2048];
        source.read_exact(&mut head).expect("read the head");
        assert_eq!(head.as_slice(), &body[..2048]);
        source.seek(SeekFrom::Start(16)).expect("seek is recorded");

        let error = source.read(&mut [0u8; 16]).expect_err("no ranges, no seek");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn a_server_that_ignores_the_range_it_was_asked_for_is_refused() {
        let client: HttpClientRef = Arc::new(StubHttpClient::new(|request| {
            let body = payload(4096);
            Ok(HttpResponse::new(
                request.url.clone(),
                200,
                http_body_ref(BytesBody::new(body)),
            )
            .with_headers(vec![("accept-ranges".to_owned(), "bytes".to_owned())])
            .with_content_length(Some(4096)))
        }));
        let mut source = source(client);

        source
            .read_exact(&mut [0u8; 2048])
            .expect("read past the offset to come back to");
        source.seek(SeekFrom::Start(16)).expect("seek back");

        let error = source.read(&mut [0u8; 16]).expect_err("the range was lost");
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn a_refusing_server_fails_the_open_rather_than_the_first_read() {
        let client: HttpClientRef = Arc::new(StubHttpClient::new(|request| {
            Ok(HttpResponse::empty(request.url.clone(), 404))
        }));

        let error = HttpSource::open_with("https://host/gone.mp3", client)
            .map(drop)
            .expect_err("a 404 is not a stream");

        assert!(
            matches!(&error, MediaError::Failed(message) if message.contains("gone.mp3")),
            "{error:?}"
        );
    }

    #[test]
    fn cancelling_ends_the_reads_that_follow() {
        let (mut source, cancel) =
            HttpSource::open_with("https://host/track.mp3", ranged_server(payload(4096)))
                .expect("the stub answers");

        cancel.cancel();

        assert_eq!(source.read(&mut [0u8; 16]).expect("cancelled"), 0);
    }
}
