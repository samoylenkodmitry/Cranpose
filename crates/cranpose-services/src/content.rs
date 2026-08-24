//! Streaming content: the single shape every byte source hands the application.
//!
//! File picking, incoming shares, document intents, dropped files and media
//! sources all resolve to a [`ContentHandle`]. A handle carries
//! [`ContentMetadata`] and opens a [`ContentReader`] that yields chunks, so a
//! multi-gigabyte media file and a two-byte clipboard drop travel the same API
//! and neither forces the application to buffer a whole payload.
//!
//! Folders resolve to a [`ContentFolderRef`]. Their trees are consumed through
//! [`folder_files`], which returns a [`ContentStream`]: an asynchronous stream
//! whose `next` future wakes its collector when the provider discovers another
//! file. Nothing here polls.

use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

/// Chunk size the framework's own readers hand back per [`ContentReader::read_chunk`].
pub const DEFAULT_CHUNK_LEN: usize = 256 * 1024;

/// A future produced by a content operation. It borrows the content it reads
/// from, so no implementation has to clone a handle into every call.
pub type ContentFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Errors produced while reading or writing content.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum ContentError {
    /// The content no longer exists at its source.
    #[error("content not found: {0}")]
    NotFound(String),
    /// The provider refused the read or write.
    #[error("content access denied: {0}")]
    PermissionDenied(String),
    /// Any other provider failure.
    #[error("content i/o failed: {0}")]
    Io(String),
    /// The operation is not offered by this platform or build.
    #[error("{0} is not supported on this platform")]
    Unsupported(&'static str),
}

/// Everything a provider can state about a piece of content without reading it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentMetadata {
    /// Display name, usually the last path or URI component.
    pub name: String,
    /// MIME type when the provider reports one.
    pub mime_type: Option<String>,
    /// Byte length when the provider reports one.
    pub len: Option<u64>,
    /// Last-modified time in milliseconds since the Unix epoch, when known.
    pub modified_millis: Option<u64>,
    /// Provider-scoped identifier: a path, a `content://` URI, a blob name.
    /// Not guaranteed to be openable outside its provider.
    pub identifier: String,
}

impl ContentMetadata {
    /// Metadata carrying only a display name.
    pub fn named(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            identifier: name.clone(),
            name,
            ..Self::default()
        }
    }

    /// Sets the MIME type.
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Sets the byte length.
    pub fn with_len(mut self, len: u64) -> Self {
        self.len = Some(len);
        self
    }

    /// Sets the last-modified time in milliseconds since the Unix epoch.
    pub fn with_modified_millis(mut self, modified_millis: u64) -> Self {
        self.modified_millis = Some(modified_millis);
        self
    }

    /// Sets the provider-scoped identifier.
    pub fn with_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.identifier = identifier.into();
        self
    }

    /// The lowercase extension of [`name`](Self::name), without the dot.
    pub fn extension(&self) -> Option<String> {
        let (_, extension) = self.name.rsplit_once('.')?;
        if extension.is_empty() {
            None
        } else {
            Some(extension.to_ascii_lowercase())
        }
    }
}

/// A pull reader over one piece of content.
///
/// Each [`read_chunk`](ContentReader::read_chunk) resolves to the next chunk of
/// bytes, or `None` once the content is exhausted. Chunk sizes are chosen by the
/// provider; framework readers use [`DEFAULT_CHUNK_LEN`].
pub trait ContentReader {
    /// Resolves to the next chunk, or `None` at end of content.
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>>;
}

/// Shared handle to a [`ContentReader`].
pub type ContentReaderRef = Rc<dyn ContentReader>;

/// One readable piece of content from any source.
pub trait Content {
    /// What the provider states about this content without reading it.
    fn metadata(&self) -> ContentMetadata;

    /// Opens a chunked reader over the bytes.
    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>>;

    /// Reads every byte. The default drains [`open`](Content::open); providers
    /// already holding the bytes override it to avoid the copy.
    fn read_all(&self) -> ContentFuture<'_, Result<Vec<u8>, ContentError>> {
        Box::pin(async move {
            let reader = self.open().await?;
            drain_reader(&reader).await
        })
    }
}

/// Shared handle to a [`Content`].
pub type ContentHandle = Rc<dyn Content>;

/// Reads a reader to its end.
pub async fn drain_reader(reader: &ContentReaderRef) -> Result<Vec<u8>, ContentError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = reader.read_chunk().await? {
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// A writable destination for streamed content.
pub trait ContentSink {
    /// Appends `bytes` to the destination.
    fn write_chunk(&self, bytes: Vec<u8>) -> ContentFuture<'_, Result<(), ContentError>>;

    /// Commits the destination. Dropping a sink without finishing it discards
    /// whatever the provider allows it to discard.
    fn finish(&self) -> ContentFuture<'_, Result<(), ContentError>>;
}

/// Shared handle to a [`ContentSink`].
pub type ContentSinkRef = Rc<dyn ContentSink>;

/// Writes every byte of `bytes` into `sink` and commits it.
pub async fn write_all(sink: &ContentSinkRef, bytes: Vec<u8>) -> Result<(), ContentError> {
    sink.write_chunk(bytes).await?;
    sink.finish().await
}

/// One child of a [`ContentFolder`].
pub enum ContentEntry {
    /// A readable file.
    File(ContentHandle),
    /// A nested folder.
    Folder(ContentFolderRef),
}

impl ContentEntry {
    /// The child's metadata regardless of kind.
    pub fn metadata(&self) -> ContentMetadata {
        match self {
            ContentEntry::File(file) => file.metadata(),
            ContentEntry::Folder(folder) => folder.metadata(),
        }
    }
}

/// A folder granted by a provider.
pub trait ContentFolder {
    /// What the provider states about this folder.
    fn metadata(&self) -> ContentMetadata;

    /// The immediate children.
    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>>;

    /// A provider-native stream over every file in the tree, when the provider
    /// discovers files incrementally. Providers that enumerate eagerly return
    /// `None` and [`folder_files`] walks them instead.
    fn stream_files(&self) -> Option<ContentStreamRef> {
        None
    }
}

/// Shared handle to a [`ContentFolder`].
pub type ContentFolderRef = Rc<dyn ContentFolder>;

/// An asynchronous stream of content.
///
/// [`next`](ContentStream::next) resolves once the next item is available; the
/// producer wakes the pending future, so collectors never poll.
pub trait ContentStream {
    /// Resolves to the next item, or `None` once the stream is exhausted.
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>>;

    /// How many items the provider has produced so far, when it counts them.
    fn produced(&self) -> Option<usize> {
        None
    }
}

/// Shared handle to a [`ContentStream`].
pub type ContentStreamRef = Rc<dyn ContentStream>;

/// Streams every file in `folder`'s tree, using the provider's native stream
/// when it has one and a depth-first walk otherwise.
pub fn folder_files(folder: ContentFolderRef) -> ContentStreamRef {
    folder
        .stream_files()
        .unwrap_or_else(|| Rc::new(WalkStream::new(folder)))
}

/// Collects an entire stream. Convenient for tests and small trees; production
/// collectors consume [`ContentStream::next`] incrementally.
pub async fn collect_stream(stream: &ContentStreamRef) -> Result<Vec<ContentHandle>, ContentError> {
    let mut items = Vec::new();
    while let Some(item) = stream.next().await? {
        items.push(item);
    }
    Ok(items)
}

/// Depth-first walk over a folder that enumerates eagerly.
struct WalkStream {
    pending: RefCell<Vec<ContentFolderRef>>,
    ready: RefCell<VecDeque<ContentHandle>>,
    produced: std::cell::Cell<usize>,
}

impl WalkStream {
    fn new(root: ContentFolderRef) -> Self {
        Self {
            pending: RefCell::new(vec![root]),
            ready: RefCell::new(VecDeque::new()),
            produced: std::cell::Cell::new(0),
        }
    }
}

impl ContentStream for WalkStream {
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>> {
        Box::pin(async move {
            loop {
                if let Some(file) = self.ready.borrow_mut().pop_front() {
                    self.produced.set(self.produced.get() + 1);
                    return Ok(Some(file));
                }
                let Some(folder) = self.pending.borrow_mut().pop() else {
                    return Ok(None);
                };
                for entry in folder.entries().await? {
                    match entry {
                        ContentEntry::File(file) => self.ready.borrow_mut().push_back(file),
                        ContentEntry::Folder(child) => self.pending.borrow_mut().push(child),
                    }
                }
            }
        })
    }

    fn produced(&self) -> Option<usize> {
        Some(self.produced.get())
    }
}

/// The producing half of a [`ContentStream`] fed by a platform callback.
///
/// A backend that receives items from outside the composition — an Android
/// enumeration callback, a drop target, a download — pushes into the channel and
/// the pending collector future is woken. This is the replacement for every
/// "drain what is ready each frame" contract.
pub struct ContentChannel {
    shared: Rc<ChannelShared>,
}

struct ChannelShared {
    ready: RefCell<VecDeque<ContentHandle>>,
    error: RefCell<Option<ContentError>>,
    closed: std::cell::Cell<bool>,
    produced: std::cell::Cell<usize>,
    waker: RefCell<Option<Waker>>,
}

impl ChannelShared {
    fn wake(&self) {
        if let Some(waker) = self.waker.borrow_mut().take() {
            waker.wake();
        }
    }
}

impl Default for ContentChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl ContentChannel {
    /// Creates an open channel.
    pub fn new() -> Self {
        Self {
            shared: Rc::new(ChannelShared {
                ready: RefCell::new(VecDeque::new()),
                error: RefCell::new(None),
                closed: std::cell::Cell::new(false),
                produced: std::cell::Cell::new(0),
                waker: RefCell::new(None),
            }),
        }
    }

    /// The consuming half, handed to the application.
    pub fn stream(&self) -> ContentStreamRef {
        Rc::new(ChannelStream {
            shared: Rc::clone(&self.shared),
        })
    }

    /// Publishes one item and wakes a pending collector.
    pub fn push(&self, content: ContentHandle) {
        if self.shared.closed.get() {
            return;
        }
        self.shared.ready.borrow_mut().push_back(content);
        self.shared.wake();
    }

    /// Ends the stream with an error and wakes a pending collector.
    pub fn fail(&self, error: ContentError) {
        if self.shared.closed.get() {
            return;
        }
        *self.shared.error.borrow_mut() = Some(error);
        self.shared.closed.set(true);
        self.shared.wake();
    }

    /// Ends the stream normally and wakes a pending collector.
    pub fn close(&self) {
        if self.shared.closed.get() {
            return;
        }
        self.shared.closed.set(true);
        self.shared.wake();
    }

    /// Whether the stream has been ended.
    pub fn is_closed(&self) -> bool {
        self.shared.closed.get()
    }
}

struct ChannelStream {
    shared: Rc<ChannelShared>,
}

impl ContentStream for ChannelStream {
    fn next(&self) -> ContentFuture<'_, Result<Option<ContentHandle>, ContentError>> {
        Box::pin(ChannelNext {
            shared: Rc::clone(&self.shared),
        })
    }

    fn produced(&self) -> Option<usize> {
        Some(self.shared.produced.get())
    }
}

struct ChannelNext {
    shared: Rc<ChannelShared>,
}

impl Future for ChannelNext {
    type Output = Result<Option<ContentHandle>, ContentError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(item) = self.shared.ready.borrow_mut().pop_front() {
            self.shared.produced.set(self.shared.produced.get() + 1);
            return Poll::Ready(Ok(Some(item)));
        }
        if let Some(error) = self.shared.error.borrow_mut().take() {
            return Poll::Ready(Err(error));
        }
        if self.shared.closed.get() {
            return Poll::Ready(Ok(None));
        }
        *self.shared.waker.borrow_mut() = Some(context.waker().clone());
        Poll::Pending
    }
}

/// Content already held in memory: clipboard drops, web `File` payloads, tests.
pub struct BytesContent {
    metadata: ContentMetadata,
    bytes: Rc<Vec<u8>>,
}

impl BytesContent {
    /// Wraps `bytes` under `metadata`, filling in the byte length.
    pub fn new(metadata: ContentMetadata, bytes: Vec<u8>) -> Self {
        let len = bytes.len() as u64;
        Self {
            metadata: ContentMetadata {
                len: Some(len),
                ..metadata
            },
            bytes: Rc::new(bytes),
        }
    }

    /// Wraps `bytes` under a display name.
    pub fn named(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self::new(ContentMetadata::named(name), bytes)
    }

    /// A shared handle to this content.
    pub fn handle(self) -> ContentHandle {
        Rc::new(self)
    }
}

impl Content for BytesContent {
    fn metadata(&self) -> ContentMetadata {
        self.metadata.clone()
    }

    fn open(&self) -> ContentFuture<'_, Result<ContentReaderRef, ContentError>> {
        let bytes = Rc::clone(&self.bytes);
        Box::pin(async move {
            Ok(Rc::new(BytesReader {
                bytes,
                offset: std::cell::Cell::new(0),
            }) as ContentReaderRef)
        })
    }

    fn read_all(&self) -> ContentFuture<'_, Result<Vec<u8>, ContentError>> {
        let bytes = Rc::clone(&self.bytes);
        Box::pin(async move { Ok(bytes.as_ref().clone()) })
    }
}

struct BytesReader {
    bytes: Rc<Vec<u8>>,
    offset: std::cell::Cell<usize>,
}

impl ContentReader for BytesReader {
    fn read_chunk(&self) -> ContentFuture<'_, Result<Option<Vec<u8>>, ContentError>> {
        Box::pin(async move {
            let start = self.offset.get();
            if start >= self.bytes.len() {
                return Ok(None);
            }
            let end = (start + DEFAULT_CHUNK_LEN).min(self.bytes.len());
            self.offset.set(end);
            Ok(Some(self.bytes[start..end].to_vec()))
        })
    }
}

/// A folder whose children are already known.
pub struct ReadyFolder {
    metadata: ContentMetadata,
    entries: RefCell<Option<Vec<ContentEntry>>>,
}

impl ReadyFolder {
    /// Wraps already-enumerated `entries` under `metadata`.
    pub fn new(metadata: ContentMetadata, entries: Vec<ContentEntry>) -> Self {
        Self {
            metadata,
            entries: RefCell::new(Some(entries)),
        }
    }

    /// A shared handle to this folder.
    pub fn handle(self) -> ContentFolderRef {
        Rc::new(self)
    }
}

impl ContentFolder for ReadyFolder {
    fn metadata(&self) -> ContentMetadata {
        self.metadata.clone()
    }

    fn entries(&self) -> ContentFuture<'_, Result<Vec<ContentEntry>, ContentError>> {
        Box::pin(async move {
            self.entries
                .borrow_mut()
                .take()
                .ok_or_else(|| ContentError::Io("folder entries already consumed".into()))
        })
    }
}

/// Turns a provider URI into readable content.
///
/// Every source that names content by URI rather than handing over bytes —
/// a document intent, a shared item, a dropped file, a media source — resolves
/// through this, so applications never learn what a `content://`, `file://` or
/// blob URI means on the current platform.
pub trait ContentResolver {
    /// Resolves `uri`, or `None` when this platform cannot open it.
    fn resolve(&self, uri: &str) -> Option<ContentHandle>;
}

/// Shared handle to a [`ContentResolver`].
pub type ContentResolverRef = Rc<dyn ContentResolver>;

thread_local! {
    static PLATFORM_RESOLVER: RefCell<Option<ContentResolverRef>> = const { RefCell::new(None) };
}

/// Installs the platform content resolver (Android's `ContentResolver`, the
/// iOS document store, the browser's blob registry).
pub fn set_platform_content_resolver(resolver: ContentResolverRef) {
    PLATFORM_RESOLVER.with(|cell| *cell.borrow_mut() = Some(resolver));
}

/// Removes any installed platform content resolver.
pub fn clear_platform_content_resolver() {
    PLATFORM_RESOLVER.with(|cell| *cell.borrow_mut() = None);
}

/// Resolves `uri` through the platform resolver, falling back to the local
/// filesystem for `file://` URIs and plain paths.
pub fn resolve_content(uri: &str) -> Option<ContentHandle> {
    if let Some(resolver) = PLATFORM_RESOLVER.with(|cell| cell.borrow().clone()) {
        if let Some(content) = resolver.resolve(uri) {
            return Some(content);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = uri.strip_prefix("file://").unwrap_or(uri);
        if !path.contains("://") {
            return Some(file_content(path));
        }
    }
    let _ = uri;
    None
}

#[cfg(not(target_arch = "wasm32"))]
pub use file::{file_content, file_folder, FileContent, FileFolder, FileSink};

#[cfg(not(target_arch = "wasm32"))]
mod file;

/// Decodes the percent-escapes in a platform content URI, refusing input it
/// cannot decode exactly.
///
/// Android hands a folder or document back as an opaque `content://` URI whose
/// readable name is percent-encoded inside it, so anything that wants to show
/// the user which folder they picked has to decode it. The pair here mirrors
/// [`String::from_utf8`] and [`String::from_utf8_lossy`]: this one answers
/// `None` rather than inventing a character, and [`percent_decode_lossy`] is
/// the best-effort form for text that is only going to be displayed.
///
/// Prefer this when the result is used as an identity -- a key, a filename, a
/// fingerprint -- where a replacement character would silently make two
/// different inputs look the same.
pub fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().and_then(hex_value)?;
            let low = bytes.get(index + 2).copied().and_then(hex_value)?;
            out.push((high << 4) | low);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Decodes the percent-escapes in a platform content URI for display.
///
/// A byte sequence that is not valid UTF-8 becomes U+FFFD, and an escape that
/// is truncated or not hexadecimal is passed through as written rather than
/// discarded -- a name is more useful slightly wrong than absent. Use
/// [`percent_decode`] when the result carries identity rather than being shown.
pub fn percent_decode_lossy(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let pair = hex_value(bytes.get(index + 1).copied().unwrap_or(0))
                .zip(hex_value(bytes.get(index + 2).copied().unwrap_or(0)));
            if let Some((high, low)) = pair {
                out.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {

    /// The two decoders answered differently across the codebase before they
    /// were one pair: a strict one that refused what it could not decode and a
    /// lossy one that substituted. Both behaviours are wanted -- a display name
    /// should survive a bad byte, an identity must not -- so the split is
    /// deliberate and pinned here rather than left to whichever copy a caller
    /// happened to reach for.
    #[test]
    fn a_uri_that_cannot_be_decoded_exactly_is_refused_but_still_displays() {
        assert_eq!(
            percent_decode("Trip%20Photos").as_deref(),
            Some("Trip Photos")
        );
        assert_eq!(percent_decode_lossy("Trip%20Photos"), "Trip Photos");

        // %FF is a valid escape but not valid UTF-8 on its own.
        assert_eq!(percent_decode("bad%FFname"), None);
        assert_eq!(percent_decode_lossy("bad%FFname"), "bad\u{fffd}name");

        // A truncated escape is malformed input, not data.
        assert_eq!(percent_decode("cut%4"), None);
        assert_eq!(percent_decode_lossy("cut%4"), "cut%4");

        // A non-hexadecimal escape is passed through by the lossy form.
        assert_eq!(percent_decode("100%zz"), None);
        assert_eq!(percent_decode_lossy("100%zz"), "100%zz");
    }

    use super::*;

    fn block<T>(future: impl Future<Output = T>) -> T {
        pollster::block_on(future)
    }

    #[test]
    fn metadata_reports_the_lowercase_extension() {
        let metadata = ContentMetadata::named("Scan.PNG");
        assert_eq!(metadata.extension().as_deref(), Some("png"));
        assert_eq!(ContentMetadata::named("noext").extension(), None);
        assert_eq!(ContentMetadata::named("trailing.").extension(), None);
    }

    #[test]
    fn metadata_carries_what_a_provider_knew_about_the_item() {
        let metadata = ContentMetadata::named("Scan.png")
            .with_len(2_048)
            .with_modified_millis(1_700_000_000_000)
            .with_identifier("content://provider/17")
            .with_mime_type("image/png");

        assert_eq!(metadata.name, "Scan.png");
        assert_eq!(metadata.len, Some(2_048));
        assert_eq!(metadata.modified_millis, Some(1_700_000_000_000));
        assert_eq!(metadata.identifier, "content://provider/17");
        assert_eq!(metadata.mime_type.as_deref(), Some("image/png"));
        // A size or a timestamp a provider did not report stays unknown rather
        // than becoming zero; an item with no provider-scoped identifier is
        // identified by its name, which is the only handle there is on it.
        let bare = ContentMetadata::named("Scan.png");
        assert_eq!(bare.len, None);
        assert_eq!(bare.modified_millis, None);
        assert_eq!(bare.mime_type, None);
        assert_eq!(bare.identifier, "Scan.png");
    }

    #[test]
    fn a_channel_reports_when_it_has_ended_and_ignores_what_arrives_after() {
        let channel = ContentChannel::new();
        assert!(!channel.is_closed());

        channel.close();
        assert!(channel.is_closed());

        // A producer that has not noticed yet must not be able to reopen it.
        channel.push(BytesContent::named("late.bin", vec![1]).handle());
        channel.fail(ContentError::Unsupported("after close"));
        assert!(channel.is_closed());

        let stream = channel.stream();
        assert!(
            block(stream.next())
                .expect("a closed channel ends cleanly")
                .is_none(),
            "a channel closed before anything was pushed must yield nothing"
        );
    }

    #[test]
    fn bytes_content_streams_in_chunks_and_reads_whole() {
        let payload = vec![7u8; DEFAULT_CHUNK_LEN + 11];
        let content = BytesContent::named("blob.bin", payload.clone()).handle();
        assert_eq!(content.metadata().len, Some(payload.len() as u64));

        let chunks = block(async {
            let reader = content.open().await.unwrap();
            let mut sizes = Vec::new();
            while let Some(chunk) = reader.read_chunk().await.unwrap() {
                sizes.push(chunk.len());
            }
            sizes
        });
        assert_eq!(chunks, vec![DEFAULT_CHUNK_LEN, 11]);
        assert_eq!(block(content.read_all()).unwrap(), payload);
    }

    #[test]
    fn walking_a_tree_yields_every_file_depth_first() {
        let nested = ReadyFolder::new(
            ContentMetadata::named("nested"),
            vec![ContentEntry::File(
                BytesContent::named("b.txt", b"b".to_vec()).handle(),
            )],
        )
        .handle();
        let root = ReadyFolder::new(
            ContentMetadata::named("root"),
            vec![
                ContentEntry::File(BytesContent::named("a.txt", b"a".to_vec()).handle()),
                ContentEntry::Folder(nested),
            ],
        )
        .handle();

        let stream = folder_files(root);
        let files = block(collect_stream(&stream)).unwrap();
        let names: Vec<String> = files.iter().map(|file| file.metadata().name).collect();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
        assert_eq!(stream.produced(), Some(2));
    }

    #[test]
    fn a_channel_wakes_its_collector_instead_of_being_polled() {
        let channel = Rc::new(ContentChannel::new());
        let stream = channel.stream();

        let mut future = Box::pin(stream.next());
        let waker = Waker::noop().clone();
        let mut context = Context::from_waker(&waker);
        assert!(future.as_mut().poll(&mut context).is_pending());

        channel.push(BytesContent::named("late.txt", b"late".to_vec()).handle());
        let Poll::Ready(Ok(Some(item))) = future.as_mut().poll(&mut context) else {
            panic!("the pushed item should have completed the pending collector");
        };
        assert_eq!(item.metadata().name, "late.txt");

        channel.close();
        assert!(matches!(block(stream.next()), Ok(None)));
        assert_eq!(stream.produced(), Some(1));
    }

    #[test]
    fn a_failed_channel_reports_the_error_once_then_ends() {
        let channel = ContentChannel::new();
        let stream = channel.stream();
        channel.fail(ContentError::Io("provider died".into()));
        let Err(failure) = block(stream.next()) else {
            panic!("a failed channel reports its error to the collector");
        };
        assert_eq!(failure, ContentError::Io("provider died".into()));
        assert!(matches!(block(stream.next()), Ok(None)));
    }
}
