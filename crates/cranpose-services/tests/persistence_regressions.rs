use std::{cell::Cell, sync::Arc, time::Duration};

use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};
use cranpose_services::{
    DurableSaveOutcome, MemoryPreferences, PreferencesError, PreferencesStore, Saver,
    rememberSaveable, run_durable_saves, set_platform_preferences,
};

use super::composition_support::serial;

#[derive(Default)]
struct FallibleStore {
    memory: MemoryPreferences,
    fail: std::sync::atomic::AtomicBool,
    writes: std::sync::atomic::AtomicUsize,
}

impl PreferencesStore for FallibleStore {
    fn get(&self, key: &str) -> Option<String> {
        self.memory.get(key)
    }
    fn set(&self, key: &str, value: &str) -> Result<(), PreferencesError> {
        self.writes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            Err(PreferencesError::Io("injected write failure".into()))
        } else {
            self.memory.set(key, value)
        }
    }
    fn remove(&self, key: &str) -> Result<(), PreferencesError> {
        self.memory.remove(key)
    }
    fn clear(&self) -> Result<(), PreferencesError> {
        self.memory.clear()
    }
    fn keys(&self) -> Vec<String> {
        self.memory.keys()
    }
}

#[test]
fn failed_save_retries_the_latest_value_without_recomposition() {
    use std::sync::atomic::Ordering;
    let _guard = serial();
    let store = Arc::new(FallibleStore::default());
    set_platform_preferences(store.clone());
    let mut composition = Composition::new(MemoryApplier::new());
    let slot = Cell::new(None);
    compose(&mut composition, &slot);
    compose(&mut composition, &slot);
    assert_eq!(
        store.writes.load(Ordering::SeqCst),
        0,
        "composing must not perform writes"
    );
    slot.get().expect("counter").set(2);
    store.fail.store(true, Ordering::SeqCst);
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Failed
    );
    assert!(store.get("counter").is_none());
    slot.get().expect("counter").set(3);
    store.fail.store(false, Ordering::SeqCst);
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
    assert_eq!(store.get("counter").as_deref(), Some("3"));
    drop(composition);
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Nothing
    );
}

#[test]
fn timed_out_saves_do_not_overlap_or_queue_obsolete_runs() {
    use std::sync::{Mutex, mpsc};

    use cranpose_services::register_durable_save;
    let _guard = serial();
    let (started, entered) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let registration = register_durable_save(move || {
        started.send(()).expect("notify caller");
        blocked
            .lock()
            .expect("gate")
            .recv()
            .expect("release first save");
        Ok(())
    });
    assert_eq!(
        run_durable_saves(Duration::from_millis(30)),
        DurableSaveOutcome::TimedOut
    );
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("first save started");
    assert_eq!(
        run_durable_saves(Duration::ZERO),
        DurableSaveOutcome::TimedOut
    );
    drop(registration);
    release.send(()).expect("release save");
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Nothing
    );
    assert!(
        entered.try_recv().is_err(),
        "the second call must not start another save"
    );
}

#[test]
fn a_failed_or_panicking_callback_does_not_skip_other_saves_or_poison_later_runs() {
    use cranpose_services::{DurableSaveError, register_durable_save};
    let _guard = serial();
    let fail = register_durable_save(|| Err(DurableSaveError::Storage("injected".into())));
    let panic = register_durable_save(|| panic!("injected callback panic"));
    let store = Arc::new(MemoryPreferences::new());
    let written = Arc::clone(&store);
    let success = register_durable_save(move || {
        written.set("saved", "yes")?;
        Ok(())
    });
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Failed
    );
    assert_eq!(store.get("saved").as_deref(), Some("yes"));
    drop((fail, panic));
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
    drop(success);
}

fn compose(composition: &mut Composition<MemoryApplier>, slot: &Cell<Option<MutableState<u32>>>) {
    composition
        .render(location_key(file!(), line!(), column!()), || {
            slot.set(Some(rememberSaveable("counter", Saver::of_display(), || 1)));
        })
        .expect("compose saved counter");
}

#[test]
fn save_before_another_frame_restores_the_latest_value() {
    let _guard = serial();
    let store = Arc::new(MemoryPreferences::new());
    set_platform_preferences(store.clone());
    let mut composition = Composition::new(MemoryApplier::new());
    let slot = Cell::new(None);
    compose(&mut composition, &slot);
    slot.get().expect("counter").set(2);
    run_durable_saves(Duration::from_secs(5));
    assert_eq!(store.get("counter").as_deref(), Some("2"));
    drop(composition);
    let mut replacement = Composition::new(MemoryApplier::new());
    compose(&mut replacement, &slot);
    assert_eq!(slot.get().expect("restored counter").get(), 2);
}
