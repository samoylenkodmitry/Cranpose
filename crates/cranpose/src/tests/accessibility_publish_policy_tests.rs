use super::*;

fn policy_enabled_at(now: Instant) -> AccessibilityPublishPolicy {
    let mut policy = AccessibilityPublishPolicy::new();
    assert!(policy.update_enabled(true));
    assert!(policy.try_begin_publish(now));
    policy
}

#[test]
fn disabled_policy_never_publishes_and_arms_no_wake() {
    let mut policy = AccessibilityPublishPolicy::new();
    let now = Instant::now();
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
    let start = Instant::now();
    let mut policy = policy_enabled_at(start);
    assert!(!policy.try_begin_publish(start + ACCESSIBILITY_PUBLISH_INTERVAL / 2));
    assert!(policy.try_begin_publish(start + ACCESSIBILITY_PUBLISH_INTERVAL));
    assert_eq!(policy.wake_deadline(), None);
}

#[test]
fn a_probe_consumes_the_window_even_when_nothing_publishes() {
    let start = Instant::now();
    let mut policy = policy_enabled_at(start);
    let probe = start + ACCESSIBILITY_PUBLISH_INTERVAL;
    assert!(policy.try_begin_publish(probe));
    assert!(!policy.try_begin_publish(probe + Duration::from_millis(16)));
    assert!(policy.try_begin_publish(probe + ACCESSIBILITY_PUBLISH_INTERVAL));
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
