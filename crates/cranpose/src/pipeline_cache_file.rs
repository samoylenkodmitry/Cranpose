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
mod tests {
    use std::path::PathBuf;

    use cranpose_services::PlatformDirectories;

    use super::cache_file;

    fn directories() -> PlatformDirectories {
        PlatformDirectories {
            data: PathBuf::from("/data/com.example.app"),
            config: PathBuf::from("/config/com.example.app"),
            cache: PathBuf::from("/cache/com.example.app"),
            documents: None,
            temporary: PathBuf::from("/tmp/com.example.app"),
            shared: None,
        }
    }

    #[test]
    fn the_blob_sits_under_the_application_data_directory() {
        assert_eq!(
            cache_file(&directories()),
            PathBuf::from("/data/com.example.app/pipeline-cache.bin")
        );
    }

    #[test]
    fn the_blob_avoids_directories_the_platform_may_empty() {
        let directories = directories();
        let path = cache_file(&directories);
        assert!(
            !path.starts_with(&directories.cache),
            "Android empties the cache directory under storage pressure, and a \
             compile paid once per install must outlive that"
        );
        assert!(!path.starts_with(&directories.temporary));
    }
}
