use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_coroflow::SavedStateHandle;
use cranpose_services::{
    FilePreferences, HostController, PlatformDirectories, PreferencesStore, run_durable_saves,
    set_application_id, set_host_controller, set_platform_preferences,
};

struct Host(PathBuf);

impl HostController for Host {
    fn set_keep_screen_on(&self, _: bool) {}
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        Some(PlatformDirectories {
            data: self.0.clone(),
            config: self.0.clone(),
            cache: self.0.clone(),
            documents: None,
            temporary: self.0.clone(),
            shared: None,
        })
    }
    fn exit(&self) {}
    fn background(&self) {}
}

fn main() {
    let root = std::env::args()
        .skip(1)
        .find(|argument| !argument.starts_with('-'))
        .map_or_else(
            || {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../target/benchmarks")
                    .join(format!("persistence-{}", std::process::id()))
            },
            PathBuf::from,
        );
    set_host_controller(Arc::new(Host(root.clone())));
    set_application_id("save-benchmark").expect("benchmark id");
    for count in [1, 16, 64] {
        let store = Arc::new(FilePreferences::new());
        store.clear().expect("clear previous scenario");
        set_platform_preferences(store);
        let saved = SavedStateHandle::new("benchmark");
        let states: Vec<_> = (0..count)
            .map(|index| {
                saved
                    .get_mutable_state_flow(&index.to_string(), 0_u64)
                    .expect("typed saved key")
            })
            .collect();
        run_durable_saves(Duration::from_secs(10));
        for round in 1..=32 {
            for state in &states {
                state.set(round);
            }
            let started = Instant::now();
            let outcome = run_durable_saves(Duration::from_secs(10));
            let nanos = started.elapsed().as_nanos();
            let text = std::fs::read_to_string(root.join("save-benchmark/preferences"))
                .expect("read persisted data");
            let missing = (0..count)
                .filter(|index| {
                    let expected = format!("benchmark/{index}={round}");
                    !text.lines().any(|line| line == expected)
                })
                .count();
            println!(
                "{{\"keys\":{count},\"round\":{round},\"nanoseconds\":{nanos},\"missing\":{missing},\"outcome\":\"{outcome:?}\"}}"
            );
        }
    }
}
