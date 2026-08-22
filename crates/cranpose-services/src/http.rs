use cranpose_core::{compositionLocalOfWithPolicy, CompositionLocal};
#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
use futures_util::{stream, StreamExt};
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(thiserror::Error, Debug, Clone)]
pub enum HttpError {
    #[error("Failed to build HTTP client: {0}")]
    ClientInit(String),
    #[error("Request failed for {url}: {message}")]
    RequestFailed { url: String, message: String },
    #[error("Request failed with status {status} for {url}")]
    HttpStatus { url: String, status: u16 },
    #[error("Failed to read response body for {url}: {message}")]
    BodyReadFailed { url: String, message: String },
    #[error("Invalid response for {url}: {message}")]
    InvalidResponse { url: String, message: String },
    #[error("No window object available")]
    NoWindow,
    #[error("{operation} worker thread panicked")]
    WorkerPanicked { operation: &'static str },
    #[error("{operation} requires cranpose-services feature `{feature}`")]
    UnsupportedFeature {
        operation: &'static str,
        feature: &'static str,
    },
    #[error("Download was cancelled")]
    Cancelled,
    #[error("Failed to write downloaded file {path}: {message}")]
    FileWrite { path: String, message: String },
}

/// How much of a transfer has arrived.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HttpProgress {
    /// Bytes transferred so far, counting anything a resumed transfer skipped.
    pub transferred: u64,
    /// The whole size, when the server said. A chunked response does not, and
    /// a progress bar that invents a total for one is lying about it.
    pub total: Option<u64>,
}

impl HttpProgress {
    /// How far through the transfer is, in `0..=1`, or `None` when the server
    /// never said how large it is.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total?;
        if total == 0 {
            return None;
        }
        Some((self.transferred as f32 / total as f32).clamp(0.0, 1.0))
    }

    /// Whether everything the server promised has arrived.
    pub fn is_complete(&self) -> bool {
        self.total.is_some_and(|total| self.transferred >= total)
    }
}

/// What a progress report is handed to.
pub type ProgressHandler = Arc<dyn Fn(HttpProgress) + Send + Sync>;

/// Cancellation and progress shared with an in-flight request.
///
/// One control covers every request rather than only file downloads: a slow
/// response is a slow response whether it is being written to disk or read into
/// memory, and a screen that leaves is a screen that leaves.
#[derive(Clone, Default)]
pub struct HttpControl {
    cancelled: Arc<AtomicBool>,
    progress: Option<ProgressHandler>,
}

impl HttpControl {
    /// A live control with no progress reporting.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports transferred and, where the server says so, total bytes.
    ///
    /// Called from whichever thread is doing the transfer, so the handler must
    /// be prepared for that — the framework's own event stream is the ordinary
    /// way to get it back to composition.
    pub fn with_progress(
        mut self,
        progress: impl Fn(HttpProgress) + Send + Sync + 'static,
    ) -> Self {
        self.progress = Some(Arc::new(progress));
        self
    }

    /// Requests cancellation.
    ///
    /// The transfer stops at its next chunk boundary. A partial file left by a
    /// cancelled download stays on disk, which is what makes the next attempt
    /// able to resume rather than start again.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Hands a progress reading to whatever [`with_progress`](Self::with_progress)
    /// registered, and does nothing where nothing did.
    ///
    /// Called by the backend doing the transfer - including an
    /// [`HttpClient`] an application provides through [`local_http_client`],
    /// which is why this is part of the control's public surface rather than
    /// private to the backends shipped here.
    pub fn report(&self, progress: HttpProgress) {
        if let Some(handler) = &self.progress {
            handler(progress);
        }
    }
}

/// The verbs a request can use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    #[default]
    Get,
    Head,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    /// The name that goes on the wire.
    pub fn name(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Head => "HEAD",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        }
    }
}

/// One request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub method: HttpMethod,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// Resume an interrupted transfer from this byte offset.
    ///
    /// Sent as a `Range` header. A server free to ignore it answers `200` with
    /// the whole body instead of `206` with the rest, which
    /// [`HttpResponse::resumed`] reports — a caller that appends to a partial
    /// file must check it, or it writes the start of the file over the middle.
    pub resume_from: Option<u64>,
}

impl HttpRequest {
    /// A `GET` for `url`.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    pub fn method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Resumes an interrupted transfer from `offset`.
    pub fn resume_from(mut self, offset: u64) -> Self {
        self.resume_from = (offset > 0).then_some(offset);
        self
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, HttpError>> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, HttpError>> + 'a>>;

/// A response body, read a chunk at a time.
///
/// A body is not a `Vec<u8>`: a model pack, a video, an application package are
/// all larger than the memory a phone will hand over, and the only way to read
/// one is to not hold it. Everything that wants the whole thing anyway asks for
/// it explicitly through [`HttpResponse::read_all`].
pub trait HttpBody {
    /// The next chunk, or `None` at the end of the body.
    fn read_chunk(&self) -> HttpFuture<'_, Option<Vec<u8>>>;
}

/// Shared handle to a response body.
///
/// Bounded exactly as [`HttpFuture`] is: a native transfer runs on a thread of
/// its own and its body crosses to whoever awaits it, while a browser's body is
/// a `ReadableStream` reader that belongs to the one thread a page has and
/// cannot claim otherwise.
#[cfg(not(target_arch = "wasm32"))]
pub type HttpBodyRef = Arc<dyn HttpBody + Send + Sync>;

/// Shared handle to a response body.
#[cfg(target_arch = "wasm32")]
pub type HttpBodyRef = Arc<dyn HttpBody>;

/// A body with nothing in it, which is what a `HEAD` answers with.
struct EmptyBody;

impl HttpBody for EmptyBody {
    fn read_chunk(&self) -> HttpFuture<'_, Option<Vec<u8>>> {
        Box::pin(async { Ok(None) })
    }
}

/// A body already in memory, handed out in [`CHUNK_LEN`]-sized pieces.
///
/// For a backend that has the bytes already — a cache, a bundled asset, a test
/// double — so it satisfies the same chunked contract as one still on the wire
/// and its callers cannot tell the difference.
pub struct BytesBody {
    bytes: Vec<u8>,
    offset: std::sync::Mutex<usize>,
    chunk: usize,
}

impl BytesBody {
    /// A body over `bytes`, delivered in one chunk per read.
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self::chunked(bytes, usize::MAX)
    }

    /// A body over `bytes`, delivered at most `chunk` bytes at a time.
    pub fn chunked(bytes: impl Into<Vec<u8>>, chunk: usize) -> Self {
        Self {
            bytes: bytes.into(),
            offset: std::sync::Mutex::new(0),
            chunk: chunk.max(1),
        }
    }

    /// How long the body is.
    pub fn len(&self) -> u64 {
        self.bytes.len() as u64
    }

    /// Whether the body carries nothing.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl HttpBody for BytesBody {
    fn read_chunk(&self) -> HttpFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            let mut offset = self
                .offset
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if *offset >= self.bytes.len() {
                return Ok(None);
            }
            let end = offset.saturating_add(self.chunk).min(self.bytes.len());
            let chunk = self.bytes[*offset..end].to_vec();
            *offset = end;
            Ok(Some(chunk))
        })
    }
}

/// What a server answered, with its body still to be read.
pub struct HttpResponse {
    /// The status line's code.
    pub status: u16,
    /// The response headers, with their names lower-cased.
    pub headers: Vec<(String, String)>,
    /// How long the body is, when the server said.
    pub content_length: Option<u64>,
    /// Whether the server honoured [`HttpRequest::resume_from`].
    ///
    /// `false` from a resumed request means the body starts at the beginning
    /// again, and a caller appending to a partial file must truncate it first.
    pub resumed: bool,
    /// The URL the request went to, for the errors reading the body reports.
    pub url: String,
    body: HttpBodyRef,
}

impl HttpResponse {
    /// Builds a response around a body.
    pub fn new(url: impl Into<String>, status: u16, body: HttpBodyRef) -> Self {
        Self {
            status,
            headers: Vec::new(),
            content_length: None,
            resumed: false,
            url: url.into(),
            body,
        }
    }

    /// A response with no body, which is what a `HEAD` answers with.
    pub fn empty(url: impl Into<String>, status: u16) -> Self {
        Self::new(url, status, Arc::new(EmptyBody))
    }

    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers = headers;
        self
    }

    pub fn with_content_length(mut self, content_length: Option<u64>) -> Self {
        self.content_length = content_length;
        self
    }

    pub fn with_resumed(mut self, resumed: bool) -> Self {
        self.resumed = resumed;
        self
    }

    /// Whether the status is in the 2xx range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// One header, matched without regard to case.
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(&name))
            .map(|(_, value)| value.as_str())
    }

    /// Fails with [`HttpError::HttpStatus`] unless the status is a success.
    pub fn error_for_status(self) -> Result<Self, HttpError> {
        if self.is_success() {
            Ok(self)
        } else {
            Err(HttpError::HttpStatus {
                url: self.url.clone(),
                status: self.status,
            })
        }
    }

    /// The next chunk of the body, or `None` at its end.
    pub async fn read_chunk(&self) -> Result<Option<Vec<u8>>, HttpError> {
        self.body.read_chunk().await
    }

    /// The whole body.
    ///
    /// Named for what it does, so a caller reading something that might not fit
    /// in memory can see that it is asking for exactly that.
    pub async fn read_all(&self) -> Result<Vec<u8>, HttpError> {
        let mut out = Vec::with_capacity(self.content_length.unwrap_or(0).min(1 << 20) as usize);
        while let Some(chunk) = self.read_chunk().await? {
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// The whole body as text.
    pub async fn read_text(&self) -> Result<String, HttpError> {
        String::from_utf8(self.read_all().await?).map_err(|error| HttpError::InvalidResponse {
            url: self.url.clone(),
            message: error.to_string(),
        })
    }
}

/// A platform's HTTP.
///
/// One method carries the whole contract: everything else — text, bytes, a file
/// on disk — is that method plus what the caller does with the body, so a
/// backend implements the transfer once and cannot make `get_text` behave
/// differently from `download_to`.
pub trait HttpClient: Send + Sync {
    /// Sends `request`, resolving when the response's status line has arrived
    /// and its body is ready to be read.
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
        control: HttpControl,
    ) -> HttpFuture<'a, HttpResponse>;

    /// The body of a successful `GET`, as text.
    fn get_text<'a>(&'a self, url: &'a str) -> HttpFuture<'a, String> {
        Box::pin(async move {
            self.send(&HttpRequest::get(url), HttpControl::new())
                .await?
                .error_for_status()?
                .read_text()
                .await
        })
    }

    /// The body of a successful `GET`.
    fn get_bytes<'a>(&'a self, url: &'a str) -> HttpFuture<'a, Vec<u8>> {
        Box::pin(async move {
            self.send(&HttpRequest::get(url), HttpControl::new())
                .await?
                .error_for_status()?
                .read_all()
                .await
        })
    }

    /// Streams a response into `target`, resuming from its current length when
    /// the server supports byte ranges, and returns the file's whole length.
    ///
    /// This is the appropriate call for model packs and other files that should
    /// not be held in memory. A cancelled transfer leaves the partial file
    /// where it is, which is what lets the next attempt continue rather than
    /// start again.
    #[cfg(not(target_arch = "wasm32"))]
    fn download_to<'a>(
        &'a self,
        url: &'a str,
        target: &'a Path,
        control: HttpControl,
    ) -> HttpFuture<'a, u64> {
        Box::pin(async move { download_through(self, url, target, control).await })
    }
}

/// Streams `url` into `target` through `client`, resuming where it left off.
///
/// Written once, against the client trait, so no backend has its own copy of
/// the resume rule — which is the rule most easily got wrong: a server that
/// ignores a `Range` header answers with the whole body, and appending that to
/// a partial file produces a file that is the right length and the wrong bytes.
#[cfg(not(target_arch = "wasm32"))]
async fn download_through<C: HttpClient + ?Sized>(
    client: &C,
    url: &str,
    target: &Path,
    control: HttpControl,
) -> Result<u64, HttpError> {
    use std::io::Write;

    if control.is_cancelled() {
        return Err(HttpError::Cancelled);
    }
    let existing = std::fs::metadata(target)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let mut request = HttpRequest::get(url);
    if existing > 0 {
        request = request.resume_from(existing);
    }
    let response = client
        .send(&request, control.clone())
        .await?
        .error_for_status()?;

    let base = if response.resumed { existing } else { 0 };
    let total = response.content_length.map(|remaining| base + remaining);
    let mut output = if response.resumed {
        std::fs::OpenOptions::new().append(true).open(target)
    } else {
        std::fs::File::create(target)
    }
    .map_err(|error| HttpError::FileWrite {
        path: target.display().to_string(),
        message: error.to_string(),
    })?;

    let mut transferred = base;
    control.report(HttpProgress { transferred, total });
    while let Some(chunk) = response.read_chunk().await? {
        if control.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        output
            .write_all(&chunk)
            .map_err(|error| HttpError::FileWrite {
                path: target.display().to_string(),
                message: error.to_string(),
            })?;
        transferred += chunk.len() as u64;
        control.report(HttpProgress { transferred, total });
    }
    output.flush().map_err(|error| HttpError::FileWrite {
        path: target.display().to_string(),
        message: error.to_string(),
    })?;
    Ok(transferred)
}

pub type HttpClientRef = Arc<dyn HttpClient>;

#[cfg(not(target_arch = "wasm32"))]
pub async fn map_ordered_concurrent<I, T, F, Fut>(
    items: &[I],
    concurrency: usize,
    task: F,
) -> Result<Vec<T>, HttpError>
where
    I: Clone + Send,
    T: Send,
    F: Fn(I) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = T> + Send,
{
    let task = Arc::new(task);
    let mut results = Vec::with_capacity(items.len());

    for chunk in items.chunks(concurrency.max(1)) {
        let chunk_results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(chunk.len());
            for item in chunk.iter().cloned() {
                let task = Arc::clone(&task);
                handles.push(scope.spawn(move || pollster::block_on(task(item))));
            }

            let mut chunk_results = Vec::with_capacity(handles.len());
            for handle in handles {
                let value = handle.join().map_err(|_| HttpError::WorkerPanicked {
                    operation: "ordered concurrent task",
                })?;
                chunk_results.push(value);
            }
            Ok::<Vec<T>, HttpError>(chunk_results)
        })?;
        results.extend(chunk_results);
    }

    Ok(results)
}

#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
pub async fn map_ordered_concurrent<I, T, F, Fut>(
    items: &[I],
    concurrency: usize,
    task: F,
) -> Result<Vec<T>, HttpError>
where
    I: Clone,
    F: Fn(I) -> Fut + Clone,
    Fut: Future<Output = T>,
{
    let mut results = stream::iter(items.iter().cloned().enumerate().map(|(index, item)| {
        let task = task.clone();
        async move { (index, task(item).await) }
    }))
    .buffer_unordered(concurrency.max(1))
    .collect::<Vec<_>>()
    .await;

    results.sort_by_key(|(index, _)| *index);
    Ok(results.into_iter().map(|(_, value)| value).collect())
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
pub async fn map_ordered_concurrent<I, T, F, Fut>(
    items: &[I],
    _concurrency: usize,
    task: F,
) -> Result<Vec<T>, HttpError>
where
    I: Clone,
    F: Fn(I) -> Fut,
    Fut: Future<Output = T>,
{
    Ok(map_ordered_sequential(items, task).await)
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
async fn map_ordered_sequential<I, T, F, Fut>(items: &[I], task: F) -> Vec<T>
where
    I: Clone,
    F: Fn(I) -> Fut,
    Fut: Future<Output = T>,
{
    let mut results = Vec::with_capacity(items.len());
    for item in items.iter().cloned() {
        results.push(task(item).await);
    }
    results
}

/// What a [`StubHttpClient`] answers a request with.
pub type StubAnswer = dyn Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync;

/// An HTTP client that answers from a function rather than a network.
///
/// A screen driven from a recorded payload, a robot run that must not depend on
/// a server, a test that wants a specific failure: all of them want one thing,
/// which is to decide what a request answers. Written as a client each time,
/// that is the same dozen lines of body plumbing repeated — and each copy is a
/// chance to answer differently from the real client.
///
/// ```ignore
/// let client: HttpClientRef = Arc::new(StubHttpClient::from_text(move |url| {
///     Ok(fixture_for(url))
/// }));
/// ```
pub struct StubHttpClient {
    answer: Box<StubAnswer>,
}

impl StubHttpClient {
    /// Answers from a function of the whole request.
    pub fn new(
        answer: impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            answer: Box::new(answer),
        }
    }

    /// Answers every request with `body` and a `200`.
    pub fn with_body(body: impl Into<Vec<u8>>) -> Self {
        let body = body.into();
        Self::new(move |request| {
            Ok(HttpResponse::new(
                request.url.clone(),
                200,
                Arc::new(BytesBody::new(body.clone())),
            ))
        })
    }

    /// Answers with text, or an error, from a function of the URL.
    pub fn from_text(
        answer: impl Fn(&str) -> Result<String, HttpError> + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |request| {
            let body = answer(&request.url)?;
            Ok(HttpResponse::new(
                request.url.clone(),
                200,
                Arc::new(BytesBody::new(body)),
            ))
        })
    }
}

impl HttpClient for StubHttpClient {
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
        _control: HttpControl,
    ) -> HttpFuture<'a, HttpResponse> {
        Box::pin(async move { (self.answer)(request) })
    }
}

/// How much of a body is read at a time.
///
/// Large enough that a fast connection is not held up by per-chunk bookkeeping,
/// small enough that progress moves visibly and a cancellation is noticed
/// promptly.
#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
const CHUNK_LEN: usize = 64 * 1024;

struct DefaultHttpClient {
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    native_client: Result<reqwest::blocking::Client, HttpError>,
}

impl DefaultHttpClient {
    fn new() -> Self {
        Self {
            #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
            native_client: build_native_client(),
        }
    }
}

/// The status line and headers, handed back the moment they arrive so a caller
/// can start reading the body while the rest of it is still on the wire.
#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
struct ResponseHead {
    status: u16,
    headers: Vec<(String, String)>,
    content_length: Option<u64>,
    resumed: bool,
}

/// A body read by a worker thread and delivered through a waker.
#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
struct ChannelBody {
    chunks: crate::async_io::ChunkStream<HttpError>,
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
impl HttpBody for ChannelBody {
    fn read_chunk(&self) -> HttpFuture<'_, Option<Vec<u8>>> {
        Box::pin(self.chunks.next())
    }
}

/// Runs one request on a thread of its own.
///
/// The platform's HTTP is synchronous, and the framework has no thread pool to
/// hide that behind: the alternative to a thread is blocking whichever task
/// polled the future, which on the UI thread is the frame. The thread reads the
/// status line, hands it back through a [`Signal`](crate::async_io::Signal), and
/// then keeps reading the body into a bounded channel — so the connection stays
/// busy while the reader works, and stops when the reader stops.
#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
async fn send_native(
    client: reqwest::blocking::Client,
    request: HttpRequest,
    control: HttpControl,
) -> Result<HttpResponse, HttpError> {
    use crate::async_io::{ChunkChannel, Signal};

    let head_signal: Signal<Result<ResponseHead, HttpError>> = Signal::new();
    let (chunks, stream) = ChunkChannel::<HttpError>::new();
    let worker_head = head_signal.clone();
    let url = request.url.clone();
    let worker_url = url.clone();

    std::thread::Builder::new()
        .name("cranpose-http".to_string())
        .spawn(move || {
            let outcome = read_native_response(&client, &request, &control, &chunks, &worker_head);
            match outcome {
                Ok(()) => chunks.finish(),
                Err(error) => {
                    // The head may not have been delivered yet: a connection
                    // that never opened has no status line, and whoever is
                    // awaiting one has to hear about it rather than wait.
                    worker_head.set(Err(error.clone()));
                    chunks.fail(error);
                }
            }
            let _ = worker_url;
        })
        .map_err(|error| HttpError::RequestFailed {
            url: url.clone(),
            message: format!("could not start the transfer: {error}"),
        })?;

    // Awaited, not blocked on: the whole reason the transfer runs on a thread of
    // its own is that whoever polled this future — often the frame — must stay
    // free while the connection opens.
    let head = head_signal
        .wait()
        .await
        .ok_or_else(|| HttpError::RequestFailed {
            url: url.clone(),
            message: "the transfer ended before a response arrived".to_string(),
        })??;

    Ok(
        HttpResponse::new(url, head.status, Arc::new(ChannelBody { chunks: stream }))
            .with_headers(head.headers)
            .with_content_length(head.content_length)
            .with_resumed(head.resumed),
    )
}

/// The worker body of [`send_native`]: sends the request, publishes the head,
/// then pumps the body.
#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
fn read_native_response(
    client: &reqwest::blocking::Client,
    request: &HttpRequest,
    control: &HttpControl,
    chunks: &crate::async_io::ChunkChannel<HttpError>,
    head_signal: &crate::async_io::Signal<Result<ResponseHead, HttpError>>,
) -> Result<(), HttpError> {
    use std::io::Read;

    if control.is_cancelled() {
        return Err(HttpError::Cancelled);
    }
    let method = match request.method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Head => reqwest::Method::HEAD,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Delete => reqwest::Method::DELETE,
    };
    let mut builder = client.request(method, &request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    if let Some(offset) = request.resume_from {
        builder = builder.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }

    let mut response = builder.send().map_err(|error| HttpError::RequestFailed {
        url: request.url.clone(),
        message: error.to_string(),
    })?;

    let status = response.status();
    let resumed = request.resume_from.is_some() && status == reqwest::StatusCode::PARTIAL_CONTENT;
    let content_length = response.content_length();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_ascii_lowercase(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    head_signal.set(Ok(ResponseHead {
        status: status.as_u16(),
        headers,
        content_length,
        resumed,
    }));

    let mut buffer = vec![0u8; CHUNK_LEN];
    let mut transferred = 0u64;
    loop {
        if control.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        let count = response
            .read(&mut buffer)
            .map_err(|error| HttpError::BodyReadFailed {
                url: request.url.clone(),
                message: error.to_string(),
            })?;
        if count == 0 {
            break;
        }
        transferred += count as u64;
        control.report(HttpProgress {
            transferred,
            total: content_length,
        });
        // A reader that has gone is a transfer nobody wants finished.
        if !chunks.push(buffer[..count].to_vec()) {
            break;
        }
    }
    Ok(())
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
async fn send_native(
    _request: HttpRequest,
    _control: HttpControl,
) -> Result<HttpResponse, HttpError> {
    Err(HttpError::UnsupportedFeature {
        operation: "native HTTP requests",
        feature: "http-native",
    })
}

impl HttpClient for DefaultHttpClient {
    fn send<'a>(
        &'a self,
        request: &'a HttpRequest,
        control: HttpControl,
    ) -> HttpFuture<'a, HttpResponse> {
        Box::pin(async move {
            #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
            {
                let client = self.native_client.as_ref().map_err(Clone::clone)?.clone();
                send_native(client, request.clone(), control).await
            }
            #[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
            {
                send_native(request.clone(), control).await
            }
            #[cfg(target_arch = "wasm32")]
            {
                send_web(request, control).await
            }
        })
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
fn build_native_client() -> Result<reqwest::blocking::Client, HttpError> {
    use std::time::Duration;

    configure_native_client_builder(
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            .timeout(None)
            .user_agent("cranpose/0.1"),
    )?
    .build()
    .map_err(|err| HttpError::ClientInit(err.to_string()))
}

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
fn configure_native_client_builder(
    builder: reqwest::blocking::ClientBuilder,
) -> Result<reqwest::blocking::ClientBuilder, HttpError> {
    #[cfg(target_os = "android")]
    {
        return Ok(builder.tls_certs_only(android_root_certificates()?));
    }

    #[cfg(not(target_os = "android"))]
    {
        Ok(builder)
    }
}

#[cfg(all(target_os = "android", feature = "http-native"))]
fn android_root_certificates() -> Result<Vec<reqwest::Certificate>, HttpError> {
    certificates_from_der_chain(
        webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .map(|certificate| certificate.as_ref()),
    )
}

#[cfg(any(
    all(test, not(target_arch = "wasm32"), feature = "http-native"),
    all(target_os = "android", feature = "http-native")
))]
fn certificates_from_der_chain<'a, I>(
    certificates: I,
) -> Result<Vec<reqwest::Certificate>, HttpError>
where
    I: IntoIterator<Item = &'a [u8]>,
{
    certificates
        .into_iter()
        .enumerate()
        .map(|(index, der)| {
            reqwest::Certificate::from_der(der).map_err(|err| {
                HttpError::ClientInit(format!(
                    "Failed to load TLS root certificate {index}: {err}"
                ))
            })
        })
        .collect()
}

/// A `fetch` body, read through the `ReadableStream` the browser hands back.
///
/// The browser streams a response, so the framework streams it too: a page that
/// downloads a hundred megabytes should not have to hold it as one `ArrayBuffer`
/// to hand it on in pieces.
#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
struct FetchBody {
    url: String,
    reader: web_sys::ReadableStreamDefaultReader,
    control: HttpControl,
    transferred: std::cell::Cell<u64>,
    total: Option<u64>,
}

#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
impl HttpBody for FetchBody {
    fn read_chunk(&self) -> HttpFuture<'_, Option<Vec<u8>>> {
        use wasm_bindgen_futures::JsFuture;

        Box::pin(async move {
            if self.control.is_cancelled() {
                let _ = self.reader.cancel();
                return Err(HttpError::Cancelled);
            }
            let result = JsFuture::from(self.reader.read()).await.map_err(|error| {
                HttpError::BodyReadFailed {
                    url: self.url.clone(),
                    message: format!("{error:?}"),
                }
            })?;
            let done = js_sys::Reflect::get(&result, &wasm_bindgen::JsValue::from_str("done"))
                .ok()
                .and_then(|value| value.as_bool())
                .unwrap_or(true);
            if done {
                return Ok(None);
            }
            let value = js_sys::Reflect::get(&result, &wasm_bindgen::JsValue::from_str("value"))
                .map_err(|error| HttpError::BodyReadFailed {
                    url: self.url.clone(),
                    message: format!("{error:?}"),
                })?;
            let chunk = js_sys::Uint8Array::new(&value).to_vec();
            self.transferred
                .set(self.transferred.get() + chunk.len() as u64);
            self.control.report(HttpProgress {
                transferred: self.transferred.get(),
                total: self.total,
            });
            Ok(Some(chunk))
        })
    }
}

#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
async fn send_web(request: &HttpRequest, control: HttpControl) -> Result<HttpResponse, HttpError> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Request, RequestInit, RequestMode, Response};

    if control.is_cancelled() {
        return Err(HttpError::Cancelled);
    }
    let options = RequestInit::new();
    options.set_method(request.method.name());
    options.set_mode(RequestMode::Cors);
    if let Some(body) = &request.body {
        options.set_body(&js_sys::Uint8Array::from(body.as_slice()).into());
    }

    let fetch_request =
        Request::new_with_str_and_init(&request.url, &options).map_err(|error| {
            HttpError::RequestFailed {
                url: request.url.clone(),
                message: format!("{error:?}"),
            }
        })?;
    let headers = fetch_request.headers();
    for (name, value) in &request.headers {
        headers
            .set(name, value)
            .map_err(|error| HttpError::RequestFailed {
                url: request.url.clone(),
                message: format!("{error:?}"),
            })?;
    }
    if let Some(offset) = request.resume_from {
        headers
            .set("Range", &format!("bytes={offset}-"))
            .map_err(|error| HttpError::RequestFailed {
                url: request.url.clone(),
                message: format!("{error:?}"),
            })?;
    }

    let window = web_sys::window().ok_or(HttpError::NoWindow)?;
    let value = JsFuture::from(window.fetch_with_request(&fetch_request))
        .await
        .map_err(|error| HttpError::RequestFailed {
            url: request.url.clone(),
            message: format!("{error:?}"),
        })?;
    let response: Response = value.dyn_into().map_err(|_| HttpError::InvalidResponse {
        url: request.url.clone(),
        message: "the browser answered with something that is not a Response".to_string(),
    })?;

    let status = response.status();
    let mut header_pairs = Vec::new();
    let entries = js_sys::try_iter(&response.headers()).ok().flatten();
    if let Some(entries) = entries {
        for entry in entries.flatten() {
            let pair = js_sys::Array::from(&entry);
            if pair.length() >= 2 {
                let name = pair.get(0).as_string().unwrap_or_default();
                let value = pair.get(1).as_string().unwrap_or_default();
                header_pairs.push((name.to_ascii_lowercase(), value));
            }
        }
    }
    let total = header_pairs
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<u64>().ok());
    let resumed = request.resume_from.is_some() && status == 206;

    let body = response.body().ok_or_else(|| HttpError::InvalidResponse {
        url: request.url.clone(),
        message: "the response carries no body".to_string(),
    })?;
    let reader: web_sys::ReadableStreamDefaultReader =
        body.get_reader()
            .dyn_into()
            .map_err(|_| HttpError::InvalidResponse {
                url: request.url.clone(),
                message: "the response body cannot be read in chunks".to_string(),
            })?;

    Ok(HttpResponse::new(
        request.url.clone(),
        status,
        Arc::new(FetchBody {
            url: request.url.clone(),
            reader,
            control,
            transferred: std::cell::Cell::new(0),
            total,
        }),
    )
    .with_headers(header_pairs)
    .with_content_length(total)
    .with_resumed(resumed))
}

#[cfg(all(target_arch = "wasm32", not(feature = "web-http")))]
async fn send_web(request: &HttpRequest, _control: HttpControl) -> Result<HttpResponse, HttpError> {
    let _ = request;
    Err(HttpError::UnsupportedFeature {
        operation: "web HTTP requests",
        feature: "web-http",
    })
}

pub fn default_http_client() -> HttpClientRef {
    Arc::new(DefaultHttpClient::new())
}

pub fn local_http_client() -> CompositionLocal<HttpClientRef> {
    thread_local! {
        static LOCAL_HTTP_CLIENT: std::cell::RefCell<Option<CompositionLocal<HttpClientRef>>> = const { std::cell::RefCell::new(None) };
    }

    LOCAL_HTTP_CLIENT.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_http_client, Arc::ptr_eq))
            .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use cranpose_core::CompositionLocalProvider;
    use std::cell::RefCell;
    use std::rc::Rc;
    #[cfg(not(target_arch = "wasm32"))]
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn a_response_carries_what_a_resumed_download_needs_to_know() {
        // A range request that the server honoured answers `206` with the
        // length of what is left, not of the whole file, and says it resumed —
        // which is how a caller knows to append rather than truncate.
        let response = HttpResponse::empty("https://host/big.bin", 206)
            .with_content_length(Some(4_096))
            .with_resumed(true);
        assert_eq!(response.content_length, Some(4_096));
        assert!(response.resumed);
        // 206 Partial Content is a success — a caller that only accepted 200
        // would treat every resumed download as a failure.
        assert!(response.is_success());

        // A server that ignored the range restarts the transfer, and a caller
        // that appended to its part file would corrupt it.
        let restarted = HttpResponse::empty("https://host/big.bin", 200)
            .with_content_length(None)
            .with_resumed(false);
        assert_eq!(restarted.content_length, None);
        assert!(!restarted.resumed);
        assert!(restarted.is_success());
    }

    #[test]
    fn the_stub_client_answers_every_url_with_the_same_body() {
        // What a test that only cares about the caller uses: no route table,
        // no per-URL closure, just "whatever you ask for, here is this".
        let client = StubHttpClient::with_body(b"pong".to_vec());
        for url in ["https://host/a", "https://host/b?query=1"] {
            let request = HttpRequest::get(url);
            let response = pollster::block_on(client.send(&request, HttpControl::new()))
                .expect("the stub answers every request");
            assert_eq!(response.status, 200);
            assert!(response.is_success());
            assert_eq!(response.url, url);
            let body = pollster::block_on(response.read_all()).expect("body");
            assert_eq!(body.as_slice(), b"pong");
        }
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    use std::thread;

    struct TestHttpClient;

    impl HttpClient for TestHttpClient {
        fn send<'a>(
            &'a self,
            request: &'a HttpRequest,
            _control: HttpControl,
        ) -> HttpFuture<'a, HttpResponse> {
            Box::pin(async move {
                Ok(HttpResponse::new(
                    request.url.clone(),
                    200,
                    Arc::new(BytesBody::new("ok")),
                ))
            })
        }
    }

    #[test]
    fn a_control_shares_cancellation_and_reports_progress() {
        let reported = Arc::new(AtomicU64::new(0));
        let recorder = Arc::clone(&reported);
        let control = HttpControl::new().with_progress(move |progress| {
            assert_eq!(progress.total, Some(20));
            recorder.store(progress.transferred, Ordering::Release);
        });
        let clone = control.clone();
        control.report(HttpProgress {
            transferred: 12,
            total: Some(20),
        });
        assert_eq!(reported.load(Ordering::Acquire), 12);
        assert!(!clone.is_cancelled());
        control.cancel();
        assert!(
            clone.is_cancelled(),
            "a control handed to a transfer must see the cancellation the caller made"
        );
    }

    #[test]
    fn progress_reports_a_fraction_only_when_the_server_said_how_large_it_is() {
        assert_eq!(
            HttpProgress {
                transferred: 5,
                total: Some(20)
            }
            .fraction(),
            Some(0.25)
        );
        assert_eq!(
            HttpProgress {
                transferred: 5,
                total: None
            }
            .fraction(),
            None,
            "a chunked response has no total, and inventing one is lying about it"
        );
        assert_eq!(
            HttpProgress {
                transferred: 5,
                total: Some(0)
            }
            .fraction(),
            None
        );
        assert!(HttpProgress {
            transferred: 20,
            total: Some(20)
        }
        .is_complete());
    }

    #[test]
    fn a_request_carries_what_it_asks_for() {
        let request = HttpRequest::get("https://example.test/thing")
            .method(HttpMethod::Post)
            .header("Accept", "application/json")
            .body(b"payload".to_vec())
            .resume_from(4096);
        assert_eq!(request.method.name(), "POST");
        assert_eq!(
            request.headers,
            vec![("Accept".to_string(), "application/json".to_string())]
        );
        assert_eq!(request.body.as_deref(), Some(b"payload".as_slice()));
        assert_eq!(request.resume_from, Some(4096));
        assert_eq!(
            HttpRequest::get("https://example.test/thing")
                .resume_from(0)
                .resume_from,
            None,
            "resuming from the beginning is not resuming"
        );
    }

    #[test]
    fn a_response_reads_its_headers_without_regard_to_case() {
        let response = HttpResponse::empty("https://example.test", 200).with_headers(vec![
            ("content-type".to_string(), "text/plain".to_string()),
            ("etag".to_string(), "\"abc\"".to_string()),
        ]);
        assert_eq!(response.header("Content-Type"), Some("text/plain"));
        assert_eq!(response.header("ETAG"), Some("\"abc\""));
        assert_eq!(response.header("missing"), None);
        assert!(response.is_success());
    }

    #[test]
    fn a_failing_status_is_an_error_the_caller_can_stop_on() {
        let response = HttpResponse::empty("https://example.test", 404);
        assert!(!response.is_success());
        let error = response
            .error_for_status()
            .err()
            .expect("404 is not success");
        assert!(matches!(error, HttpError::HttpStatus { status: 404, .. }));
    }

    /// A body arrives in pieces whether it came off a socket or out of memory,
    /// and reading it whole must give the same bytes either way.
    #[test]
    fn a_body_read_in_chunks_reassembles_to_what_was_sent() {
        let response = HttpResponse::new(
            "https://example.test",
            200,
            Arc::new(BytesBody::chunked("cranpose streams bodies", 4)),
        );
        let mut chunks = Vec::new();
        while let Some(chunk) = pollster::block_on(response.read_chunk()).expect("a chunk") {
            assert!(chunk.len() <= 4, "a chunked body honours its chunk size");
            chunks.push(chunk);
        }
        assert!(chunks.len() > 1, "the body arrived in pieces");
        let joined = chunks.concat();
        assert_eq!(
            String::from_utf8(joined).expect("text"),
            "cranpose streams bodies"
        );
    }

    #[test]
    fn reading_a_body_as_text_rejects_bytes_that_are_not_text() {
        let response = HttpResponse::new(
            "https://example.test",
            200,
            Arc::new(BytesBody::new(vec![0xff, 0xfe])),
        );
        assert!(matches!(
            pollster::block_on(response.read_text()),
            Err(HttpError::InvalidResponse { .. })
        ));
    }

    #[test]
    fn an_empty_body_ends_immediately() {
        let response = HttpResponse::empty("https://example.test", 204);
        assert_eq!(
            pollster::block_on(response.read_all()).expect("an empty body reads"),
            Vec::<u8>::new()
        );
    }

    #[test]
    fn default_http_client_is_available() {
        let client = default_http_client();
        let cloned = client.clone();
        assert_eq!(Arc::strong_count(&client), 2);
        drop(cloned);
        assert_eq!(Arc::strong_count(&client), 1);
    }

    /// The point of running a transfer on a thread of its own is that whoever
    /// polled the future stays free. Blocking on the answer instead puts the
    /// wait back on the caller's thread — which on the UI thread is the frame —
    /// and the thread buys nothing.
    #[test]
    fn the_native_transfer_awaits_its_response_rather_than_blocking_on_it() {
        let source = include_str!("http.rs");
        let send = source
            .split("async fn send_native(")
            .nth(1)
            .expect("the native send");
        let body = send.split("\nasync fn ").next().unwrap_or(send);
        assert!(
            body.contains("head_signal\n        .wait()\n        .await"),
            "the native send must await the response head"
        );
        let blocking = ["pollster", "::", "block_on"].concat();
        assert!(
            !body.contains(&blocking),
            "the native send must not block the thread that polled it"
        );
    }

    #[test]
    fn default_http_client_has_no_process_global_native_client_cache() {
        let source = include_str!("http.rs");
        let once_lock = ["Once", "Lock"].concat();
        let static_client = ["static ", "CLIENT"].concat();
        let native_client_fn = ["fn ", "native_client()"].concat();

        assert!(
            !source.contains(&static_client)
                && !source.contains(&native_client_fn)
                && !source.contains(&once_lock),
            "native HTTP client state must be owned by DefaultHttpClient instead of a process-global cache"
        );
    }

    /// Text, bytes and a file on disk are all one transfer plus what the caller
    /// does with the body, so a backend that implements `send` gets the rest
    /// and cannot make them disagree.
    #[test]
    fn every_convenience_reads_the_body_the_backend_produced() {
        let client = TestHttpClient;
        assert_eq!(
            pollster::block_on(client.get_bytes("https://example.com")).expect("bytes"),
            b"ok".to_vec()
        );
        assert_eq!(
            pollster::block_on(client.get_text("https://example.com")).expect("text"),
            "ok"
        );
    }

    #[test]
    fn map_ordered_concurrent_preserves_input_order() {
        let inputs = [3usize, 1, 4, 1, 5];
        let outputs = pollster::block_on(map_ordered_concurrent(&inputs, 2, |value| async move {
            value * 10
        }))
        .expect("ordered concurrent mapping");

        assert_eq!(outputs, vec![30, 10, 40, 10, 50]);
    }

    #[test]
    fn native_ordered_concurrency_is_not_tied_to_native_http_client_feature() {
        let source = include_str!("http.rs");
        assert!(
            source.contains(
                "#[cfg(not(target_arch = \"wasm32\"))]\npub async fn map_ordered_concurrent"
            ),
            "native ordered concurrency must stay available without the HTTP client feature"
        );
        assert!(
            !source.contains(
                "all(not(target_arch = \"wasm32\"), feature = \"http-native\")]\npub async fn map_ordered_concurrent"
            ) && !source.contains(
                "all(not(target_arch = \"wasm32\"), not(feature = \"http-native\"))]\npub async fn map_ordered_concurrent"
            ),
            "native ordered concurrency must not split into HTTP-enabled and HTTP-disabled behavior"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn map_ordered_concurrent_reports_worker_panic() {
        let inputs = [1usize];
        let should_panic = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let should_panic_for_task = Arc::clone(&should_panic);

        let error = pollster::block_on(map_ordered_concurrent(&inputs, 1, move |_| {
            let should_panic = Arc::clone(&should_panic_for_task);
            async move {
                if should_panic.load(std::sync::atomic::Ordering::SeqCst) {
                    panic!("test worker panic");
                }
                1usize
            }
        }))
        .expect_err("worker panic should be reported");

        assert!(matches!(error, HttpError::WorkerPanicked { .. }));
    }

    #[test]
    fn local_http_client_can_be_overridden() {
        let local = local_http_client();
        let default_client = default_http_client();
        let custom_client: HttpClientRef = Arc::new(TestHttpClient);
        let captured = Rc::new(RefCell::new(None));

        {
            let captured_for_closure = Rc::clone(&captured);
            let custom_client = custom_client.clone();
            let local_for_provider = local.clone();
            let local_for_read = local.clone();
            run_test_composition(move || {
                let captured = Rc::clone(&captured_for_closure);
                let local_for_read = local_for_read.clone();
                CompositionLocalProvider(
                    vec![local_for_provider.provides(custom_client.clone())],
                    move || {
                        let current = local_for_read.current();
                        *captured.borrow_mut() = Some(current);
                    },
                );
            });
        }

        let current = captured.borrow().as_ref().expect("client captured").clone();
        assert!(Arc::ptr_eq(&current, &custom_client));
        assert!(!Arc::ptr_eq(&current, &default_client));
    }

    #[cfg(all(not(target_arch = "wasm32"), not(feature = "http-native")))]
    #[test]
    fn default_http_client_reports_disabled_native_http_feature() {
        let error = pollster::block_on(default_http_client().get_text("https://example.com"))
            .expect_err("native HTTP should be feature-gated");

        assert!(matches!(
            error,
            HttpError::UnsupportedFeature {
                feature: "http-native",
                ..
            }
        ));
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn native_http_client_builds() {
        build_native_client().expect("native HTTP client should initialize");
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn certificates_from_der_chain_accepts_valid_roots() {
        let certificates = certificates_from_der_chain(
            webpki_root_certs::TLS_SERVER_ROOT_CERTS
                .iter()
                .take(3)
                .map(|certificate| certificate.as_ref()),
        )
        .expect("root certificates should parse");

        assert_eq!(certificates.len(), 3);
    }

    /// A local server that answers one request, so the native transfer can be
    /// exercised without reaching the network.
    ///
    /// `body_len` bytes are written in `chunk` pieces, with `delay` between
    /// them, which is what makes streaming, progress and cancellation
    /// observable rather than instantaneous.
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    fn local_server(
        body: Vec<u8>,
        chunk: usize,
        delay: std::time::Duration,
        supports_range: bool,
    ) -> Option<(String, thread::JoinHandle<()>)> {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping local HTTP server bind in restricted environment: {error}");
                return None;
            }
            Err(error) => panic!("bind local test server: {error}"),
        };
        let address = listener.local_addr().expect("local test server address");
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local test request");
            let mut request = [0u8; 2048];
            let read = stream.read(&mut request).expect("read local test request");
            let request = String::from_utf8_lossy(&request[..read]).to_string();

            let range_from = supports_range
                .then(|| {
                    request
                        .lines()
                        .find(|line| line.to_ascii_lowercase().starts_with("range:"))
                        .and_then(|line| line.split("bytes=").nth(1))
                        .and_then(|value| value.trim_end_matches('-').trim().parse::<u64>().ok())
                })
                .flatten();

            let payload = match range_from {
                Some(offset) if (offset as usize) < body.len() => &body[offset as usize..],
                Some(_) => &body[body.len()..],
                None => &body[..],
            };
            let status = if range_from.is_some() {
                "206 Partial Content"
            } else {
                "200 OK"
            };
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            )
            .expect("write local test response head");
            for piece in payload.chunks(chunk.max(1)) {
                if stream.write_all(piece).is_err() {
                    // The client hung up: a cancelled transfer, which is the
                    // behaviour under test rather than a failure.
                    return;
                }
                let _ = stream.flush();
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
            }
        });
        Some((format!("http://{address}"), handle))
    }

    /// A body larger than one chunk must arrive in pieces rather than being
    /// buffered whole, which is the whole point of streaming it.
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn a_native_body_arrives_in_pieces() {
        let body = vec![b'x'; 200 * 1024];
        let Some((url, server)) = local_server(
            body.clone(),
            16 * 1024,
            std::time::Duration::from_millis(5),
            false,
        ) else {
            return;
        };

        let client = default_http_client();
        let response = pollster::block_on(client.send(&HttpRequest::get(&url), HttpControl::new()))
            .expect("a response");
        assert!(response.is_success());
        assert_eq!(response.content_length, Some(body.len() as u64));

        let mut received = Vec::new();
        let mut chunks = 0usize;
        while let Some(chunk) = pollster::block_on(response.read_chunk()).expect("a chunk") {
            chunks += 1;
            received.extend_from_slice(&chunk);
        }
        server.join().expect("the local server finishes");
        assert_eq!(received.len(), body.len());
        assert!(
            chunks > 1,
            "a 200 KiB body must arrive in more than one piece, saw {chunks}"
        );
    }

    /// Progress must reach the caller while the transfer runs, not once at the
    /// end — a progress bar that fills in one step is not a progress bar.
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn a_native_transfer_reports_progress_as_it_runs() {
        let body = vec![b'y'; 200 * 1024];
        let Some((url, server)) = local_server(
            body.clone(),
            16 * 1024,
            std::time::Duration::from_millis(2),
            false,
        ) else {
            return;
        };

        let reports = Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = Arc::clone(&reports);
        let control = HttpControl::new().with_progress(move |progress| {
            recorder
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(progress);
        });

        let client = default_http_client();
        let response =
            pollster::block_on(client.send(&HttpRequest::get(&url), control)).expect("a response");
        let received = pollster::block_on(response.read_all()).expect("the body");
        server.join().expect("the local server finishes");

        assert_eq!(received.len(), body.len());
        let reports = reports
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        assert!(
            reports.len() > 1,
            "progress must move more than once over a 200 KiB transfer, saw {}",
            reports.len()
        );
        assert!(
            reports
                .windows(2)
                .all(|pair| pair[1].transferred >= pair[0].transferred),
            "progress must not go backwards: {reports:?}"
        );
        assert_eq!(
            reports.last().map(|progress| progress.transferred),
            Some(body.len() as u64)
        );
    }

    /// A cancelled transfer stops rather than running to completion in the
    /// background, and says so.
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn a_cancelled_native_transfer_stops() {
        let body = vec![b'z'; 512 * 1024];
        let Some((url, server)) =
            local_server(body, 8 * 1024, std::time::Duration::from_millis(5), false)
        else {
            return;
        };

        let control = HttpControl::new();
        let client = default_http_client();
        let response = pollster::block_on(client.send(&HttpRequest::get(&url), control.clone()))
            .expect("a response");

        // Read a little, then stop.
        let first = pollster::block_on(response.read_chunk()).expect("a chunk");
        assert!(first.is_some());
        control.cancel();

        let mut ended = false;
        for _ in 0..64 {
            match pollster::block_on(response.read_chunk()) {
                Ok(Some(_)) => continue,
                Ok(None) => {
                    ended = true;
                    break;
                }
                Err(HttpError::Cancelled) => {
                    ended = true;
                    break;
                }
                Err(other) => panic!("unexpected error after cancelling: {other}"),
            }
        }
        drop(response);
        let _ = server.join();
        assert!(ended, "a cancelled transfer must end rather than run on");
    }

    /// A download resumes from what is already on disk, and a server that
    /// ignores the range restarts the file rather than appending to it.
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn a_download_resumes_from_what_is_already_on_disk() {
        let body: Vec<u8> = (0..4096u32).map(|value| value as u8).collect();
        let directory = crate::test_scratch_dir("http-resume");
        let target = directory.join("payload.bin");
        let _ = std::fs::remove_file(&target);
        std::fs::write(&target, &body[..1024]).expect("a partial file");

        let Some((url, server)) = local_server(body.clone(), 1024, std::time::Duration::ZERO, true)
        else {
            return;
        };
        let client = default_http_client();
        let written = pollster::block_on(client.download_to(&url, &target, HttpControl::new()))
            .expect("the download finishes");
        server.join().expect("the local server finishes");

        assert_eq!(written, body.len() as u64);
        assert_eq!(std::fs::read(&target).expect("the file"), body);
        let _ = std::fs::remove_file(&target);
    }

    /// A server free to ignore a `Range` header answers with the whole body.
    /// Appending that to a partial file gives a file of the right length and
    /// the wrong bytes, so the partial file has to be replaced instead.
    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn a_server_that_ignores_a_range_restarts_the_file_rather_than_appending() {
        let body: Vec<u8> = (0..4096u32).map(|value| value as u8).collect();
        let directory = crate::test_scratch_dir("http-restart");
        let target = directory.join("payload.bin");
        let _ = std::fs::remove_file(&target);
        std::fs::write(&target, &body[..1024]).expect("a partial file");

        let Some((url, server)) =
            local_server(body.clone(), 1024, std::time::Duration::ZERO, false)
        else {
            return;
        };
        let client = default_http_client();
        let written = pollster::block_on(client.download_to(&url, &target, HttpControl::new()))
            .expect("the download finishes");
        server.join().expect("the local server finishes");

        assert_eq!(written, body.len() as u64);
        assert_eq!(
            std::fs::read(&target).expect("the file"),
            body,
            "the partial file must be replaced, not appended to"
        );
        let _ = std::fs::remove_file(&target);
    }

    #[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
    #[test]
    fn default_http_client_fetches_text_from_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = match TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
                eprintln!("skipping local HTTP server bind in restricted test environment: {err}");
                return;
            }
            Err(err) => panic!("bind local test server: {err}"),
        };
        let address = listener
            .local_addr()
            .expect("read local test server address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept local test request");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).expect("read local test request");
            let body = "cranpose-http-test";
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write local test response");
        });

        let url = format!("http://{address}");
        let text = pollster::block_on(default_http_client().get_text(&url))
            .expect("fetch text from local test server");
        server.join().expect("join local test server");

        assert_eq!(text, "cranpose-http-test");
    }
}
