#![expect(unsafe_code)]
//! Android tells an updated app so (`MY_PACKAGE_REPLACED`), and its
//! receiver, `CranposePipelines`, runs here: the pipelines the app's last
//! launches drew are built for this build in the background, so its first
//! launch after the update finds them in the driver cache.

use std::{path::Path, sync::Arc, time::Duration};

use cranpose_render_wgpu::WgpuRenderer;
use jni::{
    EnvUnowned, Outcome,
    objects::{JClass, JString},
};
use web_time::Instant;

/// How long the receiver builds pipelines: Android ends a background
/// broadcast after a minute.
const PREPARE_TIMEOUT: Duration = Duration::from_secs(45);

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposePipelines_nativePrepareAfterUpdate<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    data_dir: JString<'local>,
) {
    crate::android::init_logging(crate::android::DEFAULT_LOG_TAG);
    match env
        .with_env(|env| data_dir.try_to_string(env))
        .into_outcome()
    {
        Outcome::Ok(data_dir) => prepare_after_update(Path::new(&data_dir)),
        Outcome::Err(_) | Outcome::Panic(_) => {
            log::warn!("[pipeline-cache] the app's data directory did not decode");
        }
    }
}

/// Builds the pipelines the cache file in `data_dir` records on the device
/// the app draws with, and writes them back for this build.
fn prepare_after_update(data_dir: &Path) {
    let started = Instant::now();
    crate::android::keep_pipeline_cache_in(data_dir);
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::VULKAN;
    descriptor.flags = wgpu::InstanceFlags::empty();
    let instance = wgpu::Instance::new(descriptor);
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    })) {
        Ok(adapter) => adapter,
        Err(error) => {
            log::warn!("[pipeline-cache] no Vulkan adapter to prepare pipelines on: {error}");
            return;
        }
    };
    let info = adapter.get_info();
    crate::android::name_pipeline_cache_file(&info);
    let (device, queue) =
        match pollster::block_on(adapter.request_device(&crate::android::android_device(&adapter)))
        {
            Ok(device) => device,
            Err(error) => {
                log::warn!("[pipeline-cache] no device to prepare pipelines on: {error}");
                return;
            }
        };
    let mut renderer = WgpuRenderer::default();
    renderer.init_gpu(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8Unorm,
        info.backend,
        adapter.get_downlevel_capabilities().flags,
    );
    let built = renderer.prepare_recorded_pipelines(PREPARE_TIMEOUT);
    drop(renderer);
    log::info!(
        "[pipeline-cache] prepared the pipelines after an update in {:.0} ms{}",
        started.elapsed().as_secs_f64() * 1000.0,
        if built { "" } else { ", unfinished" },
    );
}
