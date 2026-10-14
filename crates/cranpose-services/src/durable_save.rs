use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use crate::preferences::{PreferencesError, PreferencesRef};

/// Result of a lifecycle or explicit save boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableSaveOutcome {
    /// No work was registered.
    Nothing,
    /// Every callback and preferences batch succeeded.
    Completed,
    /// A callback panicked, a worker could not start, or a write failed.
    Failed,
    /// The deadline expired. Running work can finish under its background lease.
    TimedOut,
}

/// An error reported by a durable-save callback.
#[derive(Clone, Debug, thiserror::Error)]
pub enum DurableSaveError {
    /// A preferences backend rejected a write.
    #[error(transparent)]
    Preferences(#[from] PreferencesError),
    /// Application-specific durable storage failed.
    #[error("durable save failed: {0}")]
    Storage(String),
}

type SaveCallback = dyn Fn() -> Result<(), DurableSaveError> + Send + Sync;
type PreferenceCallback = dyn Fn() -> String + Send + Sync;

enum SaveWork {
    Callback(Box<SaveCallback>),
    Preference {
        store: PreferencesRef,
        key: String,
        read: Box<PreferenceCallback>,
    },
}

static NEXT_SAVE: AtomicU64 = AtomicU64::new(1);
type Saves = BTreeMap<u64, Arc<SaveWork>>;

fn saves() -> &'static Mutex<Saves> {
    static SAVES: OnceLock<Mutex<Saves>> = OnceLock::new();
    SAVES.get_or_init(Mutex::default)
}

/// Keeps a shared durable save registered until its owner is dropped.
pub struct DurableSaveRegistration {
    id: u64,
}

impl Drop for DurableSaveRegistration {
    fn drop(&mut self) {
        let removed = saves()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.id);
        drop(removed);
    }
}

fn register(work: SaveWork) -> DurableSaveRegistration {
    let id = NEXT_SAVE.fetch_add(1, Ordering::Relaxed);
    saves()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, Arc::new(work));
    DurableSaveRegistration { id }
}

/// Stops the shared saves of `store` keys that `keys` forgets, such as a
/// popped back-stack entry's saved state that a view model still holds.
fn stop_preference_saves(store: &PreferencesRef, keys: &local_saves::Forget) {
    let stopped: Vec<_> = saves()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extract_if(.., |_, work| match &**work {
            SaveWork::Preference {
                store: saved, key, ..
            } => Arc::ptr_eq(saved, store) && keys.matches(key),
            SaveWork::Callback(_) => false,
        })
        .collect();
    drop(stopped);
}

/// Registers fallible work for the next save boundary. Callbacks run serially;
/// timed-out runs are allowed to finish before another run can start.
pub fn register_durable_save(
    save: impl Fn() -> Result<(), DurableSaveError> + Send + Sync + 'static,
) -> DurableSaveRegistration {
    register(SaveWork::Callback(Box::new(save)))
}

/// Registers a value to be sampled when saving starts and batched with other
/// keys using the same store. The callback must not perform storage I/O.
pub fn register_preference_save(
    store: PreferencesRef,
    key: String,
    read: impl Fn() -> String + Send + Sync + 'static,
) -> DurableSaveRegistration {
    register(SaveWork::Preference {
        store,
        key,
        read: Box::new(read),
    })
}

/// Registers fallible durable work for this composition's lifetime.
#[expect(non_snake_case)]
#[track_caller]
pub fn DurableSaveEffect<K: PartialEq + 'static>(
    keys: K,
    save: impl Fn() -> Result<(), DurableSaveError> + Send + Sync + 'static,
) {
    cranpose_core::__disposable_effect_impl(
        cranpose_core::caller_location_key()
            ^ cranpose_core::location_key(file!(), line!(), column!()),
        keys,
        move |scope| {
            let registration = register_durable_save(save);
            scope.on_dispose(move || drop(registration))
        },
    );
}

mod local_saves;

pub use local_saves::SaveableRegistration;
pub(crate) use local_saves::{LocalSaves, forget, register as register_local, saved};

struct PreferenceBatch {
    store: PreferencesRef,
    values: BTreeMap<String, String>,
    forget: Vec<Arc<local_saves::Forget>>,
}

impl PreferenceBatch {
    fn removed_keys(&self) -> Vec<String> {
        if self.forget.is_empty() {
            return Vec::new();
        }
        let mut keys = self.store.keys();
        keys.retain(|key| self.forget.iter().any(|forget| forget.matches(key)));
        keys
    }
}

#[derive(Default)]
struct SaveBatch {
    preferences: Vec<PreferenceBatch>,
    callbacks: Vec<Arc<SaveWork>>,
}

impl SaveBatch {
    fn for_store(&mut self, store: &PreferencesRef) -> &mut PreferenceBatch {
        let index = match self
            .preferences
            .iter()
            .position(|batch| Arc::ptr_eq(&batch.store, store))
        {
            Some(index) => index,
            None => {
                self.preferences.push(PreferenceBatch {
                    store: Arc::clone(store),
                    values: BTreeMap::new(),
                    forget: Vec::new(),
                });
                self.preferences.len() - 1
            }
        };
        &mut self.preferences[index]
    }

    fn push(&mut self, store: &PreferencesRef, key: String, value: String) {
        self.for_store(store).values.insert(key, value);
    }

    fn forget(&mut self, store: &PreferencesRef, keys: Arc<local_saves::Forget>) {
        self.for_store(store).forget.push(keys);
    }

    /// The work to do now, and what of this thread's saves it covers.
    fn collect() -> (Self, local_saves::Sampled) {
        let mut batch = Self {
            callbacks: saves()
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .cloned()
                .collect(),
            ..Self::default()
        };
        let sampled = local_saves::sample(&mut batch);
        (batch, sampled)
    }

    fn is_empty(&self) -> bool {
        self.preferences.is_empty() && self.callbacks.is_empty()
    }

    fn run(mut self) -> DurableSaveOutcome {
        let mut succeeded = true;
        for work in std::mem::take(&mut self.callbacks) {
            succeeded &= match &*work {
                SaveWork::Callback(save) => run_callback(save),
                SaveWork::Preference { store, key, read } => {
                    match catch_unwind(AssertUnwindSafe(read)) {
                        Ok(value) => {
                            self.push(store, key.clone(), value);
                            true
                        }
                        Err(_) => false,
                    }
                }
            };
        }
        for batch in self.preferences {
            succeeded &= run_callback(|| {
                batch
                    .store
                    .update_many(&batch.values, &batch.removed_keys())?;
                batch.store.flush()?;
                Ok(())
            });
        }
        if succeeded {
            DurableSaveOutcome::Completed
        } else {
            DurableSaveOutcome::Failed
        }
    }
}

fn run_callback(save: impl FnOnce() -> Result<(), DurableSaveError>) -> bool {
    match catch_unwind(AssertUnwindSafe(save)) {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            log::warn!("cranpose: {error}");
            false
        }
        Err(_) => {
            log::warn!("cranpose: durable save panicked");
            false
        }
    }
}

/// Captures composition-local values on the calling thread and saves registered
/// values in one serial batch. Call this on each composition's owning thread.
///
/// Native runs use at most one worker with no queued batches. A concurrent call
/// waits within its budget before sampling fresh values. A timeout does not
/// cancel a running write. Retry a failed or timed-out boundary before relying
/// on restoration. Web writes execute synchronously on the browser thread.
#[inline(never)]
pub fn run_durable_saves(deadline: Duration) -> DurableSaveOutcome {
    #[cfg(not(target_arch = "wasm32"))]
    {
        executor::run(deadline)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let started = web_time::Instant::now();
        let (batch, sampled) = SaveBatch::collect();
        if batch.is_empty() {
            return DurableSaveOutcome::Nothing;
        }
        let result = batch.run();
        if result == DurableSaveOutcome::Completed {
            sampled.completed();
        }
        if result == DurableSaveOutcome::Completed && started.elapsed() > deadline {
            DurableSaveOutcome::TimedOut
        } else {
            result
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod executor;
