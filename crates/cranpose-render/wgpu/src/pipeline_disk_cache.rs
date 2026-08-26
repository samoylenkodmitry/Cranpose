//! On-disk persistence for the device pipeline cache.
//!
//! Every render pipeline in this crate is created lazily on the render
//! thread, and on a mobile driver the backend compile inside that create is
//! enormous: a Pixel Watch 3 spends 2.0 s of its first six seconds in six
//! such calls (`[pipeline-create]` measured 661 ms for one), each swallowed
//! as one silent sub-second frame. A [`wgpu::PipelineCache`] handed to every
//! creation lets the driver reuse compiled code across creates within one
//! process; writing its blob to disk lets the next launch skip the compiles
//! outright. The platform layer names the file — keyed by adapter identity,
//! so a driver update starts cold instead of loading a stale blob (wgpu
//! additionally validates the header and falls back to empty) — and this
//! module only loads and stores it.
//!
//! `CRANPOSE_PIPELINE_DISK_CACHE=0` is the kill switch (on Android via the
//! `debug.cranpose.pipeline_disk_cache` property). Platforms that never set
//! `CRANPOSE_PIPELINE_CACHE_FILE`, and devices whose adapter was not granted
//! [`wgpu::Features::PIPELINE_CACHE`], keep today's behavior exactly.

use std::path::{Path, PathBuf};

use web_time::Instant;

fn disk_cache_enabled() -> bool {
    crate::debug_toggles::debug_toggle("CRANPOSE_PIPELINE_DISK_CACHE").as_deref() != Some("0")
}

/// The blob's path, or `None` when persistence is off — by kill switch or
/// because the platform never named a file.
pub(crate) fn file_path() -> Option<PathBuf> {
    if !disk_cache_enabled() {
        return None;
    }
    match crate::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_FILE") {
        Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => None,
    }
}

/// Creates the device's pipeline cache, seeded from disk when a blob exists.
/// `None` exactly when the device lacks [`wgpu::Features::PIPELINE_CACHE`] —
/// an in-memory cache is worth having even without a disk file, since the
/// shape pipeline family shares most of its module across permutations.
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
    // The contract is that `data` came from `PipelineCache::get_data` on a
    // matching adapter: `persist` below writes exactly that into a file the
    // platform layer keys by adapter identity, so a driver update swaps
    // files instead of feeding a stale blob back.
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

/// Writes the cache blob if it changed. Called off the render thread; both
/// `get_data` and concurrent pipeline creation are internally synchronized
/// by the driver.
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
    // Write-then-rename so a launch never reads a half-written blob.
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

/// Persistence schedule: one thread, two sweeps. The first runs after the
/// base pipeline set exists on a cold launch (the watch finishes it inside
/// eight seconds even at bootstrap frame rates), the second late enough to
/// catch stragglers a scene pulls in lazily — effect blits, `DstOut`
/// variants, runtime shaders. `persist` compares content, so an unchanged
/// cache costs one `get_data` and no write.
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
