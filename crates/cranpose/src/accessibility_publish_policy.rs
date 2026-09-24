use std::time::{Duration, Instant};

pub(crate) const ACCESSIBILITY_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) struct AccessibilityPublishPolicy {
    enabled: bool,
    last_publish: Option<Instant>,
    pending_deadline: Option<Instant>,
}

impl AccessibilityPublishPolicy {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            last_publish: None,
            pending_deadline: None,
        }
    }

    pub(crate) fn update_enabled(&mut self, enabled: bool) -> bool {
        let became_enabled = enabled && !self.enabled;
        if became_enabled {
            self.last_publish = None;
        }
        if !enabled {
            self.pending_deadline = None;
        }
        self.enabled = enabled;
        became_enabled
    }

    pub(crate) fn try_begin_publish(&mut self, now: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        match self.last_publish {
            Some(last) if now.duration_since(last) < ACCESSIBILITY_PUBLISH_INTERVAL => {
                self.pending_deadline = Some(last + ACCESSIBILITY_PUBLISH_INTERVAL);
                false
            }
            _ => {
                self.pending_deadline = None;
                self.last_publish = Some(now);
                true
            }
        }
    }

    pub(crate) fn wake_deadline(&self) -> Option<Instant> {
        self.pending_deadline
    }
}

#[cfg(test)]
#[path = "tests/accessibility_publish_policy_tests.rs"]
mod tests;
