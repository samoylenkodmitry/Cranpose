use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cranpose_services::{
    BundledAssetError, BundledAssetReader, BundledAssets, StreamingAssetReader,
    set_platform_bundled_assets,
};

pub(crate) fn register() {
    set_platform_bundled_assets(Arc::new(DesktopBundledAssets));
}

struct DesktopBundledAssets;

impl DesktopBundledAssets {
    fn resolve(&self, path: &str) -> Option<PathBuf> {
        let relative = safe_relative_path(path)?;
        roots()
            .into_iter()
            .map(|root| root.join(&relative))
            .find(|candidate| candidate.is_file())
    }

    fn locate(&self, path: &str) -> Result<PathBuf, BundledAssetError> {
        if safe_relative_path(path).is_none() {
            return Err(BundledAssetError::InvalidPath(path.to_owned()));
        }
        self.resolve(path)
            .ok_or_else(|| BundledAssetError::NotFound(path.to_owned()))
    }
}

impl BundledAssets for DesktopBundledAssets {
    fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
        let resolved = self.locate(path)?;
        std::fs::read(&resolved).map_err(|error| BundledAssetError::ReadFailed {
            path: path.to_owned(),
            message: error.to_string(),
        })
    }

    fn open(&self, path: &str) -> Result<Box<dyn BundledAssetReader>, BundledAssetError> {
        let resolved = self.locate(path)?;
        let file = File::open(&resolved).map_err(|error| BundledAssetError::ReadFailed {
            path: path.to_owned(),
            message: error.to_string(),
        })?;
        Ok(Box::new(StreamingAssetReader::new(path, file)))
    }

    fn len(&self, path: &str) -> Option<u64> {
        Some(self.resolve(path)?.metadata().ok()?.len())
    }
}

fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        roots.push(directory.to_path_buf());
        let resources = directory.join("..").join("Resources");
        if resources.is_dir() {
            roots.push(resources);
        }
    }
    if let Ok(working) = std::env::current_dir() {
        roots.push(working);
    }
    roots
}

fn safe_relative_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    let candidate = Path::new(path);
    let mut safe = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!safe.as_os_str().is_empty()).then_some(safe)
}

#[cfg(test)]
#[path = "tests/desktop_bundled_assets_tests.rs"]
mod tests;
