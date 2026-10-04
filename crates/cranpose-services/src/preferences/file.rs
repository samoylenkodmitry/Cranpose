#[cfg(unix)]
use std::fs::File;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

use super::{PreferencesError, PreferencesStore, encode, parse};
use crate::host::application_directories;

/// File-backed preferences with coordinated, atomic writes within this process.
///
/// Instances opening the same canonical path share their values and writer.
/// Reads include pending changes after a failed write; [`PreferencesStore::flush`]
/// retries them. A successful write synchronizes the file before replacing it.
/// Concurrent access from other processes is not supported.
pub struct FilePreferences {
    path: Option<PathBuf>,
    file: OnceLock<Arc<Mutex<FileState>>>,
}

struct FileState {
    path: PathBuf,
    entries: BTreeMap<String, String>,
    dirty: bool,
}

type Files = BTreeMap<PathBuf, Weak<Mutex<FileState>>>;

fn io(error: std::io::Error) -> PreferencesError {
    PreferencesError::Io(error.to_string())
}

impl Default for FilePreferences {
    fn default() -> Self {
        Self::new()
    }
}

impl FilePreferences {
    /// Lazily opens `preferences` in the application's config directory.
    pub fn new() -> Self {
        Self {
            path: None,
            file: OnceLock::new(),
        }
    }

    /// Lazily opens an explicit preferences file, creating its parent directory.
    pub fn at_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            file: OnceLock::new(),
        }
    }

    fn file(&self) -> Result<&Mutex<FileState>, PreferencesError> {
        if let Some(file) = self.file.get() {
            return Ok(file);
        }
        let path = match &self.path {
            Some(path) => path.clone(),
            None => application_directories()
                .map_err(|error| PreferencesError::Io(error.to_string()))?
                .config
                .join("preferences"),
        };
        let path = canonical_file(&path)?;
        static FILES: OnceLock<Mutex<Files>> = OnceLock::new();
        let mut files = FILES
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| PreferencesError::Io("preferences registry poisoned".into()))?;
        files.retain(|_, file| file.strong_count() != 0);
        let file = match files.get(&path).and_then(Weak::upgrade) {
            Some(file) => file,
            None => {
                let text = match fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
                    Err(error) => return Err(io(error)),
                };
                let file = Arc::new(Mutex::new(FileState {
                    path: path.clone(),
                    entries: parse(&text),
                    dirty: false,
                }));
                files.insert(path, Arc::downgrade(&file));
                file
            }
        };
        let _ = self.file.set(file);
        self.file
            .get()
            .map(Arc::as_ref)
            .ok_or_else(|| PreferencesError::Io("preferences initialization failed".into()))
    }

    fn with_entries<R>(
        &self,
        body: impl FnOnce(&mut FileState) -> Result<R, PreferencesError>,
    ) -> Result<R, PreferencesError> {
        let mut file = self
            .file()?
            .lock()
            .map_err(|_| PreferencesError::Io("preferences lock poisoned".into()))?;
        body(&mut file)
    }

    fn mutate(
        &self,
        body: impl FnOnce(&mut BTreeMap<String, String>) -> bool,
    ) -> Result<(), PreferencesError> {
        self.with_entries(|file| {
            file.dirty |= body(&mut file.entries);
            file.flush()
        })
    }
}

fn canonical_file(path: &Path) -> Result<PathBuf, PreferencesError> {
    if path.try_exists().map_err(io)? {
        return fs::canonicalize(path).map_err(io);
    }
    let name = path
        .file_name()
        .ok_or_else(|| PreferencesError::Io("preferences path must name a file".into()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(io)?;
    Ok(fs::canonicalize(parent).map_err(io)?.join(name))
}

impl FileState {
    fn flush(&mut self) -> Result<(), PreferencesError> {
        if !self.dirty {
            return Ok(());
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| PreferencesError::Io("preferences parent is missing".into()))?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(io)?;
        file.write_all(encode(&self.entries).as_bytes())
            .map_err(io)?;
        file.as_file().sync_all().map_err(io)?;
        file.persist(&self.path).map_err(|error| io(error.error))?;
        #[cfg(unix)]
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(io)?;
        self.dirty = false;
        Ok(())
    }
}

impl PreferencesStore for FilePreferences {
    fn get(&self, key: &str) -> Option<String> {
        self.with_entries(|file| Ok(file.entries.get(key).cloned()))
            .ok()
            .flatten()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError> {
        self.mutate(|entries| update(entries, key, value))
    }

    fn set_many(&self, values: &BTreeMap<String, String>) -> Result<(), PreferencesError> {
        self.mutate(|entries| {
            let mut changed = false;
            for (key, value) in values {
                changed |= update(entries, key, value);
            }
            changed
        })
    }

    fn flush(&self) -> Result<(), PreferencesError> {
        self.with_entries(FileState::flush)
    }

    fn remove(&self, key: &str) -> Result<(), PreferencesError> {
        self.mutate(|entries| entries.remove(key).is_some())
    }

    fn keys(&self) -> Vec<String> {
        self.with_entries(|file| Ok(file.entries.keys().cloned().collect()))
            .unwrap_or_default()
    }

    fn clear(&self) -> Result<(), PreferencesError> {
        self.mutate(|entries| {
            let changed = !entries.is_empty();
            entries.clear();
            changed
        })
    }
}

fn update(entries: &mut BTreeMap<String, String>, key: &str, value: &str) -> bool {
    if entries.get(key).is_some_and(|old| old == value) {
        return false;
    }
    entries.insert(key.to_owned(), value.to_owned());
    true
}
