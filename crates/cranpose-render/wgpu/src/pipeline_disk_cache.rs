use std::{
    hash::Hasher,
    io::Write,
    path::{Path, PathBuf},
    sync::{OnceLock, mpsc},
};

use cranpose_ui_graphics::FxHasher;
use web_time::Instant;

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
    // SAFETY: `data` is `persist`'s own `get_data` output under this build's
    // key, and `fallback: true` has wgpu validate the header and fall back to
    // an empty cache.
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

/// Names what fills a blob: the framework's shaders, and this crate's version
/// for the shader rewrites, pipeline layouts and translator between those
/// sources and the driver.
///
/// The driver keeps every pipeline of the blob it loads in the cache it saves,
/// so a blob kept across builds collects the pipelines of every shader edit,
/// and the driver holds all of it resident. A blob under another key is
/// dropped instead, and refilled with this build's pipelines alone.
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

/// Decides, once a tick, whether the cache should be written: after the
/// pipeline count has grown since the last write and then held still for a
/// whole tick, so a burst of compiles is written once, after its last one,
/// and a variant first reached late in a session reaches the disk too.
#[derive(Default)]
struct PersistWatch {
    persisted: u64,
    seen: u64,
}

impl PersistWatch {
    fn observe(&mut self, created: u64) -> bool {
        let quiet = created == self.seen;
        self.seen = created;
        if quiet && created != self.persisted {
            self.persisted = created;
            return true;
        }
        false
    }
}

const PERSIST_TICK: std::time::Duration = std::time::Duration::from_secs(2);

pub(crate) fn spawn_persist_watcher(cache: wgpu::PipelineCache) -> Option<mpsc::Sender<()>> {
    let path = file_path()?;
    let (lifetime, stopped) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("cranpose-pl-cache".into())
        .spawn(move || {
            let mut watch = PersistWatch::default();
            while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(PERSIST_TICK) {
                if watch.observe(
                    crate::render::pipelines_created()
                        + crate::render::pipelines_created_off_frame(),
                ) {
                    persist(&cache, &path);
                }
            }
        });
    match spawned {
        Ok(_) => Some(lifetime),
        Err(error) => {
            log::warn!("[pipeline-cache] persist thread failed to spawn: {error}");
            None
        }
    }
}

#[cfg(test)]
#[path = "tests/pipeline_disk_cache_tests.rs"]
mod tests;
