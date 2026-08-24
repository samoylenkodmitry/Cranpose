//! Local-network peer streaming for Cranpose apps.
//!
//! One app instance **serves** byte ranges of content it can read (files,
//! `content://` documents, …) over plain HTTP on the LAN; another instance
//! **fetches** them. This is the transport for peer-to-peer media sharing — e.g.
//! one player streaming another player's library on the same network, with no
//! server, no cloud, and no account.
//!
//! The transport is deliberately the portable half: [`PeerServer`] and
//! [`fetch_range`] are pure `std::net` and work the same on desktop, Android,
//! and iOS. The *non*-portable pieces — LAN discovery (mDNS/NSD/Bonjour) and the
//! Android keep-alive foreground service — live elsewhere; this module only
//! moves bytes once you know an address.
//!
//! ## Model
//!
//! The app supplies a [`SourceResolver`]: given an opaque **handle** (a string
//! the app chose when it shared something), return a [`ByteSource`] or `None`.
//! The server only ever serves handles the resolver recognizes, so it is **not**
//! an open file server — an app exposes exactly what it decided to share. Every
//! request must carry the shared `Bearer` token established out-of-band (e.g. by
//! a pairing step), so a stray device on the LAN cannot read anything.
//!
//! `GET /track/{handle}` with an optional `Range: bytes=START-END` header
//! returns `200`/`206` with the bytes; `401` without the token; `404` for an
//! unknown handle.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Errors from peer serving or fetching.
#[derive(thiserror::Error, Debug)]
pub enum PeerError {
    /// The token was missing or wrong.
    #[error("peer request was not authorized")]
    Unauthorized,
    /// No source is registered for the requested handle.
    #[error("peer handle not found")]
    NotFound,
    /// The peer returned an unexpected HTTP status.
    #[error("peer returned HTTP {0}")]
    Status(u16),
    /// A malformed request/response or other protocol error.
    #[error("peer protocol error: {0}")]
    Protocol(String),
    /// An underlying I/O failure.
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for PeerError {
    fn from(error: std::io::Error) -> Self {
        PeerError::Io(error.to_string())
    }
}

/// Random-access source of bytes the app is willing to serve.
///
/// Implementations are `Send + Sync` so the server can read from a worker
/// thread. A non-seekable backing store (e.g. a streamed `content://` document)
/// should spool internally so `read_at` still answers arbitrary offsets.
pub trait ByteSource: Send + Sync {
    /// Total length in bytes, if known. `None` disables `Range` responses.
    fn len(&self) -> Option<u64>;

    /// Reads up to `buf.len()` bytes starting at `offset`; returns the number
    /// read (`0` at end of source).
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize>;

    /// Whether the source is empty. Provided for the `clippy::len_without_is_empty` lint.
    fn is_empty(&self) -> bool {
        self.len() == Some(0)
    }
}

/// In-memory [`ByteSource`] backed by a byte buffer. Handy for small payloads
/// and tests.
pub struct BytesSource {
    bytes: Vec<u8>,
}

impl BytesSource {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }
}

impl ByteSource for BytesSource {
    fn len(&self) -> Option<u64> {
        Some(self.bytes.len() as u64)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> std::io::Result<usize> {
        let offset = offset.min(self.bytes.len() as u64) as usize;
        let available = &self.bytes[offset..];
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        Ok(n)
    }
}

/// Resolves a shared handle to its [`ByteSource`], or `None` if not shared.
pub type SourceResolver = Arc<dyn Fn(&str) -> Option<Arc<dyn ByteSource>> + Send + Sync>;

/// A running peer server. Dropping it stops accepting new connections.
pub struct PeerServer {
    addr: SocketAddr,
    running: Arc<AtomicBool>,
}

impl PeerServer {
    /// Binds `bind_addr` (e.g. `"0.0.0.0:0"` for an OS-chosen port) and serves
    /// shared sources, authorizing every request against `token`.
    pub fn start(
        bind_addr: impl ToSocketAddrs,
        token: impl Into<String>,
        resolver: SourceResolver,
    ) -> Result<PeerServer, PeerError> {
        let listener = TcpListener::bind(bind_addr)?;
        let addr = listener.local_addr()?;
        let running = Arc::new(AtomicBool::new(true));
        let token = token.into();

        let loop_running = running.clone();
        std::thread::Builder::new()
            .name("cranpose-peer".to_string())
            .spawn(move || {
                for stream in listener.incoming() {
                    if !loop_running.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let token = token.clone();
                    let resolver = resolver.clone();
                    // One thread per connection so a long stream never blocks
                    // other peers' requests.
                    let _ = std::thread::Builder::new()
                        .name("cranpose-peer-conn".to_string())
                        .spawn(move || {
                            let _ = handle_connection(stream, &token, &resolver);
                        });
                }
            })
            .map_err(|error| PeerError::Io(error.to_string()))?;

        Ok(PeerServer { addr, running })
    }

    /// The bound address (use its `port()` to advertise).
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The bound port.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }
}

impl Drop for PeerServer {
    fn drop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        // Wake the blocking `accept()` so the loop observes the flag and exits.
        let _ = TcpStream::connect(self.addr);
    }
}

fn handle_connection(
    mut stream: TcpStream,
    token: &str,
    resolver: &SourceResolver,
) -> Result<(), PeerError> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(()); // Connection closed (e.g. the shutdown self-connect).
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    let mut authorization = None;
    let mut range = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "authorization" => authorization = Some(value.to_string()),
                "range" => range = parse_range_header(value),
                _ => {}
            }
        }
    }

    if method != "GET" {
        return write_status(&mut stream, 405, "Method Not Allowed");
    }
    if authorization.as_deref() != Some(&format!("Bearer {token}")) {
        return write_status(&mut stream, 401, "Unauthorized");
    }
    let Some(handle) = path.strip_prefix("/track/") else {
        return write_status(&mut stream, 404, "Not Found");
    };
    let handle = crate::content::percent_decode_lossy(handle);
    let Some(source) = resolver(&handle) else {
        return write_status(&mut stream, 404, "Not Found");
    };

    serve_source(&mut stream, source.as_ref(), range)
}

fn serve_source(
    stream: &mut TcpStream,
    source: &dyn ByteSource,
    range: Option<(u64, Option<u64>)>,
) -> Result<(), PeerError> {
    let total = source.len();

    let (status, reason, start, length) = match (range, total) {
        (Some((start, end)), Some(total)) if start < total => {
            let last = end.unwrap_or(total - 1).min(total - 1);
            if last < start {
                return write_status(stream, 416, "Range Not Satisfiable");
            }
            (206, "Partial Content", start, last - start + 1)
        }
        (Some((start, _)), Some(total)) if start >= total => {
            return write_status(stream, 416, "Range Not Satisfiable");
        }
        (_, Some(total)) => (200, "OK", 0, total),
        // Unknown length and a range request: refuse rather than guess.
        (Some(_), None) => return write_status(stream, 416, "Range Not Satisfiable"),
        (None, None) => {
            // Unknown length, full GET: stream until the source is exhausted.
            return serve_unknown_length(stream, source);
        }
    };

    let mut header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {length}\r\nAccept-Ranges: bytes\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n"
    );
    if status == 206 {
        if let Some(total) = total {
            let end = start + length - 1;
            header.push_str(&format!("Content-Range: bytes {start}-{end}/{total}\r\n"));
        }
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes())?;

    stream_bytes(stream, source, start, length)
}

fn serve_unknown_length(stream: &mut TcpStream, source: &dyn ByteSource) -> Result<(), PeerError> {
    // No Content-Length: use chunked-free "read until EOF, then close".
    let header =
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n";
    stream.write_all(header.as_bytes())?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut offset = 0u64;
    loop {
        let n = source.read_at(offset, &mut buf)?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n])?;
        offset += n as u64;
    }
    Ok(())
}

fn stream_bytes(
    stream: &mut TcpStream,
    source: &dyn ByteSource,
    start: u64,
    length: u64,
) -> Result<(), PeerError> {
    let mut buf = vec![0u8; 64 * 1024];
    let mut sent = 0u64;
    while sent < length {
        let want = ((length - sent) as usize).min(buf.len());
        let n = source.read_at(start + sent, &mut buf[..want])?;
        if n == 0 {
            break;
        }
        stream.write_all(&buf[..n])?;
        sent += n as u64;
    }
    Ok(())
}

fn write_status(stream: &mut TcpStream, code: u16, reason: &str) -> Result<(), PeerError> {
    let response =
        format!("HTTP/1.1 {code} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    stream.write_all(response.as_bytes())?;
    Ok(())
}

/// Parses `bytes=START-END` / `bytes=START-` into `(start, Some(end)|None)`.
fn parse_range_header(value: &str) -> Option<(u64, Option<u64>)> {
    let spec = value.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start = start.trim().parse::<u64>().ok()?;
    let end = end.trim();
    let end = if end.is_empty() {
        None
    } else {
        Some(end.parse::<u64>().ok()?)
    };
    Some((start, end))
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Result of a [`fetch_range`] call.
pub struct FetchResult {
    /// Total source length, parsed from `Content-Range` when present.
    pub total_len: Option<u64>,
    /// The fetched bytes.
    pub bytes: Vec<u8>,
}

struct ResponseHead {
    total_len: Option<u64>,
    content_length: Option<u64>,
    reader: BufReader<TcpStream>,
}

/// Connects to `base`, sends the range GET, parses the status line + headers,
/// and returns a reader positioned at the response body. Maps 401/404 to typed
/// errors and rejects any non-2xx.
fn open_request(
    base: &str,
    token: &str,
    handle: &str,
    start: u64,
    len: Option<u64>,
) -> Result<ResponseHead, PeerError> {
    let mut stream = TcpStream::connect(base)?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;

    let range = match len {
        Some(len) if len > 0 => format!("bytes={start}-{}", start + len - 1),
        Some(_) => format!("bytes={start}-{start}"),
        None => format!("bytes={start}-"),
    };
    let request = format!(
        "GET /track/{} HTTP/1.1\r\nHost: {base}\r\nAuthorization: Bearer {token}\r\nRange: {range}\r\nConnection: close\r\n\r\n",
        encode_handle(handle)
    );
    stream.write_all(request.as_bytes())?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    let status = parse_status(&status_line)?;

    let mut total_len = None;
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.trim().parse::<u64>().ok(),
                "content-range" => total_len = parse_content_range_total(value.trim()),
                _ => {}
            }
        }
    }

    match status {
        401 => Err(PeerError::Unauthorized),
        404 => Err(PeerError::NotFound),
        200 | 206 => Ok(ResponseHead {
            total_len,
            content_length,
            reader,
        }),
        other => Err(PeerError::Status(other)),
    }
}

/// Fetches a byte range of a shared handle from a peer at `base` (e.g.
/// `"192.168.1.20:54123"`) into memory. `len = None` fetches to the end.
pub fn fetch_range(
    base: &str,
    token: &str,
    handle: &str,
    start: u64,
    len: Option<u64>,
) -> Result<FetchResult, PeerError> {
    let mut head = open_request(base, token, handle, start, len)?;
    let mut bytes = Vec::new();
    match head.content_length {
        Some(length) => {
            bytes.resize(length as usize, 0);
            head.reader.read_exact(&mut bytes)?;
        }
        None => {
            head.reader.read_to_end(&mut bytes)?;
        }
    }
    Ok(FetchResult {
        total_len: head.total_len,
        bytes,
    })
}

/// Streams a byte range of a shared handle from a peer into `writer`, in chunks,
/// without buffering the whole range in memory — use this to spool a track to
/// disk. Returns the total source length (from `Content-Range`) when known.
pub fn fetch_to_writer(
    base: &str,
    token: &str,
    handle: &str,
    start: u64,
    len: Option<u64>,
    writer: &mut dyn Write,
) -> Result<Option<u64>, PeerError> {
    let mut head = open_request(base, token, handle, start, len)?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut remaining = head.content_length;
    loop {
        let want = match remaining {
            Some(0) => break,
            Some(r) => (r as usize).min(buf.len()),
            None => buf.len(),
        };
        let n = head.reader.read(&mut buf[..want])?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n])?;
        if let Some(r) = remaining.as_mut() {
            *r -= n as u64;
        }
    }
    Ok(head.total_len)
}

/// Returns the total length of a shared handle, via a one-byte range probe.
pub fn content_length(base: &str, token: &str, handle: &str) -> Result<Option<u64>, PeerError> {
    Ok(fetch_range(base, token, handle, 0, Some(1))?.total_len)
}

fn parse_status(line: &str) -> Result<u16, PeerError> {
    line.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| PeerError::Protocol(format!("bad status line: {line:?}")))
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    // "bytes START-END/TOTAL"
    value.rsplit('/').next()?.trim().parse::<u64>().ok()
}

fn encode_handle(handle: &str) -> String {
    let mut out = String::with_capacity(handle.len());
    for byte in handle.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver_for(handle: &'static str, bytes: Vec<u8>) -> SourceResolver {
        Arc::new(move |requested: &str| {
            if requested == handle {
                Some(Arc::new(BytesSource::new(bytes.clone())) as Arc<dyn ByteSource>)
            } else {
                None
            }
        })
    }

    #[test]
    fn round_trips_full_and_partial() {
        let data: Vec<u8> = (0..=255u8).cycle().take(5000).collect();
        let server = PeerServer::start("127.0.0.1:0", "secret", resolver_for("song", data.clone()))
            .expect("start");
        let base = format!("127.0.0.1:{}", server.port());

        let full = fetch_range(&base, "secret", "song", 0, None).expect("full");
        assert_eq!(full.bytes, data);
        assert_eq!(full.total_len, Some(5000));

        let part = fetch_range(&base, "secret", "song", 1000, Some(256)).expect("part");
        assert_eq!(part.bytes, data[1000..1256]);
        assert_eq!(part.total_len, Some(5000));

        assert_eq!(content_length(&base, "secret", "song").unwrap(), Some(5000));
    }

    #[test]
    fn streams_to_writer_without_buffering() {
        let data: Vec<u8> = (0..2000u32).map(|i| i as u8).collect();
        let server =
            PeerServer::start("127.0.0.1:0", "k", resolver_for("s", data.clone())).expect("start");
        let base = format!("127.0.0.1:{}", server.port());
        let mut out = Vec::new();
        let total = fetch_to_writer(&base, "k", "s", 0, None, &mut out).expect("stream");
        assert_eq!(out, data);
        assert_eq!(total, Some(2000));
    }

    #[test]
    fn rejects_wrong_token() {
        let server =
            PeerServer::start("127.0.0.1:0", "right", resolver_for("a", vec![1, 2, 3])).expect("s");
        let base = format!("127.0.0.1:{}", server.port());
        assert!(matches!(
            fetch_range(&base, "wrong", "a", 0, None),
            Err(PeerError::Unauthorized)
        ));
    }

    #[test]
    fn unknown_handle_is_not_found() {
        let server =
            PeerServer::start("127.0.0.1:0", "t", resolver_for("a", vec![1, 2, 3])).expect("s");
        let base = format!("127.0.0.1:{}", server.port());
        assert!(matches!(
            fetch_range(&base, "t", "missing", 0, None),
            Err(PeerError::NotFound)
        ));
    }

    #[test]
    fn handle_is_percent_encoded_round_trip() {
        let server =
            PeerServer::start("127.0.0.1:0", "t", resolver_for("a b/c.mp3", vec![9, 8, 7]))
                .expect("s");
        let base = format!("127.0.0.1:{}", server.port());
        let got = fetch_range(&base, "t", "a b/c.mp3", 0, None).expect("fetch");
        assert_eq!(got.bytes, vec![9, 8, 7]);
    }
}
