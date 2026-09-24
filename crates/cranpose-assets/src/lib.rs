//! Asset loading and management primitives for Cranpose.

use std::{
    collections::HashMap,
    fmt,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError},
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
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
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
#[path = "tests/assets_tests.rs"]
mod tests;
