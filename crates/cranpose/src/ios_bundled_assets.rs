use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cranpose_services::{
    BundledAssetError, BundledAssetReader, BundledAssets, StreamingAssetReader,
    set_platform_bundled_assets,
};
use objc2_foundation::NSBundle;

pub(crate) fn register() {
    set_platform_bundled_assets(Arc::new(IosBundledAssets));
}

struct IosBundledAssets;

impl IosBundledAssets {
    fn resolve(&self, path: &str) -> Result<PathBuf, BundledAssetError> {
        let relative = Path::new(path);
        let is_plain_relative = !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|component| matches!(component, Component::Normal(_)));
        if !is_plain_relative {
            return Err(BundledAssetError::InvalidPath(path.to_string()));
        }
        let root =
            NSBundle::mainBundle()
                .resourcePath()
                .ok_or_else(|| BundledAssetError::ReadFailed {
                    path: path.to_string(),
                    message: "the application bundle has no resource directory".to_string(),
                })?;
        Ok(PathBuf::from(root.to_string()).join(relative))
    }
}

impl BundledAssets for IosBundledAssets {
    fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
        let full_path = self.resolve(path)?;
        std::fs::read(&full_path).map_err(|error| read_error(path, &full_path, error))
    }

    fn open(&self, path: &str) -> Result<Box<dyn BundledAssetReader>, BundledAssetError> {
        let full_path = self.resolve(path)?;
        let file = File::open(&full_path).map_err(|error| read_error(path, &full_path, error))?;
        Ok(Box::new(StreamingAssetReader::new(path, file)))
    }

    fn len(&self, path: &str) -> Option<u64> {
        let full_path = self.resolve(path).ok()?;
        std::fs::metadata(full_path)
            .ok()
            .map(|metadata| metadata.len())
    }
}

fn read_error(path: &str, full_path: &Path, error: std::io::Error) -> BundledAssetError {
    if error.kind() == std::io::ErrorKind::NotFound {
        BundledAssetError::NotFound(path.to_string())
    } else {
        BundledAssetError::ReadFailed {
            path: path.to_string(),
            message: format!("{}: {error}", full_path.display()),
        }
    }
}
