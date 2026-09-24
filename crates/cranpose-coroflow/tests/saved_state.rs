use std::{sync::Arc, time::Duration};

use cranpose_coroflow::SavedStateHandle;
use cranpose_services::{MemoryPreferences, run_durable_saves, set_platform_preferences};

#[test]
fn saved_state_survives_into_a_new_view_model() {
    set_platform_preferences(Arc::new(MemoryPreferences::new()));
    let saved = SavedStateHandle::new("notes");
    assert_eq!(saved.get::<u32>("count"), None);
    saved.set("count", &3_u32);
    assert_eq!(saved.get::<u32>("count"), Some(3));

    let query = saved.get_mutable_state_flow("query", String::new());
    query.set("groceries".to_owned());
    let other = SavedStateHandle::new("other");
    assert_eq!(other.get::<u32>("count"), None, "namespaces stay apart");
    run_durable_saves(Duration::from_secs(5));
    drop(saved);

    let restored = SavedStateHandle::new("notes");
    let query = restored.get_mutable_state_flow("query", String::new());
    assert_eq!(query.value(), "groceries");
}
