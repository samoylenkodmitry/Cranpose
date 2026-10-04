use std::{
    hash::Hasher,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use cranpose_ui_graphics::FxHasher;
use web_time::{Duration, Instant};

use crate::{
    debug_toggles::DebugToggle,
    pipeline_records::{self, FirstScreenRecords, ShaderPipelineRecord},
};

static DISK_CACHE: DebugToggle = DebugToggle::new("CRANPOSE_PIPELINE_DISK_CACHE");

fn disk_cache_enabled() -> bool {
    !DISK_CACHE.equals("0")
}

static HOST_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Names the file the driver's compiled pipelines are kept in between runs.
///
/// This crate renders and does not know where an application may write, so the
/// platform backend passes the path in once, before the first device is
/// created. Without it the cache lives and dies with the process, and every
/// launch pays the driver's compile again. A later call is ignored: the
/// loaded blob and the watcher that writes it back must name one file.
pub fn set_file_path(path: PathBuf) {
    let _ = HOST_FILE.set(path);
}

pub(crate) fn file_path() -> Option<PathBuf> {
    if !disk_cache_enabled() {
        return None;
    }
    match crate::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_FILE") {
        Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => HOST_FILE.get().cloned(),
    }
}

/// What the last launch left for this one: the driver's compiled
/// pipelines, where the device keeps them, and the shape, runtime shader
/// and fixed pipelines that launch drew its first screen with.
pub(crate) struct Loaded {
    pub(crate) cache: Option<wgpu::PipelineCache>,
    pub(crate) first_screen: Vec<u64>,
    pub(crate) first_screen_records: FirstScreenRecords,
}

pub(crate) fn load(device: &wgpu::Device) -> Loaded {
    let path = file_path();
    let file = path.as_deref().and_then(|path| match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!("[pipeline-cache] unreadable {path:?}: {error}");
            None
        }
    });
    let mut contents = file.as_deref().and_then(current_contents);
    let first_screen = contents
        .as_ref()
        .map_or_else(Vec::new, |contents| contents.first_screen.clone().collect());
    let first_screen_records = contents
        .as_mut()
        .map(|contents| std::mem::take(&mut contents.records))
        .unwrap_or_default();
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        log::info!(
            "[pipeline-cache] not offered by {:?}; compiled pipelines persist only as far \
             as the driver's own cache does",
            device.adapter_info().backend
        );
        return Loaded {
            cache: None,
            first_screen,
            first_screen_records,
        };
    }
    let data = contents
        .as_ref()
        .and_then(|contents| contents.blob)
        .filter(|blob| !blob.is_empty());
    // SAFETY: `data` is this build's own `get_data` output, and `fallback:
    // true` has wgpu validate the header and fall back to an empty cache.
    #[expect(unsafe_code)]
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("cranpose pipeline disk cache"),
            data,
            fallback: true,
        })
    };
    match (data, &file) {
        (Some(data), _) => log::info!("[pipeline-cache] loaded {} B from disk", data.len()),
        (None, Some(file))
            if contents
                .as_ref()
                .is_none_or(|contents| contents.blob.is_none()) =>
        {
            log::info!(
                "[pipeline-cache] cold: dropped {} B compiled from other shaders",
                file.len()
            );
        }
        (None, _) => log::info!("[pipeline-cache] cold (no blob on disk)"),
    }
    Loaded {
        cache: Some(cache),
        first_screen,
        first_screen_records,
    }
}

const FILE_LAYOUT: u32 = 4;

/// Names what fills a file: its layout, the framework's WGSL sources, and
/// this crate's version, which changes with each release of the shader
/// rewrites, pipeline layouts and translator between those sources and the
/// driver, and with every pipeline key's layout.
///
/// The driver keeps every pipeline of the blob it loads in the cache it saves,
/// so a blob kept across shader changes only grows, and the driver holds all
/// of it resident. A file under another key loads cold and is replaced.
fn blob_key() -> [u8; 8] {
    let mut hasher = FxHasher::default();
    hasher.write_u32(FILE_LAYOUT);
    hasher.write_u64(cranpose_ui_graphics::framework_shaders::SOURCES_KEY);
    hasher.write(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.finish().to_le_bytes()
}

struct Contents<'a> {
    first_screen: FirstScreenKeys<'a>,
    records: FirstScreenRecords,
    blob: Option<&'a [u8]>,
}

#[derive(Clone)]
struct FirstScreenKeys<'a>(&'a [u8]);

impl Iterator for FirstScreenKeys<'_> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let (key, rest) = self.0.split_first_chunk::<8>()?;
        self.0 = rest;
        Some(u64::from_le_bytes(*key))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }
}

impl ExactSizeIterator for FirstScreenKeys<'_> {
    fn len(&self) -> usize {
        self.0.len() / 8
    }
}

fn current_contents(file: &[u8]) -> Option<Contents<'_>> {
    let (build, rest) = file.split_first_chunk::<8>()?;
    let rest = rest.strip_prefix(crate::render::ShapePipelineKey::DISK_LAYOUT.as_slice())?;
    let (count, rest) = rest.split_first_chunk::<4>()?;
    let keys_len = usize::try_from(u32::from_le_bytes(*count))
        .ok()?
        .checked_mul(8)?;
    let (keys, rest) = rest.split_at_checked(keys_len)?;
    // The shape keys stand on their own: records that do not decode cost
    // only themselves and the driver blob behind them.
    let (records, blob) = pipeline_records::decode(rest).map_or_else(
        || (FirstScreenRecords::default(), None),
        |(records, blob)| (records, Some(blob)),
    );
    Some(Contents {
        first_screen: FirstScreenKeys(keys),
        records,
        blob: blob.filter(|_| *build == blob_key()),
    })
}

fn file_bytes<'a>(
    first_screen: impl ExactSizeIterator<Item = u64>,
    shaders: &[ShaderPipelineRecord],
    fixed: impl ExactSizeIterator<Item = &'a str>,
    blob: &[u8],
) -> Option<Vec<u8>> {
    let count = u32::try_from(first_screen.len()).ok()?;
    let mut bytes = Vec::with_capacity(20 + first_screen.len() * 8 + blob.len());
    bytes.extend_from_slice(&blob_key());
    bytes.extend_from_slice(&crate::render::ShapePipelineKey::DISK_LAYOUT);
    bytes.extend_from_slice(&count.to_le_bytes());
    for key in first_screen {
        bytes.extend_from_slice(&key.to_le_bytes());
    }
    pipeline_records::encode(shaders, fixed, &mut bytes)?;
    bytes.extend_from_slice(blob);
    Some(bytes)
}

fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)
}

pub(crate) fn persist(cache: Option<&wgpu::PipelineCache>, path: &Path) {
    let started = Instant::now();
    let blob = cache
        .and_then(wgpu::PipelineCache::get_data)
        .unwrap_or_default();
    let existing = std::fs::read(path).ok();
    let previous = existing.as_deref().and_then(current_contents);
    // A launch closed before its first frame keeps the previous launch's
    // first screen.
    let (bytes, key_count) = if FIRST_FRAME_DRAWN.load(Ordering::Acquire) {
        let first_screen = first_screen();
        (
            file_bytes(
                first_screen.keys.iter().copied(),
                &first_screen.shaders,
                first_screen.fixed.iter().copied(),
                &blob,
            ),
            first_screen.len(),
        )
    } else if let Some(previous) = previous {
        let records = &previous.records;
        (
            file_bytes(
                previous.first_screen.clone(),
                &records.shaders,
                records.fixed.iter().map(String::as_str),
                &blob,
            ),
            previous.first_screen.len() + records.shaders.len() + records.fixed.len(),
        )
    } else {
        (
            file_bytes(std::iter::empty(), &[], std::iter::empty(), &blob),
            0,
        )
    };
    let Some(bytes) = bytes else {
        return;
    };
    if existing.is_some_and(|existing| existing == bytes) {
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        log::warn!("[pipeline-cache] create_dir_all {parent:?}: {error}");
        return;
    }
    let tmp = path.with_extension("tmp");
    let written = write_file(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, path));
    match written {
        Ok(()) => log::info!(
            "[pipeline-cache] persisted {} B and {} first-screen pipelines in {:.1} ms",
            blob.len(),
            key_count,
            crate::render::instant_ms(started, Instant::now()),
        ),
        Err(error) => log::warn!("[pipeline-cache] write {path:?}: {error}"),
    }
}

pub(crate) const FIRST_SCREEN_SPAN: Duration = Duration::from_secs(2);

/// The pipelines this process's renderers drew their first screens with,
/// each list in the order first drawn.
struct FirstScreen {
    /// The shape pipelines, by their keys' bits.
    keys: Vec<u64>,
    shaders: Vec<ShaderPipelineRecord>,
    /// The fixed pipelines, by label.
    fixed: Vec<&'static str>,
}

impl FirstScreen {
    fn len(&self) -> usize {
        self.keys.len() + self.shaders.len() + self.fixed.len()
    }
}

static FIRST_SCREEN: Mutex<FirstScreen> = Mutex::new(FirstScreen {
    keys: Vec::new(),
    shaders: Vec::new(),
    fixed: Vec::new(),
});
static FIRST_FRAME_DRAWN: AtomicBool = AtomicBool::new(false);

fn first_screen() -> MutexGuard<'static, FirstScreen> {
    FIRST_SCREEN.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Notes a shape pipeline a renderer drew its first screen with. The notes
/// are written with the cache, for the next launch to build ahead of its
/// first frame.
pub(crate) fn note_first_screen_pipeline(key: u64) {
    let mut first_screen = first_screen();
    if !first_screen.keys.contains(&key) {
        first_screen.keys.push(key);
        CHANGES.note_change();
    }
}

/// Notes a runtime shader pipeline a renderer drew its first screen with,
/// written with the cache like [`note_first_screen_pipeline`]'s.
pub(crate) fn note_first_screen_shader(record: ShaderPipelineRecord) {
    let mut first_screen = first_screen();
    if !first_screen.shaders.contains(&record) {
        first_screen.shaders.push(record);
        CHANGES.note_change();
    }
}

/// Notes a fixed pipeline a renderer drew its first screen with, written
/// with the cache like [`note_first_screen_pipeline`]'s.
pub(crate) fn note_first_screen_fixed(label: &'static str) {
    let mut first_screen = first_screen();
    if !first_screen.fixed.contains(&label) {
        first_screen.fixed.push(label);
        CHANGES.note_change();
    }
}

pub(crate) fn note_frame_drawn() {
    if !FIRST_FRAME_DRAWN.load(Ordering::Acquire) {
        FIRST_FRAME_DRAWN.store(true, Ordering::Release);
        CHANGES.note_change();
    }
}

/// How long pipeline builds must pause before the cache is written. A burst
/// of builds, such as a screen's first frames, is written once after its last
/// build, and a session closed a moment after launch still keeps what the
/// launch compiled, so the next launch skips those compiles.
const PERSIST_QUIET: Duration = Duration::from_millis(500);

static CHANGES: PersistSignal = PersistSignal::new();

/// Counts a pipeline build and wakes the watchers waiting to write it back.
pub(crate) fn note_pipeline_built() {
    CHANGES.note_change();
}

struct PersistSignal {
    revision: Mutex<u64>,
    changed: Condvar,
}

/// Why a watcher woke, with the build count at that moment.
#[derive(Debug, PartialEq, Eq)]
enum Wake {
    /// Builds past the written count paused for the quiet period.
    Quiet(u64),
    /// The watcher's owner let it go.
    Stopped(u64),
}

impl PersistSignal {
    const fn new() -> Self {
        Self {
            revision: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, u64> {
        self.revision.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn note_change(&self) {
        *self.lock() += 1;
        self.changed.notify_all();
    }

    fn stop(&self, stopped: &AtomicBool) {
        stopped.store(true, Ordering::Release);
        // Holding the lock orders the flag against a watcher between its
        // check and its wait, so the wake below cannot be lost.
        let _held = self.lock();
        self.changed.notify_all();
    }

    fn wait_for_quiet(&self, written: u64, quiet: Duration, stopped: &AtomicBool) -> Wake {
        let is_stopped = || stopped.load(Ordering::Acquire);
        let mut revision = self
            .changed
            .wait_while(self.lock(), |revision| {
                *revision == written && !is_stopped()
            })
            .unwrap_or_else(PoisonError::into_inner);
        loop {
            if is_stopped() {
                return Wake::Stopped(*revision);
            }
            let seen = *revision;
            let (next, waited) = self
                .changed
                .wait_timeout_while(revision, quiet, |revision| {
                    *revision == seen && !is_stopped()
                })
                .unwrap_or_else(PoisonError::into_inner);
            revision = next;
            if waited.timed_out() {
                return Wake::Quiet(*revision);
            }
        }
    }
}

/// Keeps the cache's watcher writing new pipelines to disk. Dropping it
/// writes what is still unwritten and ends the watcher.
pub(crate) struct PersistWatcher {
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for PersistWatcher {
    fn drop(&mut self) {
        CHANGES.stop(&self.stopped);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::warn!("[pipeline-cache] persist thread panicked during shutdown");
        }
    }
}

pub(crate) fn spawn_persist_watcher(cache: Option<wgpu::PipelineCache>) -> Option<PersistWatcher> {
    let path = file_path()?;
    let stopped = Arc::new(AtomicBool::new(false));
    let watcher_stopped = Arc::clone(&stopped);
    let mut written = *CHANGES.lock();
    let spawned = std::thread::Builder::new()
        .name("cranpose-pl-cache".into())
        .spawn(move || {
            loop {
                match CHANGES.wait_for_quiet(written, PERSIST_QUIET, &watcher_stopped) {
                    Wake::Quiet(revision) => {
                        persist(cache.as_ref(), &path);
                        written = revision;
                    }
                    Wake::Stopped(revision) => {
                        if revision != written {
                            persist(cache.as_ref(), &path);
                        }
                        return;
                    }
                }
            }
        });
    match spawned {
        Ok(thread) => Some(PersistWatcher {
            stopped,
            thread: Some(thread),
        }),
        Err(error) => {
            log::warn!("[pipeline-cache] persist thread failed to spawn: {error}");
            None
        }
    }
}

#[cfg(test)]
#[path = "tests/pipeline_disk_cache_tests.rs"]
mod tests;
