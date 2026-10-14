//! The browser's session history, as a page sees it: the URL fragment of the
//! current entry, a number the app keeps in each entry it adds, and the
//! user's moves through the entries with Back and Forward.
//!
//! The web host installs it; elsewhere [`browser_history`] is `None`. A
//! navigation host keeps the history in step with its back stack.

use std::rc::Rc;

/// Session history of the page.
pub trait BrowserHistory {
    /// The URL fragment of the current entry, without the leading `#`.
    fn fragment(&self) -> String;

    /// The number this app keeps in the current entry, or `None` for an entry
    /// it did not add, such as the page's first one or a fragment the user
    /// typed.
    fn depth(&self) -> Option<usize>;

    /// Adds an entry after the current one, which becomes current, keeping
    /// `depth` in it and showing `fragment` in the address bar.
    fn push(&self, depth: usize, fragment: &str);

    /// Replaces the current entry's number and fragment.
    fn replace(&self, depth: usize, fragment: &str);

    /// Moves `delta` entries back (negative) or forward, as the browser's own
    /// buttons do. The move ends in an `on_move` call of [`Self::observe`].
    fn go(&self, delta: isize);

    /// Calls `on_move` each time the current entry changes by a move through
    /// the history, until the observer is dropped.
    fn observe(&self, on_move: Rc<dyn Fn()>) -> BrowserHistoryObserver;
}

/// Shared handle to a [`BrowserHistory`].
pub type BrowserHistoryRef = Rc<dyn BrowserHistory>;

/// Ends an observation of [`BrowserHistory::observe`] when dropped.
pub struct BrowserHistoryObserver {
    _stop: Option<Box<dyn std::any::Any>>,
}

impl BrowserHistoryObserver {
    /// An observer whose `stop` value ends the observation when it drops.
    pub fn new(stop: impl std::any::Any) -> Self {
        Self {
            _stop: Some(Box::new(stop)),
        }
    }
}

/// Installs the page's history, replacing any previously installed one.
pub fn set_platform_browser_history(history: BrowserHistoryRef) {
    crate::thread_state::with(|state| *state.browser_history.borrow_mut() = Some(history));
}

/// Removes the installed history (tests and teardown).
pub fn clear_platform_browser_history() {
    crate::thread_state::with(|state| *state.browser_history.borrow_mut() = None);
}

/// The page's session history, or `None` outside a browser.
pub fn browser_history() -> Option<BrowserHistoryRef> {
    crate::thread_state::with(|state| state.browser_history.borrow().clone())
}
