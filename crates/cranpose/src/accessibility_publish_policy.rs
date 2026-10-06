use std::time::{Duration, Instant};

/// How often a changing tree is published while nobody uses it: a screen
/// reader reads the tree when the person touches, swipes or acts, so
/// changes such as a ticker's need no more than this.
pub(crate) const ACCESSIBILITY_PUBLISH_INTERVAL: Duration = Duration::from_secs(1);
/// How often a changing tree is published while a reader is in use, so the
/// person never explores a stale screen.
pub(crate) const ACCESSIBILITY_INTERACTIVE_INTERVAL: Duration = Duration::from_millis(100);
/// How long a read or an action keeps publishes at the interactive interval.
pub(crate) const ACCESSIBILITY_INTERACTION_WINDOW: Duration = Duration::from_secs(2);
/// The longest wait between published trees while nothing reads them.
pub(crate) const ACCESSIBILITY_UNREAD_PUBLISH_INTERVAL: Duration = Duration::from_secs(4);

pub(crate) struct AccessibilityPublishPolicy {
    enabled: bool,
    last_publish: Option<Instant>,
    pending_deadline: Option<Instant>,
    /// The wait after a publish while nobody interacts: the publish
    /// interval, or longer while published trees go unread.
    interval: Duration,
    /// Until when the reader counts as in use.
    interactive_until: Option<Instant>,
    /// Whether a reader read since the last tree was sent.
    read_since_publish: bool,
    published_once: bool,
}

impl AccessibilityPublishPolicy {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            last_publish: None,
            pending_deadline: None,
            interval: ACCESSIBILITY_PUBLISH_INTERVAL,
            interactive_until: None,
            read_since_publish: false,
            published_once: false,
        }
    }

    pub(crate) fn update_enabled(&mut self, enabled: bool) -> bool {
        let became_enabled = enabled && !self.enabled;
        if became_enabled {
            self.last_publish = None;
            self.interval = ACCESSIBILITY_PUBLISH_INTERVAL;
            self.published_once = false;
        }
        if !enabled {
            self.pending_deadline = None;
        }
        self.enabled = enabled;
        became_enabled
    }

    /// A reader read the tree or acted on it at `now`: changes publish at
    /// the interactive interval for [`ACCESSIBILITY_INTERACTION_WINDOW`].
    pub(crate) fn note_read(&mut self, now: Instant) {
        self.read_since_publish = true;
        self.interval = ACCESSIBILITY_PUBLISH_INTERVAL;
        self.interactive_until = Some(now + ACCESSIBILITY_INTERACTION_WINDOW);
    }

    pub(crate) fn try_begin_publish(&mut self, now: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        let wait = if self.interactive_until.is_some_and(|until| now < until) {
            ACCESSIBILITY_INTERACTIVE_INTERVAL
        } else {
            self.interval
        };
        match self.last_publish {
            Some(last) if now.duration_since(last) < wait => {
                self.pending_deadline = Some(last + wait);
                false
            }
            _ => {
                self.pending_deadline = None;
                self.last_publish = Some(now);
                true
            }
        }
    }

    /// A tree was sent to the platform. When `backs_off_unread`, the tree
    /// before it went unread and the reader is not in use, the wait before
    /// the next one doubles, up to [`ACCESSIBILITY_UNREAD_PUBLISH_INTERVAL`].
    /// A platform backs off only where it reports reads and no screen reader
    /// runs: a screen reader reads only when the person uses it.
    pub(crate) fn published(&mut self, backs_off_unread: bool) {
        let in_use = self
            .interactive_until
            .zip(self.last_publish)
            .is_some_and(|(until, last)| last < until);
        let unread = self.published_once && !self.read_since_publish;
        self.interval = if backs_off_unread && unread && !in_use {
            (self.interval * 2).min(ACCESSIBILITY_UNREAD_PUBLISH_INTERVAL)
        } else {
            ACCESSIBILITY_PUBLISH_INTERVAL
        };
        self.read_since_publish = false;
        self.published_once = true;
    }

    pub(crate) fn wake_deadline(&self) -> Option<Instant> {
        self.pending_deadline
    }
}

#[cfg(test)]
#[path = "tests/accessibility_publish_policy_tests.rs"]
mod tests;
