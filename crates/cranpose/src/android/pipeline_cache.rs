/// Keeps the driver's compiled pipelines in the app's data directory,
/// `data_path`, unless a directory is named already.
pub(crate) fn keep_pipeline_cache_in(data_path: &std::path::Path) {
    if cranpose_core::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_DIR").is_none() {
        let cache_dir = data_path.join("cranpose_gpu");
        cranpose_core::debug_toggles::set_debug_toggle_os(
            "CRANPOSE_PIPELINE_CACHE_DIR",
            Some(cache_dir.as_os_str()),
        );
    }
}

/// The app's assets, for the pipeline caches it ships for fresh installs.
static APP_ASSETS: std::sync::OnceLock<ndk::asset::AssetManager> = std::sync::OnceLock::new();

/// The asset directory an app ships pipeline caches in, each named as the
/// device that wrote it names its cache file.
const SEED_ASSETS: &std::ffi::CStr = c"cranpose_gpu";

/// Keeps the pipeline cache in the app's data directory, and the app's
/// assets for the caches it ships. A recording run keeps the cache where adb
/// can read it: the app's external files directory, which the system
/// created for it.
pub(super) fn keep_pipeline_cache(app: &android_activity::AndroidApp) {
    let cache_root =
        if super::frame_telemetry::property_flag("debug.cranpose.pipeline_cache_external") {
            app.external_data_path()
        } else {
            app.internal_data_path()
        };
    if let Some(data_path) = cache_root {
        keep_pipeline_cache_in(&data_path);
    }
    let _ = APP_ASSETS.set(app.asset_manager());
}

/// Starts the named pipeline cache file from the caches the app ships,
/// unless it exists: the one written on a device with this GPU and driver,
/// whole, or else another device's, for its records.
pub(super) fn seed_pipeline_cache() {
    let (Some(assets), Some(path)) = (
        APP_ASSETS.get(),
        cranpose_core::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_FILE"),
    ) else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let Some(seeds) = assets.open_dir(SEED_ASSETS) else {
        return;
    };
    let seeds: Vec<std::ffi::CString> = seeds.collect();
    let this_driver = path.file_name().and_then(std::ffi::OsStr::to_str);
    let Some((name, same_driver)) = seeds
        .iter()
        .find(|name| name.to_str().ok() == this_driver)
        .map(|name| (name, true))
        .or_else(|| seeds.first().map(|name| (name, false)))
    else {
        return;
    };
    let Ok(asset_path) =
        std::ffi::CString::new([SEED_ASSETS.to_bytes(), b"/", name.to_bytes()].concat())
    else {
        return;
    };
    let mut bytes = Vec::new();
    match assets.open(&asset_path) {
        Some(mut asset) => {
            if let Err(error) = std::io::Read::read_to_end(&mut asset, &mut bytes) {
                log::warn!("[pipeline-cache] reading the seed {asset_path:?}: {error}");
                return;
            }
        }
        None => return,
    }
    cranpose_render_wgpu::pipeline_disk_cache::seed(&path, bytes, same_driver);
}

/// Names the pipeline cache file for the GPU and driver `adapter_info`
/// describes in the cache directory, unless a file is named already: a
/// driver update leaves the old driver's pipelines behind.
pub(crate) fn name_pipeline_cache_file(adapter_info: &wgpu::AdapterInfo) {
    if cranpose_core::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_FILE").is_some() {
        return;
    }
    let Some(cache_dir) =
        cranpose_core::debug_toggles::debug_toggle_os("CRANPOSE_PIPELINE_CACHE_DIR")
    else {
        return;
    };
    let mut driver_hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in adapter_info
        .driver
        .bytes()
        .chain(adapter_info.driver_info.bytes())
    {
        driver_hash ^= u64::from(byte);
        driver_hash = driver_hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let file_name = format!(
        "pipeline_cache_v1_{:04x}_{:04x}_{driver_hash:016x}.bin",
        adapter_info.vendor, adapter_info.device,
    );
    let file_path = std::path::Path::new(&cache_dir).join(file_name);
    cranpose_core::debug_toggles::set_debug_toggle_os(
        "CRANPOSE_PIPELINE_CACHE_FILE",
        Some(file_path.as_os_str()),
    );
}
