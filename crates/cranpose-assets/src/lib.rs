//! Asset loading and management primitives for Cranpose.

use std::{
    collections::HashMap,
    fmt,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

/// Error returned by [`AssetManager`] load operations.
#[derive(Debug)]
pub enum AssetError {
    /// The requested asset path is empty.
    EmptyPath,
    /// Asset paths must be relative to a registered root.
    AbsolutePath { path: PathBuf },
    /// Asset paths cannot contain parent-directory components.
    EscapesRoot { path: PathBuf },
    /// No registered root contained the requested asset.
    NotFound { path: PathBuf, roots: Vec<PathBuf> },
    /// The asset file could not be read.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The asset bytes are not valid UTF-8.
    Utf8 {
        path: PathBuf,
        source: std::str::Utf8Error,
    },
}

impl fmt::Display for AssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssetError::EmptyPath => formatter.write_str("asset path is empty"),
            AssetError::AbsolutePath { path } => {
                write!(formatter, "asset path must be relative: {}", path.display())
            }
            AssetError::EscapesRoot { path } => {
                write!(
                    formatter,
                    "asset path cannot escape registered roots: {}",
                    path.display()
                )
            }
            AssetError::NotFound { path, roots } => {
                write!(formatter, "asset not found: {}", path.display())?;
                if !roots.is_empty() {
                    write!(formatter, " under")?;
                    for root in roots {
                        write!(formatter, " {}", root.display())?;
                    }
                }
                Ok(())
            }
            AssetError::Io { path, source } => {
                write!(
                    formatter,
                    "failed to read asset {}: {source}",
                    path.display()
                )
            }
            AssetError::Utf8 { path, source } => {
                write!(
                    formatter,
                    "asset {} is not valid UTF-8: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for AssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AssetError::Io { source, .. } => Some(source),
            AssetError::Utf8 { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Synchronous asset manager with root-based lookup and byte caching.
#[derive(Debug)]
pub struct AssetManager {
    roots: Vec<PathBuf>,
    cache: Mutex<HashMap<PathBuf, Arc<[u8]>>>,
}

impl AssetManager {
    /// Creates an asset manager rooted at the current working directory.
    pub fn new() -> Self {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::with_root(root)
    }

    /// Creates an asset manager with a single asset root.
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self {
            roots: vec![root.into()],
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Adds another lookup root. Earlier roots win when several contain the same path.
    pub fn add_root(&mut self, root: impl Into<PathBuf>) {
        self.roots.push(root.into());
    }

    /// Returns the registered asset roots in lookup order.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Loads an asset as shared bytes.
    pub fn load_bytes(&self, path: impl AsRef<Path>) -> Result<Arc<[u8]>, AssetError> {
        let key = normalize_asset_path(path.as_ref())?;
        if let Some(bytes) = self.cache().get(&key).cloned() {
            return Ok(bytes);
        }

        let resolved = self
            .resolve_existing(&key)
            .ok_or_else(|| AssetError::NotFound {
                path: key.clone(),
                roots: self.roots.clone(),
            })?;
        let bytes = std::fs::read(&resolved).map_err(|source| AssetError::Io {
            path: resolved,
            source,
        })?;
        let bytes = Arc::<[u8]>::from(bytes);
        self.cache().insert(key, Arc::clone(&bytes));
        Ok(bytes)
    }

    /// Loads a UTF-8 text asset.
    pub fn load_string(&self, path: impl AsRef<Path>) -> Result<String, AssetError> {
        let key = normalize_asset_path(path.as_ref())?;
        let bytes = self.load_bytes(&key)?;
        std::str::from_utf8(&bytes)
            .map(str::to_owned)
            .map_err(|source| AssetError::Utf8 { path: key, source })
    }

    /// Removes all cached asset bytes.
    pub fn clear_cache(&self) {
        self.cache().clear();
    }

    /// Returns the number of cached assets.
    pub fn cached_asset_count(&self) -> usize {
        self.cache().len()
    }

    fn resolve_existing(&self, key: &Path) -> Option<PathBuf> {
        self.roots
            .iter()
            .map(|root| root.join(key))
            .find(|candidate| candidate.is_file())
    }

    fn cache(&self) -> MutexGuard<'_, HashMap<PathBuf, Arc<[u8]>>> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_asset_path(path: &Path) -> Result<PathBuf, AssetError> {
    if path.as_os_str().is_empty() {
        return Err(AssetError::EmptyPath);
    }
    if path.is_absolute() {
        return Err(AssetError::AbsolutePath {
            path: path.to_path_buf(),
        });
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(AssetError::EscapesRoot {
                    path: path.to_path_buf(),
                });
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err(AssetError::EmptyPath);
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_asset_root() -> PathBuf {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-output/cranpose-assets");
        std::fs::create_dir_all(&root).expect("create asset test output directory");
        root
    }

    #[test]
    fn an_earlier_root_wins_over_one_added_later() {
        let base = test_asset_root().join("root-order");
        let first = base.join("first");
        let second = base.join("second");
        std::fs::create_dir_all(&first).expect("first root");
        std::fs::create_dir_all(&second).expect("second root");
        std::fs::write(first.join("shared.txt"), b"first").expect("write first");
        std::fs::write(second.join("shared.txt"), b"second").expect("write second");
        std::fs::write(second.join("only.txt"), b"only").expect("write only");

        let mut assets = AssetManager::with_root(&first);
        assets.add_root(&second);
        assert_eq!(assets.roots(), &[first.clone(), second.clone()]);

        assert_eq!(
            assets.load_bytes("shared.txt").expect("shared").as_ref(),
            b"first"
        );
        assert_eq!(
            assets.load_bytes("only.txt").expect("only").as_ref(),
            b"only"
        );
    }

    #[test]
    fn load_bytes_reads_and_caches_relative_asset() {
        let root = test_asset_root();
        std::fs::write(root.join("sample.bin"), [1u8, 2, 3]).expect("write test asset");
        let manager = AssetManager::with_root(&root);

        let first = manager.load_bytes("sample.bin").expect("load asset");
        let second = manager
            .load_bytes("./sample.bin")
            .expect("load cached asset");

        assert_eq!(&*first, &[1, 2, 3]);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(manager.cached_asset_count(), 1);
    }

    #[test]
    fn load_string_returns_utf8_content() {
        let root = test_asset_root();
        std::fs::write(root.join("message.txt"), "hello cranpose").expect("write text asset");
        let manager = AssetManager::with_root(&root);

        assert_eq!(
            manager.load_string("message.txt").expect("load text asset"),
            "hello cranpose"
        );
    }

    #[test]
    fn load_rejects_paths_that_escape_root() {
        let manager = AssetManager::with_root(test_asset_root());

        assert!(matches!(
            manager.load_bytes("../outside.bin"),
            Err(AssetError::EscapesRoot { .. })
        ));
    }

    #[test]
    fn load_reports_missing_asset_with_roots() {
        let root = test_asset_root();
        let manager = AssetManager::with_root(&root);

        match manager.load_bytes("missing.bin") {
            Err(AssetError::NotFound { path, roots }) => {
                assert_eq!(path, PathBuf::from("missing.bin"));
                assert_eq!(roots, vec![root]);
            }
            other => panic!("expected not found, got {other:?}"),
        }
    }

    #[test]
    fn clear_cache_removes_cached_assets() {
        let root = test_asset_root();
        std::fs::write(root.join("clear.bin"), [9u8]).expect("write test asset");
        let manager = AssetManager::with_root(&root);

        manager.load_bytes("clear.bin").expect("load asset");
        assert_eq!(manager.cached_asset_count(), 1);
        manager.clear_cache();
        assert_eq!(manager.cached_asset_count(), 0);
    }
}
