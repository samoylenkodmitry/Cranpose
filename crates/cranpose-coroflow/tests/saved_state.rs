use std::{
    sync::{Arc, Barrier, Mutex, PoisonError},
    time::Duration,
};

use cranpose_coroflow::{SavedStateError, SavedStateHandle};
use cranpose_services::{
    DurableSaveOutcome, MemoryPreferences, PreferencesError, PreferencesStore, run_durable_saves,
    set_platform_preferences,
};

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn saved_state_survives_into_a_new_view_model() {
    let _guard = serial();
    set_platform_preferences(Arc::new(MemoryPreferences::new()));
    let saved = SavedStateHandle::new("notes");
    assert_eq!(saved.get::<u32>("count"), Ok(None));
    saved.set("count", &3_u32).expect("set count");
    assert_eq!(saved.get::<u32>("count"), Ok(Some(3)));

    let query = saved
        .get_mutable_state_flow("query", String::new())
        .expect("query");
    let same = saved
        .get_mutable_state_flow("query", "ignored".to_owned())
        .expect("same query");
    query.set("groceries".to_owned());
    assert_eq!(same.value(), "groceries");
    assert_eq!(
        saved.get::<String>("query"),
        Ok(Some("groceries".to_owned()))
    );
    assert!(matches!(
        saved.get_mutable_state_flow("query", 42_u32),
        Err(SavedStateError::TypeMismatch(_))
    ));
    saved.set("query", &"latest".to_owned()).expect("set query");
    assert_eq!(query.value(), "latest");
    let shared = SavedStateHandle::new("notes");
    let shared_query = shared
        .get_mutable_state_flow("query", String::new())
        .expect("shared namespace");
    shared_query.set("newest".to_owned());
    assert_eq!(query.value(), "newest");
    let other = SavedStateHandle::new("other");
    assert_eq!(other.get::<u32>("count"), Ok(None), "namespaces stay apart");
    run_durable_saves(Duration::from_secs(5));
    drop(saved);
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
    drop(shared);

    let restored = SavedStateHandle::new("notes");
    let query = restored
        .get_mutable_state_flow("query", String::new())
        .expect("query");
    assert_eq!(query.value(), "newest");
}

#[test]
fn saved_values_keep_the_backend_their_namespace_was_opened_with() {
    let _guard = serial();
    let first = Arc::new(MemoryPreferences::new());
    set_platform_preferences(Arc::<MemoryPreferences>::clone(&first));
    let a = SavedStateHandle::new("notes");
    a.set("count", &1_u32).expect("first store");
    let second = Arc::new(MemoryPreferences::new());
    set_platform_preferences(Arc::<MemoryPreferences>::clone(&second));
    let b = SavedStateHandle::new("notes");
    b.set("count", &2_u32).expect("second store");
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
    assert_eq!(first.get("notes/count").as_deref(), Some("1"));
    assert_eq!(second.get("notes/count").as_deref(), Some("2"));
    drop((a, b));
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Nothing
    );
}

#[test]
fn namespace_and_key_separators_cannot_alias_another_saved_value() {
    let _guard = serial();
    set_platform_preferences(Arc::new(MemoryPreferences::new()));
    let a = SavedStateHandle::new("a/b");
    let b = SavedStateHandle::new("a");
    let c = SavedStateHandle::new("a%2Fb");
    a.set("c", &1_u32).expect("nested namespace");
    b.set("b/c", &2_u32).expect("nested key");
    c.set("c", &3_u32).expect("percent in namespace");
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
    drop((a, b, c));
    assert_eq!(SavedStateHandle::new("a/b").get::<u32>("c"), Ok(Some(1)));
    assert_eq!(SavedStateHandle::new("a").get::<u32>("b/c"), Ok(Some(2)));
    assert_eq!(SavedStateHandle::new("a%2Fb").get::<u32>("c"), Ok(Some(3)));
}

struct InterceptPreferences<F> {
    inner: MemoryPreferences,
    on_get: F,
}

impl<F: Fn(&str) + Send + Sync> PreferencesStore for InterceptPreferences<F> {
    fn get(&self, key: &str) -> Option<String> {
        (self.on_get)(key);
        self.inner.get(key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError> {
        self.inner.set(key, value)
    }

    fn remove(&self, key: &str) -> Result<(), PreferencesError> {
        self.inner.remove(key)
    }

    fn keys(&self) -> Vec<String> {
        self.inner.keys()
    }

    fn clear(&self) -> Result<(), PreferencesError> {
        self.inner.clear()
    }
}

#[test]
fn preferences_store_reading_saved_state_during_restore_does_not_deadlock() {
    let _guard = serial();
    let handle_slot: Arc<Mutex<Option<SavedStateHandle>>> = Arc::default();
    let nested_value: Arc<Mutex<Option<u32>>> = Arc::default();
    let hook_handle = Arc::clone(&handle_slot);
    let hook_nested = Arc::clone(&nested_value);
    let store = Arc::new(InterceptPreferences {
        inner: MemoryPreferences::new(),
        on_get: move |key: &str| {
            let handle = hook_handle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            if key.ends_with("/primary")
                && let Some(handle) = handle
            {
                let nested = handle
                    .get::<u32>("secondary")
                    .expect("reentrant read succeeds without deadlocking");
                *hook_nested.lock().unwrap_or_else(PoisonError::into_inner) = nested;
            }
        },
    });
    store
        .inner
        .set("reentrant/primary", "41")
        .expect("seed primary");
    store
        .inner
        .set("reentrant/secondary", "7")
        .expect("seed secondary");
    set_platform_preferences(Arc::clone(&store) as Arc<dyn PreferencesStore>);

    let handle = SavedStateHandle::new("reentrant");
    *handle_slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle.clone());

    let flow = handle
        .get_mutable_state_flow("primary", 0_u32)
        .expect("restore primary");
    assert_eq!(flow.value(), 41, "primary restores stored value");
    assert_eq!(
        *nested_value.lock().unwrap_or_else(PoisonError::into_inner),
        Some(7),
        "reentrant PreferencesStore::get read secondary without deadlocking"
    );
    handle_slot
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
}

#[test]
fn competing_saved_state_initializers_persist_only_the_winning_value() {
    let _guard = serial();
    let store = Arc::new(MemoryPreferences::new());
    set_platform_preferences(Arc::<MemoryPreferences>::clone(&store));

    let handle = SavedStateHandle::new("race");
    let barrier = Arc::new(Barrier::new(2));
    let (first, second) = std::thread::scope(|scope| {
        let a_handle = handle.clone();
        let a_barrier = Arc::clone(&barrier);
        let t1 = scope.spawn(move || {
            a_barrier.wait();
            a_handle
                .get_mutable_state_flow("counter", 10_u32)
                .expect("first initializer")
        });
        let b_handle = handle.clone();
        let b_barrier = Arc::clone(&barrier);
        let t2 = scope.spawn(move || {
            b_barrier.wait();
            b_handle
                .get_mutable_state_flow("counter", 20_u32)
                .expect("second initializer")
        });
        (
            t1.join().expect("first thread"),
            t2.join().expect("second thread"),
        )
    });

    assert_eq!(
        first.value(),
        second.value(),
        "competing initializers share one winning flow"
    );
    first.set(99_u32);
    assert_eq!(
        second.value(),
        99_u32,
        "updates to one handle reach the other"
    );
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed,
        "durable save flushes the winning entry"
    );
    assert_eq!(
        store.get("race/counter").as_deref(),
        Some("99"),
        "losing initializer must not overwrite the winning flow's persisted value"
    );
}

#[test]
fn updating_saved_state_during_concurrent_initialization_persists_the_update() {
    let _guard = serial();
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let blocked_once = std::sync::atomic::AtomicBool::new(false);
    let hook_entered = Arc::clone(&entered);
    let hook_release = Arc::clone(&release);
    let store = Arc::new(InterceptPreferences {
        inner: MemoryPreferences::new(),
        on_get: move |key: &str| {
            if key == "init_update/counter"
                && !blocked_once.swap(true, std::sync::atomic::Ordering::SeqCst)
            {
                hook_entered.wait();
                hook_release.wait();
            }
        },
    });
    set_platform_preferences(Arc::clone(&store) as Arc<dyn PreferencesStore>);

    let handle = SavedStateHandle::new("init_update");
    let flow = std::thread::scope(|scope| {
        let init_handle = handle.clone();
        let initializer = scope.spawn(move || {
            init_handle
                .get_mutable_state_flow("counter", 10_u32)
                .expect("initializer obtains flow")
        });

        // Wait until the initializer is inside PreferencesStore::get, then
        // create/update the same key and run a durable save before releasing it.
        entered.wait();
        handle.set("counter", &77_u32).expect("concurrent update");
        assert_eq!(
            run_durable_saves(Duration::from_secs(5)),
            DurableSaveOutcome::Completed,
            "durable save completes immediately after concurrent update"
        );
        assert_eq!(
            store.inner.get("init_update/counter").as_deref(),
            Some("77"),
            "durable save must include the update made while another thread initializes"
        );
        release.wait();
        initializer.join().expect("initializer thread")
    });

    assert_eq!(
        flow.value(),
        77_u32,
        "initializer joins the existing flow holding the concurrent update"
    );
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed,
        "subsequent durable save still succeeds"
    );
    assert_eq!(
        store.inner.get("init_update/counter").as_deref(),
        Some("77"),
        "persisted value stays intact after the initializer finishes"
    );
}
