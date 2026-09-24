use std::path::PathBuf;

use cranpose_services::PlatformDirectories;

const PIPELINE_CACHE_FILE_NAME: &str = "pipeline-cache.bin";

fn cache_file(directories: &PlatformDirectories) -> PathBuf {
    directories.data.join(PIPELINE_CACHE_FILE_NAME)
}

pub(crate) fn publish() {
    let Ok(directories) = cranpose_services::application_directories() else {
        return;
    };
    hand_to_renderer(cache_file(&directories));
}

#[cfg(feature = "renderer-wgpu")]
fn hand_to_renderer(path: PathBuf) {
    cranpose_render_wgpu::pipeline_disk_cache::set_file_path(path);
}

#[cfg(not(feature = "renderer-wgpu"))]
fn hand_to_renderer(_path: PathBuf) {}

#[cfg(test)]
#[path = "tests/pipeline_cache_file_tests.rs"]
mod tests;
