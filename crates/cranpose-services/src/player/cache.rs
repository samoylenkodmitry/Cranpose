//! Opt-in byte-bounded LRU media cache for feed preloading and offline playback.

use std::sync::{Arc, atomic::Ordering};

use cranpose_core::collections::map::HashMap;
use parking_lot::Mutex;
use portable_atomic::AtomicU64;
use sha2::{Digest, Sha256};

use crate::media::MediaItem;

/// Error returned by [`MediaCache`] operations.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MediaCacheError {
    /// Filesystem read, write, or directory creation failed.
    #[error("media cache I/O error: {0}")]
    Io(String),
    /// A single media payload exceeded the cache's total `max_bytes` limit.
    #[error("media entry ({size} bytes) exceeds cache budget ({max_bytes} bytes)")]
    EntryTooLarge {
        /// Size of the rejected entry in bytes.
        size: u64,
        /// Configured maximum cache capacity in bytes.
        max_bytes: u64,
    },
}

/// Usage and eviction telemetry snapshot for a [`MediaCache`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MediaCacheStats {
    /// Number of cached media entries currently stored.
    pub entry_count: usize,
    /// Total cached payload bytes currently stored.
    pub total_bytes: u64,
    /// Configured upper byte limit.
    pub max_bytes: u64,
    /// Cumulative cache lookups that found a stored entry.
    pub hits: u64,
    /// Cumulative cache lookups that missed.
    pub misses: u64,
    /// Cumulative entries evicted under LRU byte pressure.
    pub evictions: u64,
}

#[cfg(not(target_arch = "wasm32"))]
mod storage {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::UNIX_EPOCH,
    };

    use super::MediaCacheError;
    use crate::media::{MediaItem, uri_for_path};

    pub(super) struct DiskStore {
        dir: PathBuf,
    }

    pub(super) struct StoredEntry {
        pub(super) path: PathBuf,
        pub(super) byte_len: u64,
        pub(super) last_access: u64,
    }

    impl DiskStore {
        pub(super) fn open(
            dir: PathBuf,
        ) -> Result<(Self, Vec<(String, StoredEntry)>), MediaCacheError> {
            fs::create_dir_all(&dir).map_err(|e| MediaCacheError::Io(e.to_string()))?;
            let mut entries = Vec::new();
            let read_dir = fs::read_dir(&dir).map_err(|e| MediaCacheError::Io(e.to_string()))?;
            for entry in read_dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("cache") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                if !meta.is_file() {
                    continue;
                }
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                entries.push((
                    stem.to_owned(),
                    StoredEntry {
                        path,
                        byte_len: meta.len(),
                        last_access: mtime,
                    },
                ));
            }
            Ok((Self { dir }, entries))
        }

        pub(super) fn write_entry(
            &self,
            key: &str,
            bytes: &[u8],
            access: u64,
        ) -> Result<StoredEntry, MediaCacheError> {
            let final_path = self.dir.join(format!("{key}.cache"));
            let tmp_path = self.dir.join(format!("{key}.{access}.tmp"));
            fs::write(&tmp_path, bytes).map_err(|e| MediaCacheError::Io(e.to_string()))?;
            if let Err(err) = fs::rename(&tmp_path, &final_path) {
                if let Err(remove_err) = fs::remove_file(&tmp_path) {
                    log::debug!(
                        "media cache: cleanup {} failed: {remove_err}",
                        tmp_path.display()
                    );
                }
                return Err(MediaCacheError::Io(err.to_string()));
            }
            Ok(StoredEntry {
                path: final_path,
                byte_len: bytes.len() as u64,
                last_access: access,
            })
        }

        pub(super) fn remove_entry(&self, entry: &StoredEntry) {
            if let Err(err) = fs::remove_file(&entry.path)
                && err.kind() != std::io::ErrorKind::NotFound
            {
                log::debug!("media cache: remove {} failed: {err}", entry.path.display());
            }
        }

        pub(super) fn resolve_item(entry: &StoredEntry, original: &MediaItem) -> MediaItem {
            let mut resolved = original.clone();
            resolved.uri = uri_for_path(&entry.path);
            resolved.bytes = None;
            resolved
        }

        pub(super) fn directory(&self) -> &Path {
            &self.dir
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod storage {
    use std::sync::Arc;

    use super::MediaCacheError;
    use crate::media::MediaItem;

    pub(super) struct DiskStore;

    pub(super) struct StoredEntry {
        pub(super) bytes: Arc<[u8]>,
        pub(super) byte_len: u64,
        pub(super) last_access: u64,
    }

    impl DiskStore {
        pub(super) fn write_entry(
            &self,
            _key: &str,
            bytes: &[u8],
            access: u64,
        ) -> Result<StoredEntry, MediaCacheError> {
            Ok(StoredEntry {
                bytes: Arc::from(bytes),
                byte_len: bytes.len() as u64,
                last_access: access,
            })
        }

        pub(super) fn remove_entry(&self, _entry: &StoredEntry) {}

        pub(super) fn resolve_item(entry: &StoredEntry, original: &MediaItem) -> MediaItem {
            let mut resolved = original.clone();
            resolved.bytes = Some(Arc::clone(&entry.bytes));
            resolved
        }
    }
}

struct CacheState {
    entries: HashMap<String, storage::StoredEntry>,
    total_bytes: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

struct CacheInner {
    store: storage::DiskStore,
    max_bytes: u64,
    clock: AtomicU64,
    state: Mutex<CacheState>,
}

/// Opt-in byte-bounded LRU media cache supporting feed preloading and offline playback.
///
/// On native platforms, cached media payloads persist across restarts under the configured cache
/// directory using atomic write-and-rename and resolve to `file://` [`MediaItem`]s. On `wasm32`,
/// entries are held in a byte-bounded in-memory LRU store resolving to in-memory [`MediaItem`]s.
#[derive(Clone)]
pub struct MediaCache {
    inner: Arc<CacheInner>,
}

impl PartialEq for MediaCache {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for MediaCache {}

impl MediaCache {
    /// Opens or creates a persistent disk-backed [`MediaCache`] under `directory` capped at
    /// `max_bytes`, immediately evicting least-recently-used files if existing entries exceed the
    /// budget.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(
        directory: impl Into<std::path::PathBuf>,
        max_bytes: u64,
    ) -> Result<Self, MediaCacheError> {
        let (store, existing) = storage::DiskStore::open(directory.into())?;
        let mut entries = HashMap::default();
        entries.reserve(existing.len());
        let mut total_bytes: u64 = 0;
        let mut max_access: u64 = 1;
        for (key, entry) in existing {
            total_bytes = total_bytes.saturating_add(entry.byte_len);
            max_access = max_access.max(entry.last_access);
            entries.insert(key, entry);
        }
        let cache = Self {
            inner: Arc::new(CacheInner {
                store,
                max_bytes,
                clock: AtomicU64::new(max_access.saturating_add(1)),
                state: Mutex::new(CacheState {
                    entries,
                    total_bytes,
                    hits: 0,
                    misses: 0,
                    evictions: 0,
                }),
            }),
        };
        cache.evict_to_budget();
        Ok(cache)
    }

    /// Creates an in-memory byte-bounded [`MediaCache`] on `wasm32`.
    #[cfg(target_arch = "wasm32")]
    pub fn in_memory(max_bytes: u64) -> Self {
        Self {
            inner: Arc::new(CacheInner {
                store: storage::DiskStore,
                max_bytes,
                clock: AtomicU64::new(1),
                state: Mutex::new(CacheState {
                    entries: HashMap::default(),
                    total_bytes: 0,
                    hits: 0,
                    misses: 0,
                    evictions: 0,
                }),
            }),
        }
    }

    /// Cache directory path on native targets.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn directory(&self) -> &std::path::Path {
        self.inner.store.directory()
    }

    /// Stores `bytes` for `uri`, evicting least-recently-used entries as needed to stay within
    /// `max_bytes`.
    pub fn put_bytes(&self, uri: &str, bytes: &[u8]) -> Result<(), MediaCacheError> {
        let size = bytes.len() as u64;
        if size > self.inner.max_bytes {
            return Err(MediaCacheError::EntryTooLarge {
                size,
                max_bytes: self.inner.max_bytes,
            });
        }
        let key = cache_key(uri);
        let access = self.inner.clock.fetch_add(1, Ordering::AcqRel);
        let entry = self.inner.store.write_entry(&key, bytes, access)?;
        {
            let mut state = self.inner.state.lock();
            if let Some(previous) = state.entries.insert(key, entry) {
                state.total_bytes = state.total_bytes.saturating_sub(previous.byte_len);
            }
            state.total_bytes = state.total_bytes.saturating_add(size);
        }
        self.evict_to_budget();
        Ok(())
    }

    /// Whether `uri` is currently stored in the cache (does not alter hit/miss counters).
    pub fn contains(&self, uri: &str) -> bool {
        let key = cache_key(uri);
        self.inner.state.lock().entries.contains_key(&key)
    }

    /// Returns the local cached file path for `uri`, updating its LRU recency.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn lookup_path(&self, uri: &str) -> Option<std::path::PathBuf> {
        let key = cache_key(uri);
        let access = self.inner.clock.fetch_add(1, Ordering::AcqRel);
        let mut state = self.inner.state.lock();
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.last_access = access;
            let path = entry.path.clone();
            state.hits = state.hits.saturating_add(1);
            Some(path)
        } else {
            state.misses = state.misses.saturating_add(1);
            None
        }
    }

    /// Resolves `item` against the cache: on a cache hit, returns a [`MediaItem`] backed by the
    /// cached local file or bytes while preserving metadata, MIME type, clipping, and subtitles;
    /// on a miss, returns `item` cloned unchanged.
    pub fn resolve_media_item(&self, item: &MediaItem) -> MediaItem {
        let key = cache_key(&item.uri);
        let access = self.inner.clock.fetch_add(1, Ordering::AcqRel);
        let mut state = self.inner.state.lock();
        if let Some(entry) = state.entries.get_mut(&key) {
            entry.last_access = access;
            let resolved = storage::DiskStore::resolve_item(entry, item);
            state.hits = state.hits.saturating_add(1);
            drop(state);
            resolved
        } else {
            state.misses = state.misses.saturating_add(1);
            drop(state);
            item.clone()
        }
    }

    /// Removes the cached entry for `uri`, if present.
    pub fn remove(&self, uri: &str) {
        let key = cache_key(uri);
        let mut state = self.inner.state.lock();
        let removed = state.entries.remove(&key);
        if let Some(ref entry) = removed {
            state.total_bytes = state.total_bytes.saturating_sub(entry.byte_len);
        }
        drop(state);
        if let Some(entry) = removed {
            self.inner.store.remove_entry(&entry);
        }
    }

    /// Removes all cached entries and resets total stored bytes to zero.
    pub fn clear(&self) {
        let mut state = self.inner.state.lock();
        state.total_bytes = 0;
        let drained: Vec<storage::StoredEntry> =
            state.entries.drain().map(|(_, entry)| entry).collect();
        drop(state);
        for entry in drained {
            self.inner.store.remove_entry(&entry);
        }
    }

    /// Current entry count, stored byte total, hit/miss, and eviction counters.
    pub fn stats(&self) -> MediaCacheStats {
        let state = self.inner.state.lock();
        MediaCacheStats {
            entry_count: state.entries.len(),
            total_bytes: state.total_bytes,
            max_bytes: self.inner.max_bytes,
            hits: state.hits,
            misses: state.misses,
            evictions: state.evictions,
        }
    }

    fn evict_to_budget(&self) {
        let mut evicted = Vec::new();
        let mut state = self.inner.state.lock();
        while state.total_bytes > self.inner.max_bytes {
            let oldest = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(k, _)| k.clone());
            let Some(oldest_key) = oldest else {
                break;
            };
            if let Some(entry) = state.entries.remove(&oldest_key) {
                state.total_bytes = state.total_bytes.saturating_sub(entry.byte_len);
                state.evictions = state.evictions.saturating_add(1);
                evicted.push(entry);
            }
        }
        drop(state);
        for entry in evicted {
            self.inner.store.remove_entry(&entry);
        }
    }
}

fn cache_key(uri: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(uri.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push(char::from(HEX[usize::from(byte >> 4)]));
        hex.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    hex
}
