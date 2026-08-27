//! When a platform accessibility bridge is allowed to publish a semantics
//! snapshot.
//!
//! Publishing is expensive: a full layout+semantics walk, a wire encode, and a
//! JNI hop. Measured on a Kirin 980 (Huawei EVR-AL00) scrolling the demo lazy
//! list, the unconditional per-frame publish cost 6.3-6.7 ms of every 16.7 ms
//! frame — the single largest CPU stage of the frame loop. The policy brings
//! that down to zero when no assistive technology is running, and to one
//! publish per [`ACCESSIBILITY_PUBLISH_INTERVAL`] while one is.
//!
//! Built on the host as well so its decision tests run everywhere.

use std::time::{Duration, Instant};

/// Jetpack Compose coalesces recurring accessibility publications at 100 ms
/// (`SendRecurringAccessibilityEventsIntervalMillis` in
/// `AndroidComposeViewAccessibilityDelegateCompat`); matching it keeps
/// assistive-technology freshness on par with Compose apps while capping the
/// bridge cost at one tree walk + encode per interval instead of one per
/// scrolled frame.
pub(crate) const ACCESSIBILITY_PUBLISH_INTERVAL: Duration = Duration::from_millis(100);

/// Frame-loop-local decision maker for a platform accessibility bridge.
///
/// The platform reports whether any assistive technology is active through
/// [`update_enabled`](Self::update_enabled); the bridge asks
/// [`try_begin_publish`](Self::try_begin_publish) before doing any snapshot
/// work and confirms with [`note_published`](Self::note_published). A publish
/// refused inside the throttle window leaves a [`wake_deadline`](Self::wake_deadline)
/// so the event loop can wake once the window opens and flush the trailing
/// state even if the app has gone idle.
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

    /// Samples the platform's assistive-technology state. Returns `true` when
    /// this call flipped the policy from disabled to enabled: the caller must
    /// then forget the revision it last published so the current tree goes out
    /// immediately, however old it is.
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

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    /// Whether the bridge may publish right now. `false` either means no
    /// assistive technology is listening, or the throttle window is still
    /// closed — in the latter case a wake deadline is armed so the trailing
    /// state cannot be lost to an idle loop.
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
                // Whatever was pending is being looked at right now.
                self.pending_deadline = None;
                true
            }
        }
    }

    pub(crate) fn note_published(&mut self, now: Instant) {
        self.last_publish = Some(now);
        self.pending_deadline = None;
    }

    /// When a refused publish is waiting, the instant the loop should wake to
    /// flush it. `None` whenever nothing is pending.
    pub(crate) fn wake_deadline(&self) -> Option<Instant> {
        self.pending_deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_enabled_at(now: Instant) -> AccessibilityPublishPolicy {
        let mut policy = AccessibilityPublishPolicy::new();
        assert!(policy.update_enabled(true));
        assert!(policy.try_begin_publish(now));
        policy.note_published(now);
        policy
    }

    #[test]
    fn disabled_policy_never_publishes_and_arms_no_wake() {
        let mut policy = AccessibilityPublishPolicy::new();
        let now = Instant::now();
        assert!(!policy.enabled());
        assert!(!policy.try_begin_publish(now));
        assert_eq!(policy.wake_deadline(), None);
    }

    #[test]
    fn first_publish_after_enabling_is_immediate() {
        let mut policy = AccessibilityPublishPolicy::new();
        assert!(policy.update_enabled(true));
        assert!(policy.try_begin_publish(Instant::now()));
    }

    #[test]
    fn enabling_twice_reports_the_transition_once() {
        let mut policy = AccessibilityPublishPolicy::new();
        assert!(policy.update_enabled(true));
        assert!(!policy.update_enabled(true));
    }

    #[test]
    fn re_enabling_after_disable_reports_a_fresh_transition() {
        let mut policy = AccessibilityPublishPolicy::new();
        assert!(policy.update_enabled(true));
        assert!(!policy.update_enabled(false));
        assert!(policy.update_enabled(true));
    }

    #[test]
    fn publish_inside_the_window_is_refused_with_a_wake_deadline() {
        let start = Instant::now();
        let mut policy = policy_enabled_at(start);
        let inside = start + ACCESSIBILITY_PUBLISH_INTERVAL / 2;
        assert!(!policy.try_begin_publish(inside));
        assert_eq!(
            policy.wake_deadline(),
            Some(start + ACCESSIBILITY_PUBLISH_INTERVAL)
        );
    }

    #[test]
    fn publish_after_the_window_opens_is_allowed_and_clears_the_wake() {
        let start = Instant::now();
        let mut policy = policy_enabled_at(start);
        assert!(!policy.try_begin_publish(start + ACCESSIBILITY_PUBLISH_INTERVAL / 2));
        let open = start + ACCESSIBILITY_PUBLISH_INTERVAL;
        assert!(policy.try_begin_publish(open));
        assert_eq!(policy.wake_deadline(), None);
    }

    #[test]
    fn window_open_probe_that_finds_no_change_leaves_no_wake() {
        // The bridge probes, sees the tree unchanged, publishes nothing. The
        // deadline must not stay armed or an idle loop would spin on it.
        let start = Instant::now();
        let mut policy = policy_enabled_at(start);
        assert!(!policy.try_begin_publish(start + ACCESSIBILITY_PUBLISH_INTERVAL / 2));
        assert!(policy.try_begin_publish(start + ACCESSIBILITY_PUBLISH_INTERVAL));
        assert_eq!(policy.wake_deadline(), None);
    }

    #[test]
    fn disabling_clears_a_pending_wake() {
        let start = Instant::now();
        let mut policy = policy_enabled_at(start);
        assert!(!policy.try_begin_publish(start + ACCESSIBILITY_PUBLISH_INTERVAL / 2));
        assert!(policy.wake_deadline().is_some());
        policy.update_enabled(false);
        assert_eq!(policy.wake_deadline(), None);
    }

    #[test]
    fn re_enabling_publishes_immediately_even_right_after_a_publish() {
        let start = Instant::now();
        let mut policy = policy_enabled_at(start);
        policy.update_enabled(false);
        assert!(policy.update_enabled(true));
        assert!(policy.try_begin_publish(start + Duration::from_millis(1)));
    }
}
