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

pub(crate) fn load(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        log::info!(
            "[pipeline-cache] not offered by {:?}; compiled pipelines persist only as far \
             as the driver's own cache does",
            device.adapter_info().backend
        );
        return None;
    }
    let path = file_path();
    let file = path.as_deref().and_then(|path| match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!("[pipeline-cache] unreadable {path:?}: {error}");
            None
        }
    });
    let data = file.as_deref().and_then(current_blob);
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
        (None, Some(file)) => log::info!(
            "[pipeline-cache] cold: dropped {} B compiled from other shaders",
            file.len()
        ),
        (None, None) => log::info!("[pipeline-cache] cold (no blob on disk)"),
    }
    Some(cache)
}

/// Names what fills a blob: the framework's WGSL sources, and this crate's
/// version, which changes with each release of the shader rewrites, pipeline
/// layouts and translator between those sources and the driver.
///
/// The driver keeps every pipeline of the blob it loads in the cache it saves,
/// so a blob kept across shader changes only grows, and the driver holds all
/// of it resident. A blob under another key loads cold and is replaced.
fn blob_key() -> [u8; 8] {
    let mut hasher = FxHasher::default();
    hasher.write_u64(cranpose_ui_graphics::framework_shaders::SOURCES_KEY);
    hasher.write(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.finish().to_le_bytes()
}

/// The driver's blob within a cache file, if this build's key leads it.
fn current_blob(file: &[u8]) -> Option<&[u8]> {
    file.strip_prefix(blob_key().as_slice())
}

fn write_blob(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(&blob_key())?;
    file.write_all(data)
}

pub(crate) fn persist(cache: &wgpu::PipelineCache, path: &Path) {
    let started = Instant::now();
    let Some(data) = cache.get_data() else {
        return;
    };
    if let Ok(existing) = std::fs::read(path)
        && current_blob(&existing) == Some(data.as_slice())
    {
        return;
    }
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        log::warn!("[pipeline-cache] create_dir_all {parent:?}: {error}");
        return;
    }
    let tmp = path.with_extension("tmp");
    let written = write_blob(&tmp, &data).and_then(|()| std::fs::rename(&tmp, path));
    match written {
        Ok(()) => log::info!(
            "[pipeline-cache] persisted {} B in {:.1} ms",
            data.len(),
            crate::render::instant_ms(started, Instant::now()),
        ),
        Err(error) => log::warn!("[pipeline-cache] write {path:?}: {error}"),
    }
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

pub(crate) fn spawn_persist_watcher(cache: wgpu::PipelineCache) -> Option<PersistWatcher> {
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
                        persist(&cache, &path);
                        written = built;
                    }
                    Wake::Stopped(built) => {
                        if built != written {
                            persist(&cache, &path);
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
