//! Read-only assets packaged with the application.

use crate::registry::ServiceRegistry;
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Error reading an asset from the application bundle.
#[derive(Clone, Debug, thiserror::Error)]
pub enum BundledAssetError {
    /// The requested asset does not exist.
    #[error("bundled asset `{0}` was not found")]
    NotFound(String),
    /// The platform could not read the asset.
    #[error("could not read bundled asset `{path}`: {message}")]
    ReadFailed {
        /// Bundle-relative asset path.
        path: String,
        /// Platform error.
        message: String,
    },
    /// The installation declaration contains an unsafe or empty path.
    #[error("invalid bundled asset path `{0}`")]
    InvalidPath(String),
    /// Files could not be installed into application storage.
    #[error("could not install bundled assets at {path}: {message}")]
    InstallFailed {
        /// Destination path being changed.
        path: String,
        /// Filesystem error.
        message: String,
    },
}

/// One bundle-relative file in a declarative asset installation.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BundledAssetEntry {
    /// Path below the set's bundle source root.
    pub source: PathBuf,
    /// Path below the set's installation directory.
    pub destination: PathBuf,
}

#[cfg(not(target_arch = "wasm32"))]
impl BundledAssetEntry {
    /// Creates an entry that keeps the same relative path at the destination.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self {
            source: path.clone(),
            destination: path,
        }
    }

    /// Creates an entry whose installed relative path differs from its bundle path.
    pub fn mapped(source: impl Into<PathBuf>, destination: impl Into<PathBuf>) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
        }
    }
}

/// A versioned set copied from the application bundle into writable storage.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BundledAssetInstallSpec {
    /// Version written only after every entry is installed.
    pub version: String,
    /// Optional bundle-relative prefix shared by every source entry.
    pub source_root: PathBuf,
    /// Writable directory containing the installed entries and version stamp.
    pub destination: PathBuf,
    /// Files belonging to this set.
    pub entries: Vec<BundledAssetEntry>,
}

#[cfg(not(target_arch = "wasm32"))]
impl BundledAssetInstallSpec {
    /// Starts an installation declaration.
    pub fn new(version: impl Into<String>, destination: impl Into<PathBuf>) -> Self {
        Self {
            version: version.into(),
            source_root: PathBuf::new(),
            destination: destination.into(),
            entries: Vec::new(),
        }
    }

    /// Sets the shared bundle-relative source prefix.
    pub fn source_root(mut self, source_root: impl Into<PathBuf>) -> Self {
        self.source_root = source_root.into();
        self
    }

    /// Adds one file to the set.
    pub fn entry(mut self, entry: BundledAssetEntry) -> Self {
        self.entries.push(entry);
        self
    }
}

/// Result of installing a bundled asset set.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundledAssetInstallOutcome {
    /// This host has no application-bundle reader.
    Unavailable,
    /// The requested version and every declared file were already present.
    Current,
    /// Files were copied and the requested version was committed.
    Installed,
}

/// Access to files packaged in the app bundle.
pub trait BundledAssets: Send + Sync {
    /// Reads one bundle-relative asset in full.
    ///
    /// Use [`open`](BundledAssets::open) for anything that should not be held
    /// in memory all at once — a bundled model, a video, a database seed.
    fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError>;

    /// Opens one bundle-relative asset for chunked reading.
    ///
    /// The default reads the whole asset and hands it back a chunk at a time,
    /// which is honest for a backend that can only produce the bytes at once.
    /// Backends with a real streaming API — Android's `AssetManager`, a file in
    /// an application bundle — override it and never materialise the asset.
    fn open(&self, path: &str) -> Result<Box<dyn BundledAssetReader>, BundledAssetError> {
        Ok(Box::new(BufferedAssetReader::new(self.read(path)?)))
    }

    /// The asset's byte length without reading it, when the backend knows it.
    fn len(&self, path: &str) -> Option<u64> {
        let _ = path;
        None
    }
}

/// Chunked reader over one bundled asset.
///
/// Synchronous and `Send` so a worker thread can drain it into a model loader
/// or a database without involving the UI thread.
pub trait BundledAssetReader: Send {
    /// Reads the next chunk, or `None` at end of asset.
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, BundledAssetError>;
}

/// The fallback reader for a backend that can only produce whole assets.
struct BufferedAssetReader {
    bytes: Vec<u8>,
    offset: usize,
}

impl BufferedAssetReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self { bytes, offset: 0 }
    }
}

impl BundledAssetReader for BufferedAssetReader {
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, BundledAssetError> {
        if self.offset >= self.bytes.len() {
            return Ok(None);
        }
        let end = (self.offset + crate::content::DEFAULT_CHUNK_LEN).min(self.bytes.len());
        let chunk = self.bytes[self.offset..end].to_vec();
        self.offset = end;
        Ok(Some(chunk))
    }
}

/// Shared bundled-assets service.
pub type BundledAssetsRef = Arc<dyn BundledAssets>;

static PLATFORM_BUNDLED_ASSETS: ServiceRegistry<dyn BundledAssets> = ServiceRegistry::new();

/// Installs a platform bundled-assets reader.
pub fn set_platform_bundled_assets(assets: BundledAssetsRef) {
    PLATFORM_BUNDLED_ASSETS.set(assets);
}

/// Removes the platform reader.
pub fn clear_platform_bundled_assets() {
    PLATFORM_BUNDLED_ASSETS.clear();
}

/// Returns the platform reader, if this host has an application bundle.
pub fn bundled_assets() -> Option<BundledAssetsRef> {
    PLATFORM_BUNDLED_ASSETS.get()
}

/// Installs a declarative set without exposing bundle APIs or partial-file
/// handling to the application.
///
/// Each file is replaced through a sibling temporary file. The version stamp
/// is committed last, so an interrupted run is retried on the next call and is
/// never mistaken for a current installation.
#[cfg(not(target_arch = "wasm32"))]
pub fn install_bundled_asset_set(
    spec: &BundledAssetInstallSpec,
) -> Result<BundledAssetInstallOutcome, BundledAssetError> {
    validate_spec(spec)?;
    let Some(assets) = bundled_assets() else {
        return Ok(BundledAssetInstallOutcome::Unavailable);
    };
    let stamp = spec.destination.join(".cranpose-assets-version");
    let current = std::fs::read_to_string(&stamp).ok();
    if current.as_deref() == Some(spec.version.as_str())
        && spec
            .entries
            .iter()
            .all(|entry| spec.destination.join(&entry.destination).is_file())
    {
        return Ok(BundledAssetInstallOutcome::Current);
    }

    std::fs::create_dir_all(&spec.destination)
        .map_err(|error| install_error(&spec.destination, error))?;
    for entry in &spec.entries {
        let source = spec.source_root.join(&entry.source);
        let source = path_for_bundle(&source)?;
        let bytes = assets.read(&source)?;
        let target = spec.destination.join(&entry.destination);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| install_error(parent, error))?;
        }
        replace_file(&target, &bytes)?;
    }
    replace_file(&stamp, spec.version.as_bytes())?;
    Ok(BundledAssetInstallOutcome::Installed)
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_spec(spec: &BundledAssetInstallSpec) -> Result<(), BundledAssetError> {
    if spec.version.is_empty() || spec.entries.is_empty() {
        return Err(BundledAssetError::InvalidPath(String::new()));
    }
    validate_relative(&spec.source_root)?;
    for entry in &spec.entries {
        validate_relative(&entry.source)?;
        validate_relative(&entry.destination)?;
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_relative(path: &Path) -> Result<(), BundledAssetError> {
    if path.as_os_str().is_empty() {
        return Ok(());
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(BundledAssetError::InvalidPath(path.display().to_string()));
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn path_for_bundle(path: &Path) -> Result<String, BundledAssetError> {
    validate_relative(path)?;
    let mut result = String::new();
    for component in path.components() {
        if matches!(component, Component::CurDir) {
            continue;
        }
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(&component.as_os_str().to_string_lossy());
    }
    if result.is_empty() {
        return Err(BundledAssetError::InvalidPath(path.display().to_string()));
    }
    Ok(result)
}

#[cfg(not(target_arch = "wasm32"))]
fn replace_file(target: &Path, bytes: &[u8]) -> Result<(), BundledAssetError> {
    use std::io::Write;

    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| BundledAssetError::InvalidPath(target.display().to_string()))?;
    let temporary = target.with_file_name(format!(".{file_name}.cranpose-part"));
    let mut output =
        std::fs::File::create(&temporary).map_err(|error| install_error(&temporary, error))?;
    output
        .write_all(bytes)
        .and_then(|()| output.sync_all())
        .map_err(|error| install_error(&temporary, error))?;
    if !target.exists() {
        return std::fs::rename(&temporary, target).map_err(|error| install_error(target, error));
    }

    let backup = target.with_file_name(format!(".{file_name}.cranpose-backup"));
    if backup.exists() {
        std::fs::remove_file(&backup).map_err(|error| install_error(&backup, error))?;
    }
    std::fs::rename(target, &backup).map_err(|error| install_error(target, error))?;
    if let Err(error) = std::fs::rename(&temporary, target) {
        let _ = std::fs::rename(&backup, target);
        return Err(install_error(target, error));
    }
    std::fs::remove_file(&backup).map_err(|error| install_error(&backup, error))
}

#[cfg(not(target_arch = "wasm32"))]
fn install_error(path: &Path, error: std::io::Error) -> BundledAssetError {
    BundledAssetError::InstallFailed {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct WholeAssets;

    impl BundledAssets for WholeAssets {
        fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
            match path {
                "big.bin" => Ok(vec![4u8; crate::content::DEFAULT_CHUNK_LEN + 9]),
                "small.txt" => Ok(b"hello".to_vec()),
                other => Err(BundledAssetError::NotFound(other.to_string())),
            }
        }
    }

    #[test]
    fn the_default_reader_streams_a_whole_asset_in_chunks() {
        let assets = WholeAssets;
        let mut reader = assets.open("big.bin").expect("the asset opens");
        let mut sizes = Vec::new();
        let mut total = 0usize;
        while let Some(chunk) = reader.read_chunk().expect("chunks read") {
            sizes.push(chunk.len());
            total += chunk.len();
        }
        assert_eq!(sizes, vec![crate::content::DEFAULT_CHUNK_LEN, 9]);
        assert_eq!(total, crate::content::DEFAULT_CHUNK_LEN + 9);
    }

    #[test]
    fn a_missing_asset_fails_to_open() {
        assert!(matches!(
            WholeAssets.open("absent.bin").err(),
            Some(BundledAssetError::NotFound(_))
        ));
    }

    #[test]
    fn a_backend_that_cannot_stat_reports_no_length() {
        assert_eq!(WholeAssets.len("small.txt"), None);
    }

    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn registration_round_trips() {
        let _guard = crate::registry::test_service_guard();
        struct Fake;
        impl BundledAssets for Fake {
            fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
                Ok(path.as_bytes().to_vec())
            }
        }
        set_platform_bundled_assets(Arc::new(Fake));
        assert_eq!(
            bundled_assets().unwrap().read("models/a").unwrap(),
            b"models/a"
        );
        clear_platform_bundled_assets();
        assert!(bundled_assets().is_none());
    }

    #[cfg(not(target_arch = "wasm32"))]
    struct MapAssets(BTreeMap<String, Vec<u8>>);

    #[cfg(not(target_arch = "wasm32"))]
    impl BundledAssets for MapAssets {
        fn read(&self, path: &str) -> Result<Vec<u8>, BundledAssetError> {
            self.0
                .get(path)
                .cloned()
                .ok_or_else(|| BundledAssetError::NotFound(path.to_string()))
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn test_directory() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-output/bundled-assets")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declarative_set_installs_and_detects_current_version() {
        let _guard = crate::registry::test_service_guard();
        let destination = test_directory();
        set_platform_bundled_assets(Arc::new(MapAssets(BTreeMap::from([
            ("models/a.bin".to_string(), vec![1, 2]),
            ("models/nested/b.bin".to_string(), vec![3]),
        ]))));
        let spec = BundledAssetInstallSpec::new("7", &destination)
            .source_root("models")
            .entry(BundledAssetEntry::new("a.bin"))
            .entry(BundledAssetEntry::mapped("nested/b.bin", "b.bin"));
        assert_eq!(
            install_bundled_asset_set(&spec).unwrap(),
            BundledAssetInstallOutcome::Installed
        );
        assert_eq!(std::fs::read(destination.join("a.bin")).unwrap(), [1, 2]);
        assert_eq!(std::fs::read(destination.join("b.bin")).unwrap(), [3]);
        assert_eq!(
            install_bundled_asset_set(&spec).unwrap(),
            BundledAssetInstallOutcome::Current
        );
        std::fs::remove_dir_all(destination).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn declaration_rejects_parent_paths_and_handles_missing_host() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_bundled_assets();
        let invalid = BundledAssetInstallSpec::new("1", test_directory())
            .entry(BundledAssetEntry::new("../outside"));
        assert!(matches!(
            install_bundled_asset_set(&invalid),
            Err(BundledAssetError::InvalidPath(_))
        ));
        let valid = BundledAssetInstallSpec::new("1", test_directory())
            .entry(BundledAssetEntry::new("inside"));
        assert_eq!(
            install_bundled_asset_set(&valid).unwrap(),
            BundledAssetInstallOutcome::Unavailable
        );
    }
}
