use std::{
    cell::{Cell, RefCell},
    fmt::Write,
    rc::Rc,
};

use cranpose_core::{DisposableEffect, SideEffect, remember};
use cranpose_services::{BrowserHistoryObserver, BrowserHistoryRef, Saver, browser_history};

use crate::controller::NavController;

/// Keeps the browser's history and address bar in step with `controller`'s
/// back stack — Compose Multiplatform's `bindToNavigation`.
///
/// Each navigation adds a history entry whose URL fragment is `#/` and the
/// route's saved form from `saver`, percent-encoded. The browser's Back pops
/// the back stack, its Forward navigates to the entry's route again, and a
/// pop in the app goes back in the history. A page that opens with a
/// route's fragment, a link or a reload, shows that route over the start;
/// so does a fragment the user types. A navigation that is neither one push
/// nor pops replaces the current entry. Outside a browser it does nothing.
#[track_caller]
pub fn BindBrowserHistory<R: Clone + PartialEq + 'static>(
    controller: NavController<R>,
    saver: Saver<R>,
) {
    let Some(history) = browser_history() else {
        return;
    };
    // Recomposes on each change of the back stack, which the side effect
    // reads as it is when it runs.
    controller.stack.read(|_| ());
    let sync = remember(move || {
        Rc::new(HistorySync {
            history,
            saver,
            controller,
            synced: RefCell::new(Vec::new()),
            started: Cell::new(false),
            adopt: Cell::new(false),
            landing: Cell::new(None),
        })
    })
    .with(Rc::clone);
    let started = Rc::clone(&sync);
    DisposableEffect(controller, move |scope| {
        let observer = HistorySync::start(&started);
        scope.on_dispose(move || drop(observer))
    });
    SideEffect(move || sync.stack_changed(sync.controller.back_stack()));
}

struct HistorySync<R: Clone + 'static> {
    history: BrowserHistoryRef,
    saver: Saver<R>,
    controller: NavController<R>,
    /// The back stack the history shows.
    synced: RefCell<Vec<R>>,
    /// Whether [`Self::start`] marked the history.
    started: Cell<bool>,
    /// Whether the next change of the back stack came from the history,
    /// which already shows it.
    adopt: Cell<bool>,
    /// The depth a move this sync asked the history for lands at.
    landing: Cell<Option<usize>>,
}

impl<R: Clone + PartialEq + 'static> HistorySync<R> {
    /// Shows the route the page opened with, marks the current entry with the
    /// back stack, and follows the user's moves through the history.
    fn start(this: &Rc<Self>) -> BrowserHistoryObserver {
        let opened = this.fragment_route();
        if let Some(route) =
            opened.filter(|route| this.controller.current_route().as_ref() != Some(route))
        {
            this.controller.navigate(route);
        }
        let routes = this.controller.back_stack();
        this.history
            .replace(routes.len(), &this.fragment_of(routes.last()));
        *this.synced.borrow_mut() = routes;
        this.started.set(true);
        let sync = Rc::clone(this);
        this.history.observe(Rc::new(move || sync.history_moved()))
    }

    fn stack_changed(&self, routes: Vec<R>) {
        if !self.started.get() {
            return;
        }
        let previous = self.synced.replace(routes);
        let synced = self.synced.borrow();
        if *synced == previous || self.adopt.replace(false) {
            return;
        }
        let (before, after) = (previous.len(), synced.len());
        if after == before + 1 && synced.starts_with(&previous) {
            self.history.push(after, &self.fragment_of(synced.last()));
        } else if after > 0 && after < before && previous.starts_with(&synced) {
            self.landing.set(Some(after));
            self.history.go(after as isize - before as isize);
        } else if after > 0 {
            self.history
                .replace(after, &self.fragment_of(synced.last()));
        }
    }

    /// Back pops to the entry's depth; Forward, or a fragment the user typed,
    /// navigates to the entry's route.
    fn history_moved(&self) {
        let depth = self.history.depth();
        if self
            .landing
            .take()
            .is_some_and(|landing| Some(landing) == depth)
        {
            return;
        }
        let before = self.controller.back_stack();
        match depth {
            Some(depth) if depth > 0 && depth < before.len() => self.controller.pop_to_depth(depth),
            Some(depth) if depth <= before.len() => {}
            _ => {
                if let Some(route) = self.fragment_route() {
                    self.controller.navigate(route);
                }
            }
        }
        if self.controller.back_stack() != before {
            self.adopt.set(true);
        }
    }

    fn fragment_route(&self) -> Option<R> {
        let fragment = self.history.fragment();
        let saved = decode(fragment.strip_prefix('/')?)?;
        self.saver.restore(&saved)
    }

    fn fragment_of(&self, route: Option<&R>) -> String {
        route.map_or_else(String::new, |route| {
            format!("/{}", encode(&self.saver.save(route)))
        })
    }
}

/// Percent-encodes everything but the unreserved characters of RFC 3986 and
/// `/`, which a fragment may hold as it is.
fn encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

fn decode(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = tail.get(2..)?;
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}
