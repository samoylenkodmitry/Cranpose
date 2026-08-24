//! Read-only resources shipped beside a desktop executable.
//!
//! A desktop application has no asset manager, but it does have places the
//! platform puts its resources, and where those are is a platform question
//! rather than an application one. This looks in each of them in turn:
//!
//! 1. the directory holding the executable, which is where a build step or an
//!    installer puts data files;
//! 2. `../Resources` relative to it, which is where a macOS `.app` keeps them;
//! 3. the working directory, which is where a `cargo run` finds them.
//!
//! Reads stream. `open` hands back a file, so a bundled model or database seed
//! reaches its loader without being materialised first — the crate's default
//! reader buffers the whole asset, and that is the thing worth avoiding for
//! exactly the assets big enough to be shipped this way.

use std::{
    fs::File,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use cranpose_services::{
    set_platform_bundled_assets, BundledAssetError, BundledAssetReader, BundledAssets,
    StreamingAssetReader,
};

/// Installs the desktop bundled-asset backend.
pub(crate) fn register() {
    set_platform_bundled_assets(Arc::new(DesktopBundledAssets));
}

struct DesktopBundledAssets;

impl DesktopBundledAssets {
    /// The first root that holds `path`, or `None` when no root does.
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

/// Where this executable's resources may live, nearest first.
fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            roots.push(directory.to_path_buf());
            // A macOS `.app` puts the binary in `Contents/MacOS` and its data
            // one directory over.
            let resources = directory.join("..").join("Resources");
            if resources.is_dir() {
                roots.push(resources);
            }
        }
    }
    if let Ok(working) = std::env::current_dir() {
        roots.push(working);
    }
    roots
}

/// The path as a relative one that cannot climb out of a root.
///
/// An asset name arrives from a manifest or from application code, and a name
/// that escapes its root would turn a bundled-asset read into an arbitrary file
/// read.
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
mod tests {
    use super::*;

    #[test]
    fn a_path_that_climbs_out_of_the_root_is_refused() {
        assert!(safe_relative_path("../secrets").is_none());
        assert!(safe_relative_path("models/../../secrets").is_none());
        assert!(safe_relative_path("/etc/passwd").is_none());
        assert!(safe_relative_path("").is_none());
    }

    #[test]
    fn an_ordinary_relative_path_survives_unchanged() {
        assert_eq!(
            safe_relative_path("models/detector.bin"),
            Some(PathBuf::from("models/detector.bin"))
        );
        assert_eq!(
            safe_relative_path("./models/detector.bin"),
            Some(PathBuf::from("models/detector.bin"))
        );
    }

    #[test]
    fn an_asset_beside_the_executable_reads_and_streams_the_same_bytes() {
        let Ok(executable) = std::env::current_exe() else {
            return;
        };
        let Some(directory) = executable.parent() else {
            return;
        };
        let name = "cranpose-desktop-asset-fixture.bin";
        let bytes: Vec<u8> = (0..cranpose_services::DEFAULT_CHUNK_LEN + 17)
            .map(|i| (i % 251) as u8)
            .collect();
        if std::fs::write(directory.join(name), &bytes).is_err() {
            return;
        }

        let assets = DesktopBundledAssets;
        let read = assets.read(name).expect("read");
        let mut streamed = Vec::new();
        let mut reader = assets.open(name).expect("open");
        // More than one chunk, which is what distinguishes a streaming reader
        // from one that hands the whole asset back at once.
        let mut chunks = 0;
        while let Some(chunk) = reader.read_chunk().expect("chunk") {
            chunks += 1;
            streamed.extend_from_slice(&chunk);
        }
        // Read before the fixture goes away: `len` stats the file, so asking
        // after removing it measures nothing.
        let length = assets.len(name);
        let _ = std::fs::remove_file(directory.join(name));

        assert_eq!(read, bytes);
        assert_eq!(streamed, bytes);
        assert!(chunks > 1, "expected more than one chunk, got {chunks}");
        assert_eq!(length, Some(bytes.len() as u64));
    }

    #[test]
    fn a_missing_asset_is_not_found_rather_than_a_read_failure() {
        let assets = DesktopBundledAssets;
        assert!(matches!(
            assets.read("cranpose-no-such-asset.bin"),
            Err(BundledAssetError::NotFound(_))
        ));
    }
}
