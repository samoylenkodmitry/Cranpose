use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use web_time::Instant;

use crate::debug_toggles::DebugToggle;

static DISK_CACHE: DebugToggle = DebugToggle::new("CRANPOSE_PIPELINE_DISK_CACHE");

fn disk_cache_enabled() -> bool {
    !DISK_CACHE.equals("0")
}

pub(crate) fn file_path() -> Option<PathBuf> {
    if !disk_cache_enabled() {
        return None;
    }
    match crate::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_FILE") {
        Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => None,
    }
}

pub(crate) fn load(device: &wgpu::Device) -> Option<wgpu::PipelineCache> {
    if !device.features().contains(wgpu::Features::PIPELINE_CACHE) {
        return None;
    }
    let path = file_path();
    let data = path.as_deref().and_then(|path| match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            log::warn!("[pipeline-cache] unreadable {path:?}: {error}");
            None
        }
    });
    let loaded = data.as_ref().map(Vec::len);
    // SAFETY: `data` is `persist`'s own `get_data` output, and `fallback:
    // true` has wgpu validate the header and fall back to an empty cache.
    #[allow(unsafe_code)]
    let cache = unsafe {
        device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor {
            label: Some("cranpose pipeline disk cache"),
            data: data.as_deref(),
            fallback: true,
        })
    };
    match loaded {
        Some(bytes) => log::info!("[pipeline-cache] loaded {bytes} B from disk"),
        None => log::info!("[pipeline-cache] cold (no blob on disk)"),
    }
    Some(cache)
}

pub(crate) fn persist(cache: &wgpu::PipelineCache, path: &Path) {
    let started = Instant::now();
    let Some(data) = cache.get_data() else {
        return;
    };
    if let Ok(existing) = std::fs::read(path)
        && existing == data
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
    let written = std::fs::write(&tmp, &data).and_then(|()| std::fs::rename(&tmp, path));
    match written {
        Ok(()) => log::info!(
            "[pipeline-cache] persisted {} B in {:.1} ms",
            data.len(),
            crate::render::instant_ms(started, Instant::now()),
        ),
        Err(error) => log::warn!("[pipeline-cache] write {path:?}: {error}"),
    }
}

/// Pipelines created since the process started, bumped by every pipeline
/// creation so the persist watcher knows when the cache has grown.
static PIPELINES_CREATED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn note_pipeline_created() {
    PIPELINES_CREATED.fetch_add(1, Ordering::Relaxed);
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

pub(crate) fn spawn_persist_watcher(cache: wgpu::PipelineCache) {
    let Some(path) = file_path() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("cranpose-pl-cache".into())
        .spawn(move || {
            let mut watch = PersistWatch::default();
            loop {
                std::thread::sleep(PERSIST_TICK);
                if watch.observe(PIPELINES_CREATED.load(Ordering::Relaxed)) {
                    persist(&cache, &path);
                }
            }
        });
    if let Err(error) = spawned {
        log::warn!("[pipeline-cache] persist thread failed to spawn: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::PersistWatch;

    #[test]
    fn a_burst_of_pipelines_persists_once_after_it_goes_quiet() {
        let mut watch = PersistWatch::default();
        assert!(!watch.observe(0));
        assert!(!watch.observe(3), "still growing");
        assert!(!watch.observe(5), "still growing");
        assert!(watch.observe(5), "quiet for a tick with new pipelines");
        assert!(!watch.observe(5), "written already");
    }

    #[test]
    fn a_pipeline_reached_late_in_a_session_persists_too() {
        let mut watch = PersistWatch::default();
        watch.observe(19);
        assert!(watch.observe(19));
        for _ in 0..20 {
            assert!(!watch.observe(19));
        }
        assert!(!watch.observe(20));
        assert!(watch.observe(20));
    }
}
