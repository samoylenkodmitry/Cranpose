//! Read-only assets packaged with the application.

#[cfg(not(target_arch = "wasm32"))]
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use crate::registry::ServiceRegistry;

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
///
/// Reads are synchronous, which is what a packaged file on a native platform
/// is: Android's asset manager, an application bundle and a directory beside
/// the executable all answer without waiting on a network. A browser has no
/// such file — its resources arrive over HTTP — so the web registers no backend
/// and [`bundled_assets`] answers `None` there. An application that ships
/// resources to the web fetches them through [`crate::http`], which is async
/// because that is what the medium is.
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
        Ok(Box::new(StreamingAssetReader::new(
            path,
            std::io::Cursor::new(self.read(path)?),
        )))
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

/// A [`BundledAssetReader`] over any byte stream.
///
/// Every backend that streams an asset ends at the same loop: fill a chunk,
/// stop at the end, name the asset rather than its resolved location when the
/// read fails. What differs is only what was opened — a file beside the
/// executable, a file in an application bundle, a span of an Android package,
/// or bytes already in hand — and whether the asset ends where its stream
/// does. The loop lives here once; a backend supplies the stream and, when
/// its asset is a span of something larger, how many bytes of it are its own.
pub struct StreamingAssetReader<R> {
    source: R,
    /// The asset the caller asked for, so a failure names that rather than
    /// wherever it happened to resolve to.
    path: String,
    /// Bytes left when the asset ends before its stream does; `None` when the
    /// stream's own end is the asset's end.
    remaining: Option<u64>,
}

impl<R: std::io::Read + Send> StreamingAssetReader<R> {
    /// A reader over a stream whose end is the asset's end.
    pub fn new(path: impl Into<String>, source: R) -> Self {
        Self {
            source,
            path: path.into(),
            remaining: None,
        }
    }

    /// A reader over `len` bytes of a stream that continues past the asset —
    /// an Android package's file descriptor addresses the whole package, so
    /// the asset's end is a count rather than end of file.
    pub fn with_length(path: impl Into<String>, source: R, len: u64) -> Self {
        Self {
            source,
            path: path.into(),
            remaining: Some(len),
        }
    }
}

impl<R: std::io::Read + Send> BundledAssetReader for StreamingAssetReader<R> {
    fn read_chunk(&mut self) -> Result<Option<Vec<u8>>, BundledAssetError> {
        let want = match self.remaining {
            Some(0) => return Ok(None),
            Some(remaining) => remaining.min(crate::content::DEFAULT_CHUNK_LEN as u64) as usize,
            None => crate::content::DEFAULT_CHUNK_LEN,
        };
        let mut chunk = vec![0u8; want];
        let mut filled = 0;
        while filled < chunk.len() {
            match self.source.read(&mut chunk[filled..]) {
                Ok(0) => break,
                Ok(read) => filled += read,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    return Err(BundledAssetError::ReadFailed {
                        path: self.path.clone(),
                        message: error.to_string(),
                    })
                }
            }
        }
        if filled == 0 {
            // A stream that ends early ends the asset with it; recording that
            // stops the next call re-reading a source with nothing left.
            self.remaining = Some(0);
            return Ok(None);
        }
        chunk.truncate(filled);
        if let Some(remaining) = &mut self.remaining {
            *remaining -= filled as u64;
        }
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

    /// The Android package path opens a descriptor onto the whole APK and
    /// tells the reader how many of its bytes belong to this asset. Reading
    /// past that count would hand the caller the next asset's bytes; stopping
    /// short of it would truncate this one. Neither is visible without a
    /// stream that continues past the asset, which is what this gives it.
    #[test]
    fn a_length_bounded_reader_stops_at_the_asset_and_not_at_the_stream() {
        let asset_len = crate::content::DEFAULT_CHUNK_LEN + 5;
        let mut package = vec![7u8; asset_len];
        package.extend_from_slice(&[9u8; 64]);

        let mut reader = StreamingAssetReader::with_length(
            "model.bin",
            std::io::Cursor::new(package),
            asset_len as u64,
        );
        let mut read = Vec::new();
        while let Some(chunk) = reader.read_chunk().expect("chunks read") {
            read.extend_from_slice(&chunk);
        }

        assert_eq!(
            read.len(),
            asset_len,
            "the asset ends where its length says"
        );
        assert!(
            read.iter().all(|byte| *byte == 7),
            "no byte of what follows the asset in the package is handed out"
        );
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

    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicU64, Ordering},
    };

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
