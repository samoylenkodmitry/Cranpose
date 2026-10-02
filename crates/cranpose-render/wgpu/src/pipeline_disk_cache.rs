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

use crate::debug_toggles::DebugToggle;

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
/// pipelines, where the device keeps them, and the shape pipelines that
/// launch drew its first screen with.
pub(crate) struct Loaded {
    pub(crate) cache: Option<wgpu::PipelineCache>,
    pub(crate) first_screen: Vec<u64>,
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
    let contents = file.as_deref().and_then(current_contents);
    let first_screen = contents
        .as_ref()
        .map_or_else(Vec::new, |contents| contents.first_screen.clone().collect());
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        log::info!(
            "[pipeline-cache] not offered by {:?}; compiled pipelines persist only as far \
             as the driver's own cache does",
            device.adapter_info().backend
        );
        return Loaded {
            cache: None,
            first_screen,
        };
    }
    let data = contents
        .as_ref()
        .map(|contents| contents.blob)
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
        (None, Some(file)) if contents.is_none() => log::info!(
            "[pipeline-cache] cold: dropped {} B compiled from other shaders",
            file.len()
        ),
        (None, _) => log::info!("[pipeline-cache] cold (no blob on disk)"),
    }
    Loaded {
        cache: Some(cache),
        first_screen,
    }
}

/// Names what fills a file: the framework's WGSL sources, and this crate's
/// version, which changes with each release of the shader rewrites, pipeline
/// layouts and translator between those sources and the driver, and with
/// every pipeline key's layout.
///
/// The driver keeps every pipeline of the blob it loads in the cache it saves,
/// so a blob kept across shader changes only grows, and the driver holds all
/// of it resident. A file under another key loads cold and is replaced.
fn blob_key() -> [u8; 8] {
    let mut hasher = FxHasher::default();
    hasher.write_u64(cranpose_ui_graphics::framework_shaders::SOURCES_KEY);
    hasher.write(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.finish().to_le_bytes()
}

/// A cache file this build wrote: its key, the first screen's pipeline
/// keys behind their count, then the driver's blob.
struct Contents<'a> {
    first_screen: FirstScreenKeys<'a>,
    blob: &'a [u8],
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
}

/// The file's contents, if this build's key leads it and it is whole.
fn current_contents(file: &[u8]) -> Option<Contents<'_>> {
    let rest = file.strip_prefix(blob_key().as_slice())?;
    let (count, rest) = rest.split_first_chunk::<4>()?;
    let keys_len = usize::try_from(u32::from_le_bytes(*count))
        .ok()?
        .checked_mul(8)?;
    let (keys, blob) = rest.split_at_checked(keys_len)?;
    Some(Contents {
        first_screen: FirstScreenKeys(keys),
        blob,
    })
}

fn file_bytes(first_screen: &[u64], blob: &[u8]) -> Option<Vec<u8>> {
    let count = u32::try_from(first_screen.len()).ok()?;
    let mut bytes = Vec::with_capacity(12 + first_screen.len() * 8 + blob.len());
    bytes.extend_from_slice(&blob_key());
    bytes.extend_from_slice(&count.to_le_bytes());
    for key in first_screen {
        bytes.extend_from_slice(&key.to_le_bytes());
    }
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
    let first_screen = first_screen_keys();
    let Some(bytes) = file_bytes(&first_screen, &blob) else {
        return;
    };
    if std::fs::read(path).is_ok_and(|existing| existing == bytes) {
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
            first_screen.len(),
            crate::render::instant_ms(started, Instant::now()),
        ),
        Err(error) => log::warn!("[pipeline-cache] write {path:?}: {error}"),
    }
}

/// How long after its creation a renderer names the shape pipelines it
/// draws with, for the next launch to build ahead of its first frame.
pub(crate) const FIRST_SCREEN_SPAN: Duration = Duration::from_secs(2);

/// The shape pipelines, by their keys' bits, that this process's renderers
/// drew their first screens with, in the order first drawn.
static FIRST_SCREEN: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// Notes a shape pipeline a renderer drew its first screen with. The notes
/// are written with the cache, for the next launch to build ahead of its
/// first frame.
pub(crate) fn note_first_screen_pipeline(key: u64) {
    let mut keys = FIRST_SCREEN.lock().unwrap_or_else(PoisonError::into_inner);
    if !keys.contains(&key) {
        keys.push(key);
    }
}

fn first_screen_keys() -> Vec<u64> {
    FIRST_SCREEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

/// How long pipeline builds must pause before the cache is written. A burst
/// of builds, such as a screen's first frames, is written once after its last
/// build, and a session closed a moment after launch still keeps what the
/// launch compiled, so the next launch skips those compiles.
const PERSIST_QUIET: Duration = Duration::from_millis(500);

/// Pipelines this process has built, for the watchers that write them back.
static BUILDS: BuildSignal = BuildSignal::new();

/// Counts a pipeline build and wakes the watchers waiting to write it back.
pub(crate) fn note_pipeline_built() {
    BUILDS.note_built();
}

/// A count of pipeline builds that watchers can wait on.
struct BuildSignal {
    built: Mutex<u64>,
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

impl BuildSignal {
    const fn new() -> Self {
        Self {
            built: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, u64> {
        self.built.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn note_built(&self) {
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

    /// Sleeps until builds past `written` pause for `quiet`, or until the
    /// watcher is stopped. Nothing wakes it while no pipeline is built.
    fn wait_for_quiet(&self, written: u64, quiet: Duration, stopped: &AtomicBool) -> Wake {
        let is_stopped = || stopped.load(Ordering::Acquire);
        let mut built = self
            .changed
            .wait_while(self.lock(), |built| *built == written && !is_stopped())
            .unwrap_or_else(PoisonError::into_inner);
        loop {
            if is_stopped() {
                return Wake::Stopped(*built);
            }
            let seen = *built;
            let (next, waited) = self
                .changed
                .wait_timeout_while(built, quiet, |built| *built == seen && !is_stopped())
                .unwrap_or_else(PoisonError::into_inner);
            built = next;
            if waited.timed_out() {
                return Wake::Quiet(*built);
            }
        }
    }
}

/// Keeps the cache's watcher writing new pipelines to disk. Dropping it
/// writes what is still unwritten and ends the watcher.
pub(crate) struct PersistWatcher {
    stopped: Arc<AtomicBool>,
}

impl Drop for PersistWatcher {
    fn drop(&mut self) {
        BUILDS.stop(&self.stopped);
    }
}

pub(crate) fn spawn_persist_watcher(cache: Option<wgpu::PipelineCache>) -> Option<PersistWatcher> {
    let path = file_path()?;
    let stopped = Arc::new(AtomicBool::new(false));
    let watcher_stopped = Arc::clone(&stopped);
    let mut written = *BUILDS.lock();
    let spawned = std::thread::Builder::new()
        .name("cranpose-pl-cache".into())
        .spawn(move || {
            loop {
                match BUILDS.wait_for_quiet(written, PERSIST_QUIET, &watcher_stopped) {
                    Wake::Quiet(built) => {
                        persist(cache.as_ref(), &path);
                        written = built;
                    }
                    Wake::Stopped(built) => {
                        if built != written {
                            persist(cache.as_ref(), &path);
                        }
                        return;
                    }
                }
            }
        });
    match spawned {
        Ok(_) => Some(PersistWatcher { stopped }),
        Err(error) => {
            log::warn!("[pipeline-cache] persist thread failed to spawn: {error}");
            None
        }
    }
}

#[cfg(test)]
#[path = "tests/pipeline_disk_cache_tests.rs"]
mod tests;
