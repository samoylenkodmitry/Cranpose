#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use cranpose_core::{CompositionLocal, compositionLocalOfWithPolicy};
#[cfg(all(target_arch = "wasm32", feature = "web-http"))]
use futures_util::{StreamExt, stream};

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
///
/// A browser's body is a `ReadableStream` reader that belongs to the one
/// thread a page has and cannot claim otherwise, so it is never `Send + Sync`
/// there; `Rc` avoids paying for synchronisation the target cannot use.
#[cfg(target_arch = "wasm32")]
pub type HttpBodyRef = std::rc::Rc<dyn HttpBody>;

/// Wraps `body` in an [`HttpBodyRef`].
///
/// A trait-object alias cannot expose its own `new`, so this is the one place
/// that picks `Arc` on native and `Rc` on wasm; callers that need an
/// [`HttpBodyRef`] from a concrete body go through this instead of repeating
/// that choice.
#[cfg(not(target_arch = "wasm32"))]
pub fn http_body_ref<B: HttpBody + Send + Sync + 'static>(body: B) -> HttpBodyRef {
    Arc::new(body)
}

/// See the native definition of [`http_body_ref`] for why this is `Rc` on wasm.
#[cfg(target_arch = "wasm32")]
pub fn http_body_ref<B: HttpBody + 'static>(body: B) -> HttpBodyRef {
    std::rc::Rc::new(body)
}

struct EmptyBody;

impl HttpBody for EmptyBody {
    fn read_chunk(&self) -> HttpFuture<'_, Option<Vec<u8>>> {
        Box::pin(async { Ok(None) })
    }
}

/// A body already in memory, handed out in `CHUNK_LEN`-sized pieces.
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
            let mut offset = self.offset.lock().unwrap_or_else(PoisonError::into_inner);
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
        Self::new(url, status, http_body_ref(EmptyBody))
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
    let existing = std::fs::metadata(target).map_or(0, |metadata| metadata.len());
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
                http_body_ref(BytesBody::new(body.clone())),
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
                http_body_ref(BytesBody::new(body)),
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

#[cfg(all(not(target_arch = "wasm32"), feature = "http-native"))]
struct ResponseHead {
    status: u16,
    headers: Vec<(String, String)>,
    content_length: Option<u64>,
    resumed: bool,
}

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

    let head = head_signal
        .wait()
        .await
        .ok_or_else(|| HttpError::RequestFailed {
            url: url.clone(),
            message: "the transfer ended before a response arrived".to_string(),
        })??;

    Ok(HttpResponse::new(
        url,
        head.status,
        http_body_ref(ChannelBody { chunks: stream }),
    )
    .with_headers(head.headers)
    .with_content_length(head.content_length)
    .with_resumed(head.resumed))
}

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
            .user_agent(concat!("cranpose/", env!("CARGO_PKG_VERSION"))),
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
        Ok(builder.tls_certs_only(android_root_certificates()?))
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
            .map(AsRef::as_ref),
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
        http_body_ref(FetchBody {
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
#[path = "tests/http_tests.rs"]
mod tests;
