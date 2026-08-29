use std::path::{Path, PathBuf};

use web_time::Instant;

fn disk_cache_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_PIPELINE_DISK_CACHE").as_deref() != Some("0")
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

pub(crate) fn spawn_persist_schedule(cache: wgpu::PipelineCache) {
    let Some(path) = file_path() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("cranpose-pl-cache".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(8));
            persist(&cache, &path);
            std::thread::sleep(std::time::Duration::from_secs(20));
            persist(&cache, &path);
        });
    if let Err(error) = spawned {
        log::warn!("[pipeline-cache] persist thread failed to spawn: {error}");
    }
}
