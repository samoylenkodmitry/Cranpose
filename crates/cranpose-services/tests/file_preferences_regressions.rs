use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

use cranpose_services::{
    FilePreferences, HostController, PlatformDirectories, PreferencesStore, set_application_id,
    set_host_controller,
};

struct Host(PathBuf);

#[cfg(unix)]
#[test]
fn file_aliases_share_updates_without_replacing_the_alias() {
    let root = cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "preferences-alias");
    let path = root.join("preferences");
    let original = FilePreferences::at_path(&path);
    original.set("seed", "value").expect("create file");
    let alias = root.join("alias");
    std::os::unix::fs::symlink(&path, &alias).expect("file alias");
    let linked = FilePreferences::at_path(&alias);
    linked.set("through-alias", "one").expect("alias write");
    original.set("direct", "two").expect("direct write");
    assert_eq!(original.get("through-alias").as_deref(), Some("one"));
    assert_eq!(linked.get("direct").as_deref(), Some("two"));
    assert!(alias.is_symlink());
}

#[test]
fn failed_replacement_keeps_pending_values_for_an_explicit_retry() {
    let root = cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "preferences-retry");
    let path = root.join("preferences");
    let store = FilePreferences::at_path(&path);
    store.set("old", "durable").expect("initial save");
    std::fs::rename(&path, root.join("previous")).expect("preserve old file");
    std::fs::create_dir(&path).expect("block atomic replacement");
    assert!(store.set("new", "pending").is_err());
    assert_eq!(store.get("new").as_deref(), Some("pending"));
    std::fs::remove_dir(&path).expect("remove empty blocker");
    store.flush().expect("retry pending changes");
    drop(store);
    let reopened = FilePreferences::at_path(&path);
    assert_eq!(reopened.get("old").as_deref(), Some("durable"));
    assert_eq!(reopened.get("new").as_deref(), Some("pending"));
}

#[test]
fn batching_preserves_unicode_and_removal_across_reopening() {
    let root = cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "preferences-batch");
    let path = root.join("preferences");
    let store = FilePreferences::at_path(&path);
    store
        .set_many(&std::collections::BTreeMap::from([
            ("a=\n%".to_owned(), "👩‍💻\nново=100%".to_owned()),
            ("remove".to_owned(), "value".to_owned()),
        ]))
        .expect("batch write");
    store.remove("remove").expect("remove key");
    drop(store);
    let reopened = FilePreferences::at_path(&path);
    assert_eq!(reopened.get("a=\n%").as_deref(), Some("👩‍💻\nново=100%"));
    assert!(reopened.get("remove").is_none());
}

impl HostController for Host {
    fn set_keep_screen_on(&self, _: bool) {}
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        Some(PlatformDirectories {
            data: self.0.clone(),
            config: self.0.clone(),
            cache: self.0.clone(),
            temporary: self.0.clone(),
            documents: None,
            shared: None,
        })
    }
    fn exit(&self) {}
    fn background(&self) {}
}

#[test]
fn concurrent_instances_preserve_every_successful_write_on_reopen() {
    let _guard = super::composition_support::serial();
    let root = cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), "preferences-writers");
    set_host_controller(Arc::new(Host(root)));
    set_application_id("writers").expect("application id");
    let seed = FilePreferences::new();
    seed.set("seed", "present").expect("seed");
    let stores: Vec<_> = (0..8)
        .map(|_| {
            let store = FilePreferences::new();
            assert_eq!(store.get("seed").as_deref(), Some("present"));
            store
        })
        .collect();
    drop(seed);
    let barrier = Arc::new(Barrier::new(stores.len()));
    let workers: Vec<_> = stores
        .into_iter()
        .enumerate()
        .map(|(i, store)| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .set(&format!("key-{i}"), "value")
                    .expect("successful write");
            })
        })
        .collect();
    for worker in workers {
        worker.join().expect("writer completed");
    }
    let reopened = FilePreferences::new();
    for i in 0..8 {
        assert_eq!(reopened.get(&format!("key-{i}")).as_deref(), Some("value"));
    }
    cranpose_services::clear_host_controller();
    cranpose_services::clear_application_id();
}
