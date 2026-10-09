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
}

#[test]
fn reading_never_asks_and_asking_is_explicit() {
    let _guard = crate::registry::test_service_guard();
    let platform = Platform::with(MicrophonePermission::NotAsked);
    set_platform_microphone_access(Some(platform.clone()));
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
    set_platform_microphone_access(Some(platform.clone()));
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
    set_platform_microphone_access(Some(platform.clone()));
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
