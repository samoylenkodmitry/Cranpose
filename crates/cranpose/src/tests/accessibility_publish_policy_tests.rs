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

/// Publishes a tree at each open window from `start`, noting a read right
/// after the publishes `read_after` names, and returns the waits between
/// them.
fn publish_waits(start: Instant, publishes: usize, read_after: &[usize]) -> Vec<Duration> {
    let mut policy = AccessibilityPublishPolicy::new();
    assert!(policy.update_enabled(true));
    policy.set_backs_off_unread(true);
    let mut now = start;
    let mut waits = Vec::new();
    let mut last = None;
    for index in 0..publishes {
        while !policy.try_begin_publish(now) {
            now = policy
                .wake_deadline()
                .expect("a refused publish arms a wake");
        }
        if let Some(last) = last {
            waits.push(now - last);
        }
        last = Some(now);
        policy.published();
        if read_after.contains(&index) {
            policy.note_read(now);
        }
    }
    waits
}

#[test]
fn unread_trees_publish_less_often_up_to_the_unread_interval() {
    let s = Duration::from_secs;
    assert_eq!(
        publish_waits(Instant::now(), 6, &[]),
        [s(1), s(2), s(4), s(4), s(4)],
        "each unread tree doubles the wait from a second, up to four"
    );
}

#[test]
fn a_reader_in_use_gets_changes_at_the_interactive_interval() {
    let ms = Duration::from_millis;
    let mut expected = vec![ms(1000), ms(2000)];
    // The read after the third tree: changes publish every 100 ms for 2 s.
    expected.extend([ms(100); 19]);
    // Past the window and unread again: a second, then growing.
    expected.extend([ms(1000), ms(2000), ms(4000), ms(4000)]);
    assert_eq!(publish_waits(Instant::now(), 26, &[2]), expected);
}

#[test]
fn a_screen_reader_keeps_the_publish_interval_without_reads() {
    let start = Instant::now();
    let mut policy = policy_enabled_at(start);
    policy.set_backs_off_unread(false);
    policy.published();
    let next = start + ACCESSIBILITY_PUBLISH_INTERVAL;
    assert!(policy.try_begin_publish(next));
    policy.published();
    assert!(!policy.try_begin_publish(next + ACCESSIBILITY_PUBLISH_INTERVAL / 2));
    assert!(policy.try_begin_publish(next + ACCESSIBILITY_PUBLISH_INTERVAL));
}
