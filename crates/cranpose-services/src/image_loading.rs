//! Images loaded from the network, a file or bytes the app holds, decoded off
//! the UI thread at the size they are drawn, as Compose apps load them with
//! Coil's `AsyncImage`.
//!
//! [`rememberAsyncImage`] gives a composable the [`ImageLoadState`] of an
//! [`ImageRequest`]: `Loading` until the image is decoded, then `Ready` or
//! `Failed`. All images share one [`ImageLoader`]: a memory cache bounded by
//! the bytes of its decoded pixels, a disk cache of downloaded files with a
//! size limit, and one download for every request of one image. A request
//! whose composables all left the composition stops its download.
//!
//! The network goes through `local_http_client()`, so an app enables the
//! `http-native` (or `web-http`) feature as it does for its own requests.

use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
};

use cranpose_core::{LaunchedEffectAsync, State, mutableStateOf, rememberKeyed};
use cranpose_macros::composable;
use cranpose_ui_graphics::ImageBitmap;
use futures_util::future::{FutureExt, Shared, WeakShared};

#[cfg(not(target_arch = "wasm32"))]
use crate::async_io::Signal;
use crate::http::HttpClientRef;

/// The image to load, and the largest size in pixels it is drawn at.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageRequest {
    source: ImageSource,
    max_width: u32,
    max_height: u32,
}

#[derive(Clone, Debug)]
enum ImageSource {
    /// `http://`, `https://` or `file://`, or a file path.
    Url(Arc<str>),
    /// Encoded bytes the app read itself, from a picked file or a bundled
    /// asset, named by `key` for the memory cache.
    Bytes { key: Arc<str>, bytes: Arc<[u8]> },
}

impl ImageSource {
    /// What names the image: its URL, or the key of its bytes. Comparing
    /// and hashing the name keeps a lookup from reading the bytes.
    fn name(&self) -> (bool, &str) {
        match self {
            Self::Url(url) => (true, url),
            Self::Bytes { key, .. } => (false, key),
        }
    }
}

impl PartialEq for ImageSource {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for ImageSource {}

impl std::hash::Hash for ImageSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name().hash(state);
    }
}

impl ImageRequest {
    /// An image at `url`: `http://`, `https://`, `file://` or a file path.
    pub fn new(url: impl Into<Arc<str>>) -> Self {
        Self {
            source: ImageSource::Url(url.into()),
            max_width: u32::MAX,
            max_height: u32::MAX,
        }
    }

    /// An image in encoded `bytes` (PNG, JPEG, WebP or GIF) the app already
    /// holds. `key` names it in the memory cache; one key is one image.
    pub fn bytes(key: impl Into<Arc<str>>, bytes: impl Into<Arc<[u8]>>) -> Self {
        Self {
            source: ImageSource::Bytes {
                key: key.into(),
                bytes: bytes.into(),
            },
            max_width: u32::MAX,
            max_height: u32::MAX,
        }
    }

    /// Decodes the image no larger than `width` × `height` pixels, keeping
    /// its aspect ratio: a thumbnail of a large photo holds a thumbnail's
    /// memory. Pass the size it is drawn at, in device pixels.
    pub fn max_size(mut self, width: u32, height: u32) -> Self {
        self.max_width = width.max(1);
        self.max_height = height.max(1);
        self
    }

    /// [`max_size`](Self::max_size), unless the request already has one: a
    /// composable that knows the size it draws at fills it in, and an app
    /// that asked for a size keeps it.
    pub fn or_max_size(self, width: u32, height: u32) -> Self {
        if self.max_width == u32::MAX && self.max_height == u32::MAX {
            self.max_size(width, height)
        } else {
            self
        }
    }
}

/// Where a request is.
#[derive(Clone, Debug, PartialEq)]
pub enum ImageLoadState {
    Loading,
    Ready(ImageBitmap),
    Failed(ImageLoadError),
}

/// Why an image did not load.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImageLoadError {
    #[error("could not read the image: {0}")]
    Read(String),
    #[error("could not decode the image: {0}")]
    Decode(String),
}

/// How much the shared [`ImageLoader`] keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageCacheLimits {
    /// The bytes of decoded pixels the memory cache holds.
    pub memory_bytes: usize,
    /// The bytes of downloaded files the disk cache holds.
    pub disk_bytes: u64,
}

impl Default for ImageCacheLimits {
    fn default() -> Self {
        Self {
            memory_bytes: 64 << 20,
            disk_bytes: 128 << 20,
        }
    }
}

type LoadResult = Result<ImageBitmap, ImageLoadError>;

#[cfg(not(target_arch = "wasm32"))]
type LoadFuture = Pin<Box<dyn Future<Output = LoadResult> + Send>>;
#[cfg(target_arch = "wasm32")]
type LoadFuture = Pin<Box<dyn Future<Output = LoadResult>>>;

/// What a loader shares with its loads: the browser's futures run on its one
/// thread, so there they share through `Rc`.
#[cfg(not(target_arch = "wasm32"))]
type Handle<T> = Arc<T>;
#[cfg(target_arch = "wasm32")]
type Handle<T> = std::rc::Rc<T>;

/// Loads, decodes and caches images. A clone is the same loader;
/// [`image_loader`] is the one every [`rememberAsyncImage`] uses.
#[derive(Clone)]
pub struct ImageLoader {
    inner: Handle<LoaderState>,
}

struct LoaderState {
    memory: Mutex<MemoryCache>,
    disk: Option<Arc<DiskCache>>,
    in_flight: Mutex<HashMap<ImageRequest, WeakShared<LoadFuture>>>,
}

impl ImageLoader {
    /// A loader that keeps downloaded files in `disk_dir`, or none on disk
    /// when it is `None`.
    pub fn new(limits: ImageCacheLimits, disk_dir: Option<PathBuf>) -> Self {
        Self {
            inner: Handle::new(LoaderState {
                memory: Mutex::new(MemoryCache::new(limits.memory_bytes)),
                disk: disk_dir.map(|dir| Arc::new(DiskCache::new(dir, limits.disk_bytes))),
                in_flight: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The decoded image when the memory cache holds it.
    pub fn cached(&self, request: &ImageRequest) -> Option<ImageBitmap> {
        lock(&self.inner.memory).get(request)
    }

    /// Loads `request`, joining a load of the same request already under way.
    /// The load stops when every future it returned is dropped.
    pub fn load(
        &self,
        client: HttpClientRef,
        request: &ImageRequest,
    ) -> impl Future<Output = LoadResult> + use<> {
        let shared = self.shared_load(client, request);
        async move {
            match shared {
                Ok(bitmap) => Ok(bitmap),
                Err(load) => load.await,
            }
        }
    }

    /// The cached image, or the load every caller of `request` shares.
    fn shared_load(
        &self,
        client: HttpClientRef,
        request: &ImageRequest,
    ) -> Result<ImageBitmap, Shared<LoadFuture>> {
        if let Some(bitmap) = self.cached(request) {
            return Ok(bitmap);
        }
        let mut in_flight = lock(&self.inner.in_flight);
        if let Some(load) = in_flight.get(request).and_then(WeakShared::upgrade) {
            return Err(load);
        }
        let loader = self.clone();
        let owned = request.clone();
        let load: LoadFuture = Box::pin(async move {
            let result = loader.fetch(client, &owned).await;
            lock(&loader.inner.in_flight).remove(&owned);
            if let Ok(bitmap) = &result {
                lock(&loader.inner.memory).insert(owned, bitmap.clone());
            }
            result
        });
        let load = load.shared();
        if let Some(weak) = load.downgrade() {
            in_flight.insert(request.clone(), weak);
        }
        Err(load)
    }

    async fn fetch(&self, client: HttpClientRef, request: &ImageRequest) -> LoadResult {
        let (width, height) = (request.max_width, request.max_height);
        match &request.source {
            ImageSource::Bytes { bytes, .. } => {
                decode_off_thread(Encoded::Bytes(Arc::clone(bytes)), width, height).await
            }
            ImageSource::Url(url) if !is_remote(url) => {
                decode_off_thread(Encoded::File(local_path(url)), width, height).await
            }
            ImageSource::Url(url) => {
                if let Some(disk) = &self.inner.disk
                    && let Some(cached) = decode_off_thread(
                        Encoded::Cached(Arc::clone(disk), Arc::clone(url)),
                        width,
                        height,
                    )
                    .await
                    .ok()
                {
                    return Ok(cached);
                }
                let bytes: Arc<[u8]> = client
                    .get_bytes(url)
                    .await
                    .map_err(|error| ImageLoadError::Read(error.to_string()))?
                    .into();
                let encoded = match &self.inner.disk {
                    Some(disk) => Encoded::Download {
                        disk: Arc::clone(disk),
                        url: Arc::clone(url),
                        bytes,
                    },
                    None => Encoded::Bytes(bytes),
                };
                decode_off_thread(encoded, width, height).await
            }
        }
    }
}

/// The loader every [`rememberAsyncImage`] shares, with the default limits
/// and its disk cache in the application's cache folder.
#[cfg(not(target_arch = "wasm32"))]
pub fn image_loader() -> ImageLoader {
    static LOADER: std::sync::OnceLock<ImageLoader> = std::sync::OnceLock::new();
    LOADER.get_or_init(default_loader).clone()
}

/// The loader every [`rememberAsyncImage`] shares, with the default limits.
/// The browser has no disk cache for it; its own HTTP cache stands in.
#[cfg(target_arch = "wasm32")]
pub fn image_loader() -> ImageLoader {
    thread_local! {
        static LOADER: ImageLoader = default_loader();
    }
    LOADER.with(ImageLoader::clone)
}

fn default_loader() -> ImageLoader {
    let disk_dir = if cfg!(target_arch = "wasm32") {
        None
    } else {
        crate::host::application_directories()
            .ok()
            .map(|directories| directories.cache.join("images"))
    };
    ImageLoader::new(ImageCacheLimits::default(), disk_dir)
}

/// Loads `request` through [`image_loader`] while the caller stays in the
/// composition. The caller reads the returned state, so it recomposes when
/// the image is ready or failed. An image the memory cache holds is `Ready`
/// in the first composition.
///
/// ```ignore
/// match rememberAsyncImage(ImageRequest::new(url).max_size(240, 240)).value() {
///     ImageLoadState::Ready(bitmap) => Image(BitmapPainter(bitmap), ..),
///     ImageLoadState::Loading => Placeholder(),
///     ImageLoadState::Failed(_) => BrokenImage(),
/// }
/// ```
#[composable]
pub fn rememberAsyncImage(request: ImageRequest) -> State<ImageLoadState> {
    let loader = image_loader();
    let cached = loader.cached(&request);
    // Keyed: another request starts from its own cached image or `Loading`,
    // never from the image of the request before it.
    let state = rememberKeyed(request.clone(), move |_| {
        mutableStateOf(cached.map_or(ImageLoadState::Loading, ImageLoadState::Ready))
    });
    let client = crate::http::local_http_client().current();
    LaunchedEffectAsync(request.clone(), move |_| {
        Box::pin(async move {
            if let Some(bitmap) = loader.cached(&request) {
                state.set(ImageLoadState::Ready(bitmap));
                return;
            }
            state.set(ImageLoadState::Loading);
            state.set(match loader.load(client, &request).await {
                Ok(bitmap) => ImageLoadState::Ready(bitmap),
                Err(error) => ImageLoadState::Failed(error),
            });
        })
    });
    state.as_state()
}

fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn local_path(url: &str) -> PathBuf {
    PathBuf::from(url.strip_prefix("file://").unwrap_or(url))
}

/// Where the encoded image comes from, read on the decode worker.
enum Encoded {
    Bytes(Arc<[u8]>),
    File(PathBuf),
    /// The disk cache's copy, if it holds one.
    Cached(Arc<DiskCache>, Arc<str>),
    /// A download, which the worker also writes to the disk cache.
    Download {
        disk: Arc<DiskCache>,
        url: Arc<str>,
        bytes: Arc<[u8]>,
    },
}

impl Encoded {
    fn read(self) -> Result<Arc<[u8]>, ImageLoadError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::File(path) => std::fs::read(&path)
                .map(Into::into)
                .map_err(|error| ImageLoadError::Read(format!("{}: {error}", path.display()))),
            Self::Cached(disk, url) => disk
                .read(&url)
                .map(Into::into)
                .ok_or_else(|| ImageLoadError::Read(format!("{url} is not on disk"))),
            Self::Download { disk, url, bytes } => {
                disk.write(&url, &bytes);
                Ok(bytes)
            }
        }
    }
}

fn decode(encoded: Encoded, max_width: u32, max_height: u32) -> LoadResult {
    let bytes = encoded.read()?;
    let image = image::load_from_memory(&bytes)
        .map_err(|error| ImageLoadError::Decode(error.to_string()))?;
    let image = if image.width() > max_width || image.height() > max_height {
        image.resize(max_width, max_height, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let rgba = image.into_rgba8();
    let (width, height) = rgba.dimensions();
    ImageBitmap::from_rgba8(width, height, rgba.into_raw())
        .map_err(|error| ImageLoadError::Decode(error.to_string()))
}

/// Decodes on a worker thread, so a large image never holds a frame. The
/// browser has no threads to hand the work to.
#[cfg(not(target_arch = "wasm32"))]
async fn decode_off_thread(encoded: Encoded, max_width: u32, max_height: u32) -> LoadResult {
    let signal = Signal::new();
    let done = signal.clone();
    // Held while a caller waits: a job whose callers all left is skipped.
    let waiting = Arc::new(());
    let waiter = Arc::downgrade(&waiting);
    let job: DecodeJob = Box::new(move || {
        if waiter.strong_count() == 0 {
            done.close();
        } else {
            done.set(decode(encoded, max_width, max_height));
        }
    });
    if decode_queue().send(job).is_err() {
        return Err(ImageLoadError::Decode("the decode threads stopped".into()));
    }
    let result = signal.wait().await;
    drop(waiting);
    result.unwrap_or_else(|| {
        Err(ImageLoadError::Decode(
            "the decode thread dropped the job".into(),
        ))
    })
}

#[cfg(target_arch = "wasm32")]
async fn decode_off_thread(encoded: Encoded, max_width: u32, max_height: u32) -> LoadResult {
    decode(encoded, max_width, max_height)
}

#[cfg(not(target_arch = "wasm32"))]
type DecodeJob = Box<dyn FnOnce() + Send>;

/// The queue of the decode threads: as many as the machine has cores, up to
/// four, so a screen of thumbnails decodes side by side.
#[cfg(not(target_arch = "wasm32"))]
fn decode_queue() -> &'static std::sync::mpsc::Sender<DecodeJob> {
    static QUEUE: std::sync::OnceLock<std::sync::mpsc::Sender<DecodeJob>> =
        std::sync::OnceLock::new();
    QUEUE.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<DecodeJob>();
        let receiver = Arc::new(Mutex::new(receiver));
        let threads = std::thread::available_parallelism().map_or(1, |cores| cores.get().min(4));
        for index in 0..threads {
            let receiver = Arc::clone(&receiver);
            let started = std::thread::Builder::new()
                .name(format!("cranpose-image-{index}"))
                .spawn(move || {
                    while let Ok(job) = lock(&receiver).recv() {
                        job();
                    }
                });
            if let Err(error) = started {
                log::warn!("image loading: could not start a decode thread: {error}");
            }
        }
        sender
    })
}

/// Decoded images by request, the least recently used leaving first once
/// their pixels pass the limit.
struct MemoryCache {
    entries: HashMap<ImageRequest, (ImageBitmap, u64)>,
    bytes: usize,
    limit: usize,
    clock: u64,
}

impl MemoryCache {
    fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            limit,
            clock: 0,
        }
    }

    fn get(&mut self, request: &ImageRequest) -> Option<ImageBitmap> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(request).map(|(bitmap, used)| {
            *used = clock;
            bitmap.clone()
        })
    }

    fn insert(&mut self, request: ImageRequest, bitmap: ImageBitmap) {
        let size = pixel_bytes(&bitmap);
        if size > self.limit {
            return;
        }
        self.clock += 1;
        if let Some((old, _)) = self.entries.insert(request, (bitmap, self.clock)) {
            self.bytes -= pixel_bytes(&old);
        }
        self.bytes += size;
        while self.bytes > self.limit {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(request, _)| request.clone())
            else {
                break;
            };
            if let Some((evicted, _)) = self.entries.remove(&oldest) {
                self.bytes -= pixel_bytes(&evicted);
            }
        }
    }
}

fn pixel_bytes(bitmap: &ImageBitmap) -> usize {
    bitmap.width() as usize * bitmap.height() as usize * 4
}

/// Downloaded files, one per URL, named by the URL's hash and starting with
/// the URL itself, so two URLs with one hash never read as each other. Past
/// the limit the least recently read files go.
struct DiskCache {
    dir: PathBuf,
    limit: u64,
    /// The bytes the folder holds: scanned at the first write, then added up.
    size: Mutex<Option<u64>>,
}

impl DiskCache {
    fn new(dir: PathBuf, limit: u64) -> Self {
        Self {
            dir,
            limit,
            size: Mutex::new(None),
        }
    }

    fn path(&self, url: &str) -> PathBuf {
        self.dir.join(format!("{:016x}", fnv1a(url.as_bytes())))
    }

    fn read(&self, url: &str) -> Option<Vec<u8>> {
        let path = self.path(url);
        let mut contents = std::fs::read(&path).ok()?;
        let header = contents.iter().position(|&byte| byte == b'\n')?;
        if &contents[..header] != url.as_bytes() {
            return None;
        }
        // Reading counts as use for the eviction order.
        if let Ok(file) = std::fs::File::options().append(true).open(&path) {
            let _ = file.set_modified(std::time::SystemTime::now());
        }
        contents.drain(..=header);
        Some(contents)
    }

    fn write(&self, url: &str, bytes: &[u8]) {
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.path(url);
        let partial = path.with_extension("partial");
        let mut contents = Vec::with_capacity(url.len() + 1 + bytes.len());
        contents.extend_from_slice(url.as_bytes());
        contents.push(b'\n');
        contents.extend_from_slice(bytes);
        let written = contents.len() as u64;
        if std::fs::write(&partial, contents).is_err() || std::fs::rename(&partial, &path).is_err()
        {
            return;
        }
        let mut size = lock(&self.size);
        let total = match *size {
            Some(total) => total + written,
            None => self.files().iter().map(|(_, len, _)| len).sum(),
        };
        *size = Some(if total > self.limit {
            self.trim()
        } else {
            total
        });
    }

    fn files(&self) -> Vec<(std::time::SystemTime, u64, PathBuf)> {
        std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let metadata = entry.metadata().ok()?;
                Some((metadata.modified().ok()?, metadata.len(), entry.path()))
            })
            .collect()
    }

    /// Removes the least recently used files until the rest fit the limit,
    /// and returns the bytes left.
    fn trim(&self) -> u64 {
        let mut files = self.files();
        let mut total: u64 = files.iter().map(|(_, len, _)| len).sum();
        files.sort_by_key(|(modified, _, _)| *modified);
        for (_, len, path) in files {
            if total <= self.limit {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
        total
    }
}

/// FNV-1a: a hash that stays the same across builds, which a file name must.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
