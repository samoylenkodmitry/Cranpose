use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use web_time::{Duration, Instant};

use parking_lot::{Mutex, RwLock, RwLockReadGuard};

pub(crate) struct ServiceRegistry<T: ?Sized> {
    value: RwLock<Option<Arc<T>>>,
    warned: AtomicBool,
}

impl<T: ?Sized> ServiceRegistry<T> {
    pub(crate) const fn new() -> Self {
        Self {
            value: RwLock::new(None),
            warned: AtomicBool::new(false),
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, Option<Arc<T>>> {
        self.value.read()
    }

    pub(crate) fn set(&self, value: Arc<T>) {
        *self.value.write() = Some(value);
        self.warned.store(false, Ordering::Release);
    }

    pub(crate) fn clear(&self) {
        *self.value.write() = None;
        self.warned.store(false, Ordering::Release);
    }

    pub(crate) fn get(&self) -> Option<Arc<T>> {
        self.read().clone()
    }

    pub(crate) fn get_or_warn(&self, name: &str) -> Option<Arc<T>> {
        let value = self.get();
        if value.is_none() && !self.warned.swap(true, Ordering::AcqRel) {
            log::warn!("{name} platform backend is not registered");
        }
        value
    }
}

struct RecoveryState {
    next_attempt: Instant,
    delay: Duration,
}

pub(crate) struct RecoveryGate {
    state: Mutex<Option<RecoveryState>>,
    healthy: AtomicBool,
}

#[cfg(test)]
pub(crate) struct TestServiceGuard {
    _guard: parking_lot::MutexGuard<'static, ()>,
}

#[cfg(test)]
impl Drop for TestServiceGuard {
    fn drop(&mut self) {
        crate::audio::clear_platform_audio();
        crate::camera::clear_platform_camera();
        crate::haptics::clear_platform_haptics();
        crate::image_picker::clear_platform_image_picker();
        crate::media::clear_platform_media_player();
        crate::network_status::clear_platform_network_monitor();
        crate::notifier::clear_platform_notifier();
        crate::purchases::clear_platform_purchases();
    }
}

#[cfg(test)]
pub(crate) fn test_service_guard() -> TestServiceGuard {
    static TEST_SERVICE_LOCK: Mutex<()> = Mutex::new(());
    TestServiceGuard {
        _guard: TEST_SERVICE_LOCK.lock(),
    }
}

impl RecoveryGate {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(None),
            healthy: AtomicBool::new(true),
        }
    }

    pub(crate) fn try_start(&self) -> bool {
        self.healthy.store(false, Ordering::Release);
        let now = Instant::now();
        let mut state = self.state.lock();
        let state = state.get_or_insert(RecoveryState {
            next_attempt: now,
            delay: Duration::ZERO,
        });
        if now < state.next_attempt {
            return false;
        }
        let delay = state.delay.max(Duration::from_millis(16));
        state.next_attempt = now + delay;
        state.delay = (delay * 2).min(Duration::from_secs(2));
        true
    }

    pub(crate) fn succeeded(&self) {
        if self.healthy.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut state = self.state.lock();
        *state = None;
    }
}

#[cfg(test)]
mod tests {
    use super::RecoveryGate;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn recovery_gate_retries_after_backoff_and_resets() {
        let gate = RecoveryGate::new();
        assert!(gate.try_start());
        assert!(!gate.try_start());
        thread::sleep(Duration::from_millis(20));
        assert!(gate.try_start());
        gate.succeeded();
        assert!(gate.try_start());
    }
}
