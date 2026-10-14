//! The saves of this thread's compositions: what `rememberSaveable` and
//! saveable scopes register, the last values of saves that left the
//! composition, and the saved keys waiting to be removed.

use std::{collections::BTreeMap, rc::Rc, sync::Arc};

use super::{NEXT_SAVE, Ordering, SaveBatch};
use crate::preferences::PreferencesRef;

struct LocalSave {
    /// `None` keeps the value only in memory.
    store: Option<PreferencesRef>,
    key: String,
    read: Box<dyn Fn() -> Option<String>>,
}

#[derive(Default)]
pub(crate) struct LocalSaves {
    live: BTreeMap<u64, Rc<LocalSave>>,
    /// The last values of saves whose composables left the composition
    /// before their store held them, by key. A composable that comes back
    /// restores what it had. A completed save boundary writes the durable
    /// ones and forgets them.
    retired: BTreeMap<String, RetiredSave>,
    /// Saved keys to remove from their stores at the next save boundary.
    forgotten: Vec<(PreferencesRef, Arc<Forget>)>,
}

struct RetiredSave {
    store: Option<PreferencesRef>,
    value: String,
}

/// The keys under `prefix`, apart from those under one of `keep`.
pub(super) struct Forget {
    prefix: String,
    keep: Vec<String>,
}

impl Forget {
    pub(super) fn matches(&self, key: &str) -> bool {
        key.starts_with(&self.prefix) && !self.keep.iter().any(|kept| key.starts_with(kept))
    }
}

fn same_store(a: Option<&PreferencesRef>, b: Option<&PreferencesRef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

impl LocalSaves {
    fn forgotten(&self, store: &PreferencesRef, key: &str) -> bool {
        self.forgotten
            .iter()
            .any(|(forgotten, keys)| Arc::ptr_eq(forgotten, store) && keys.matches(key))
    }
}

/// Keeps a composable's value saved while it lives, and its last value
/// once it is dropped. It belongs to the thread that registered it.
pub struct SaveableRegistration {
    id: u64,
    thread: std::marker::PhantomData<Rc<()>>,
}

impl Drop for SaveableRegistration {
    fn drop(&mut self) {
        crate::thread_state::try_with(|state| {
            let removed = state.local_saves.borrow_mut().live.remove(&self.id);
            let Some(save) = removed else {
                return;
            };
            let Some(value) = (save.read)() else {
                return;
            };
            let stored = save.store.as_ref().is_some_and(|store| {
                !state.local_saves.borrow().forgotten(store, &save.key)
                    && store.get(&save.key).as_deref() == Some(value.as_str())
            });
            if !stored {
                state.local_saves.borrow_mut().retired.insert(
                    save.key.clone(),
                    RetiredSave {
                        store: save.store.clone(),
                        value,
                    },
                );
            }
        });
    }
}

/// Saves what `read` returns under `key` in `store`, or only in memory when
/// `store` is `None`, while the registration lives.
pub(crate) fn register(
    store: Option<PreferencesRef>,
    key: String,
    read: impl Fn() -> Option<String> + 'static,
) -> SaveableRegistration {
    let id = NEXT_SAVE.fetch_add(1, Ordering::Relaxed);
    crate::thread_state::with(|state| {
        let mut saves = state.local_saves.borrow_mut();
        saves.retired.remove(&key);
        saves.live.insert(
            id,
            Rc::new(LocalSave {
                store,
                key,
                read: Box::new(read),
            }),
        )
    });
    SaveableRegistration {
        id,
        thread: std::marker::PhantomData,
    }
}

/// The value saved under `key`: the last value a save left when it left the
/// composition, else the stored one unless it waits to be removed.
pub(crate) fn saved(store: Option<&PreferencesRef>, key: &str) -> Option<String> {
    let (retired, forgotten) = crate::thread_state::with(|state| {
        let saves = state.local_saves.borrow();
        (
            saves
                .retired
                .get(key)
                .filter(|retired| same_store(retired.store.as_ref(), store))
                .map(|retired| retired.value.clone()),
            store.is_some_and(|store| saves.forgotten(store, key)),
        )
    });
    if retired.is_some() {
        return retired;
    }
    store
        .filter(|_| !forgotten)
        .and_then(|store| store.get(key))
}

/// Forgets the saves under `prefix`, apart from those under one of `keep`:
/// live ones stop saving, the last values of dropped ones are discarded and
/// stored ones are removed from `store` at the next save boundary.
pub(crate) fn forget(store: Option<PreferencesRef>, prefix: String, keep: Vec<String>) {
    let keys = Arc::new(Forget { prefix, keep });
    if let Some(store) = &store {
        super::stop_preference_saves(store, &keys);
    }
    let stopped: Vec<_> = crate::thread_state::with(|state| {
        let mut saves = state.local_saves.borrow_mut();
        saves.retired.retain(|key, _| !keys.matches(key));
        if let Some(store) = store {
            saves.forgotten.push((store, Arc::clone(&keys)));
        }
        saves
            .live
            .extract_if(.., |_, save| keys.matches(&save.key))
            .collect()
    });
    drop(stopped);
}

/// What a save boundary took from this thread's saves, so that a completed
/// boundary forgets what it made obsolete.
pub(crate) struct Sampled {
    retired: Vec<(String, String)>,
    forgotten: usize,
}

/// Adds this thread's durable saves to `batch`.
pub(super) fn sample(batch: &mut SaveBatch) -> Sampled {
    let (live, retired, forgotten) = crate::thread_state::with(|state| {
        let saves = state.local_saves.borrow();
        (
            saves
                .live
                .values()
                .filter(|save| save.store.is_some())
                .cloned()
                .collect::<Vec<_>>(),
            saves
                .retired
                .iter()
                .filter_map(|(key, retired)| {
                    let store = retired.store.as_ref()?;
                    Some((Arc::clone(store), key.clone(), retired.value.clone()))
                })
                .collect::<Vec<_>>(),
            saves.forgotten.clone(),
        )
    });
    let sampled_forgotten = forgotten.len();
    for (store, keys) in forgotten {
        batch.forget(&store, keys);
    }
    let mut written = Vec::with_capacity(retired.len());
    for (store, key, value) in retired {
        batch.push(&store, key.clone(), value.clone());
        written.push((key, value));
    }
    for save in live {
        if let (Some(store), Some(value)) = (&save.store, (save.read)()) {
            batch.push(store, save.key.clone(), value);
        }
    }
    Sampled {
        retired: written,
        forgotten: sampled_forgotten,
    }
}

impl Sampled {
    /// Forgets the removals and the last values that a completed boundary
    /// wrote, unless a newer value replaced one meanwhile.
    pub(super) fn completed(self) {
        crate::thread_state::with(|state| {
            let mut saves = state.local_saves.borrow_mut();
            let forgotten = self.forgotten.min(saves.forgotten.len());
            saves.forgotten.drain(..forgotten);
            for (key, value) in self.retired {
                if saves
                    .retired
                    .get(&key)
                    .is_some_and(|retired| retired.value == value)
                {
                    saves.retired.remove(&key);
                }
            }
        });
    }
}
