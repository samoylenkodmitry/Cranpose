use std::{sync::Arc, time::Duration};

use cranpose_coroflow::{SavedStateError, SavedStateHandle};
use cranpose_services::{
    DurableSaveOutcome, MemoryPreferences, PreferencesStore, run_durable_saves,
    set_platform_preferences,
};

fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    set_platform_preferences(first.clone());
    let a = SavedStateHandle::new("notes");
    a.set("count", &1_u32).expect("first store");
    let second = Arc::new(MemoryPreferences::new());
    set_platform_preferences(second.clone());
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
