use super::*;

#[test]
fn announcements_come_back_in_order() {
    drain_announcements();
    announce("first");
    Announcer.announce_assertive("second");
    let taken = drain_announcements();
    assert_eq!(taken.len(), 2);
    assert_eq!(taken[0].text, "first");
    assert_eq!(taken[0].mode, LiveRegionMode::Polite);
    assert_eq!(taken[1].text, "second");
    assert_eq!(taken[1].mode, LiveRegionMode::Assertive);
    assert_eq!(pending_announcements(), 0);
}

#[test]
fn empty_text_never_reaches_the_reader() {
    drain_announcements();
    announce("");
    announce("   ");
    assert_eq!(pending_announcements(), 0);
}

#[test]
fn the_queue_holds_the_newest_when_nothing_drains_it() {
    drain_announcements();
    for index in 0..QUEUE_LIMIT + 5 {
        announce(format!("line {index}"));
    }
    let taken = drain_announcements();
    assert_eq!(taken.len(), QUEUE_LIMIT);
    assert_eq!(taken[0].text, "line 5");
    assert_eq!(
        taken[QUEUE_LIMIT - 1].text,
        format!("line {}", QUEUE_LIMIT + 4)
    );
}
