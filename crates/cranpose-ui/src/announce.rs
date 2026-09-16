use std::cell::RefCell;

use cranpose_core::CompositionLocal;
use cranpose_foundation::LiveRegionMode;

const QUEUE_LIMIT: usize = 32;

/// Text a screen reader should read out, with no control to move to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announcement {
    pub text: String,
    pub mode: LiveRegionMode,
}

/// Reads text out on a screen reader without moving focus, the way
/// `View.announceForAccessibility` does under Compose on Android.
///
/// ```ignore
/// let reader = cranpose_ui::local_announcer().current();
/// reader.announce("Seven receipts imported");
/// reader.announce_assertive("Import failed");
/// ```
///
/// Use this for an event with no control behind it. When a control on screen
/// holds the text, mark that control with `Modifier::semantics(|config| {
/// config.live_region = Some(LiveRegionMode::Polite) })` instead, so the reader
/// can also move to it and read it again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Announcer;

impl Announcer {
    /// Reads `text` once the reader finishes what it says now.
    pub fn announce(&self, text: impl Into<String>) {
        push(text.into(), LiveRegionMode::Polite);
    }

    /// Cuts off what the reader says now and reads `text` at once.
    pub fn announce_assertive(&self, text: impl Into<String>) {
        push(text.into(), LiveRegionMode::Assertive);
    }
}

/// CompositionLocal carrying the [`Announcer`]. The same instance comes back on
/// every call.
pub fn local_announcer() -> CompositionLocal<Announcer> {
    thread_local! {
        static LOCAL_ANNOUNCER: RefCell<Option<CompositionLocal<Announcer>>> =
            const { RefCell::new(None) };
    }

    LOCAL_ANNOUNCER.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| cranpose_core::compositionLocalOf(Announcer::default))
            .clone()
    })
}

/// Reads `text` out politely. Short form of [`Announcer::announce`] for code
/// that sits outside composition.
pub fn announce(text: impl Into<String>) {
    Announcer.announce(text);
}

/// Takes the queued announcements. A platform accessibility bridge calls this
/// once a frame and hands each one to the screen reader.
pub fn drain_announcements() -> Vec<Announcement> {
    queue(std::mem::take)
}

/// How many announcements wait for a bridge to take them.
pub fn pending_announcements() -> usize {
    queue(|pending| pending.len())
}

fn push(text: String, mode: LiveRegionMode) {
    if text.trim().is_empty() {
        return;
    }
    queue(|pending| {
        if pending.len() >= QUEUE_LIMIT {
            pending.remove(0);
        }
        pending.push(Announcement { text, mode });
    });
}

fn queue<T>(action: impl FnOnce(&mut Vec<Announcement>) -> T) -> T {
    thread_local! {
        static PENDING: RefCell<Vec<Announcement>> = const { RefCell::new(Vec::new()) };
    }

    PENDING.with(|cell| action(&mut cell.borrow_mut()))
}

#[cfg(test)]
mod tests {
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
}
