//! System "back" navigation requests.
//!
//! A platform back affordance — Android's system back / gesture, iOS's
//! left-edge swipe — feeds [`push_back_request`]; the app drains it with
//! [`take_back_requests`] and pops its own navigation. This gives one API across
//! platforms for what is otherwise a per-OS gesture.

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

static BACK_REQUESTS: AtomicUsize = AtomicUsize::new(0);

/// Record a system back request (called by the platform backend's gesture /
/// button handler).
pub fn push_back_request() {
    BACK_REQUESTS.fetch_add(1, Ordering::SeqCst);
}

/// Take (and clear) the number of pending back requests. Polled by the app; a
/// burst collapses into a count the app can coalesce.
pub fn take_back_requests() -> usize {
    BACK_REQUESTS.swap(0, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_accumulate_and_drain() {
        let _ = take_back_requests(); // clear any residue
        push_back_request();
        push_back_request();
        assert_eq!(take_back_requests(), 2);
        assert_eq!(take_back_requests(), 0);
    }
}
