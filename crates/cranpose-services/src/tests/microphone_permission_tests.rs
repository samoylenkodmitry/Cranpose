use std::sync::atomic::AtomicUsize;

use super::*;

/// A platform that records what was asked of it.
struct Platform {
    permission: Mutex<MicrophonePermission>,
    requests: AtomicUsize,
    settings: AtomicUsize,
}

impl Platform {
    fn with(permission: MicrophonePermission) -> Arc<Self> {
        Arc::new(Self {
            permission: Mutex::new(permission),
            requests: AtomicUsize::new(0),
            settings: AtomicUsize::new(0),
        })
    }

    fn answer(&self, permission: MicrophonePermission) {
        if let Ok(mut current) = self.permission.lock() {
            *current = permission;
        }
        publish_microphone_permission();
    }
}

impl MicrophoneAccess for Platform {
    fn permission(&self) -> MicrophonePermission {
        self.permission
            .lock()
            .map_or(MicrophonePermission::NotAsked, |permission| *permission)
    }
    fn request(&self) {
        self.requests.fetch_add(1, Ordering::SeqCst);
    }
    fn open_settings(&self) {
        self.settings.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn without_a_backend_the_microphone_is_allowed_and_nothing_is_asked() {
    let _guard = crate::registry::test_service_guard();
    set_platform_microphone_access(None);
    assert_eq!(microphone_permission(), MicrophonePermission::Granted);
    request_microphone_permission();
    open_microphone_settings();
    assert!(prepare_microphone().is_ok());
}

#[test]
fn the_platform_releases_the_audio_when_the_last_recording_lets_go() {
    struct Counted {
        prepared: AtomicUsize,
        finished: AtomicUsize,
    }
    impl MicrophoneAccess for Counted {
        fn permission(&self) -> MicrophonePermission {
            MicrophonePermission::Granted
        }
        fn request(&self) {}
        fn open_settings(&self) {}
        fn prepare_recording(&self) -> Result<(), String> {
            self.prepared.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn finish_recording(&self) {
            self.finished.fetch_add(1, Ordering::SeqCst);
        }
    }
    let _guard = crate::registry::test_service_guard();
    let platform = Arc::new(Counted {
        prepared: AtomicUsize::new(0),
        finished: AtomicUsize::new(0),
    });
    set_platform_microphone_access(Some(Arc::<Counted>::clone(&platform)));
    let first = prepare_microphone().expect("ready");
    let second = prepare_microphone().expect("ready");
    assert_eq!(platform.prepared.load(Ordering::SeqCst), 2);
    drop(first);
    assert_eq!(platform.finished.load(Ordering::SeqCst), 0);
    drop(second);
    assert_eq!(platform.finished.load(Ordering::SeqCst), 1);
    // Without a platform there is nothing to release.
    set_platform_microphone_access(None);
    drop(prepare_microphone().expect("ready"));
    assert_eq!(platform.finished.load(Ordering::SeqCst), 1);
}

#[test]
fn preparing_reports_why_the_platform_cannot_record() {
    struct Busy;
    impl MicrophoneAccess for Busy {
        fn permission(&self) -> MicrophonePermission {
            MicrophonePermission::Granted
        }
        fn request(&self) {}
        fn open_settings(&self) {}
        fn prepare_recording(&self) -> Result<(), String> {
            Err("a call holds the audio session".into())
        }
    }
    let _guard = crate::registry::test_service_guard();
    set_platform_microphone_access(Some(Arc::new(Busy)));
    assert_eq!(
        prepare_microphone().err(),
        Some("a call holds the audio session".into())
    );
    set_platform_microphone_access(None);
}

#[test]
fn reading_never_asks_and_asking_is_explicit() {
    let _guard = crate::registry::test_service_guard();
    let platform = Platform::with(MicrophonePermission::NotAsked);
    set_platform_microphone_access(Some(Arc::<Platform>::clone(&platform)));
    assert_eq!(microphone_permission(), MicrophonePermission::NotAsked);
    let observer = observe_microphone_permission(|_| {});
    assert_eq!(platform.requests.load(Ordering::SeqCst), 0);
    request_microphone_permission();
    assert_eq!(platform.requests.load(Ordering::SeqCst), 1);
    drop(observer);
    set_platform_microphone_access(None);
}

#[test]
fn only_a_prompt_that_can_still_show_is_requested() {
    let _guard = crate::registry::test_service_guard();
    let platform = Platform::with(MicrophonePermission::Denied);
    set_platform_microphone_access(Some(Arc::<Platform>::clone(&platform)));
    request_microphone_permission();
    assert_eq!(platform.requests.load(Ordering::SeqCst), 1);
    for permission in [MicrophonePermission::Granted, MicrophonePermission::Blocked] {
        platform.answer(permission);
        request_microphone_permission();
    }
    assert_eq!(platform.requests.load(Ordering::SeqCst), 1);
    open_microphone_settings();
    assert_eq!(platform.settings.load(Ordering::SeqCst), 1);
    set_platform_microphone_access(None);
}

#[test]
fn observers_see_the_permission_at_once_and_each_answer() {
    let _guard = crate::registry::test_service_guard();
    let platform = Platform::with(MicrophonePermission::NotAsked);
    set_platform_microphone_access(Some(Arc::<Platform>::clone(&platform)));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let observer = {
        let seen = Arc::clone(&seen);
        observe_microphone_permission(move |permission| {
            if let Ok(mut seen) = seen.lock() {
                seen.push(permission);
            }
        })
    };
    platform.answer(MicrophonePermission::Granted);
    drop(observer);
    platform.answer(MicrophonePermission::Blocked);
    let seen = seen.lock().map(|seen| seen.clone()).unwrap_or_default();
    assert_eq!(
        seen,
        [
            MicrophonePermission::NotAsked,
            MicrophonePermission::Granted
        ]
    );
    set_platform_microphone_access(None);
}
