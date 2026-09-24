use super::*;

#[test]
fn test_prefetch_forward_scroll() {
    let mut scheduler = PrefetchScheduler::new();
    let strategy = PrefetchStrategy::new(2);

    scheduler.update(5, 10, 100, 1.0, &strategy);

    assert_eq!(scheduler.next_prefetch(), Some(11));
    assert_eq!(scheduler.next_prefetch(), Some(12));
    assert_eq!(scheduler.next_prefetch(), None);
}

#[test]
fn test_prefetch_backward_scroll() {
    let mut scheduler = PrefetchScheduler::new();
    let strategy = PrefetchStrategy::new(2);

    scheduler.update(5, 10, 100, -1.0, &strategy);

    assert_eq!(scheduler.next_prefetch(), Some(4));
    assert_eq!(scheduler.next_prefetch(), Some(3));
    assert_eq!(scheduler.next_prefetch(), None);
}

#[test]
fn test_prefetch_at_end() {
    let mut scheduler = PrefetchScheduler::new();
    let strategy = PrefetchStrategy::new(2);

    scheduler.update(95, 99, 100, 1.0, &strategy);

    assert_eq!(scheduler.next_prefetch(), None);
}

#[test]
fn test_prefetch_disabled() {
    let mut scheduler = PrefetchScheduler::new();
    let strategy = PrefetchStrategy::disabled();

    scheduler.update(5, 10, 100, 1.0, &strategy);

    assert_eq!(scheduler.next_prefetch(), None);
}
