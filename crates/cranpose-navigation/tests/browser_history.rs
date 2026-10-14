mod support;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_navigation::{
    BindBrowserHistory, NavController, NavHost, NavOptions, rememberNavController,
};
use cranpose_services::{
    BrowserHistory, BrowserHistoryObserver, Saver, set_platform_browser_history,
};
use support::{Host, Screen};

/// A browser's session history: entries of a kept depth and a fragment, and
/// the current one. Moves reach the observer once the test delivers them, as
/// a browser's popstate arrives after the call that moved.
#[derive(Default)]
struct FakeHistory {
    entries: RefCell<Vec<(Option<usize>, String)>>,
    current: Cell<usize>,
    on_move: RefCell<Option<Rc<dyn Fn()>>>,
    pending_moves: Cell<usize>,
}

impl FakeHistory {
    fn opened_at(fragment: &str) -> Rc<Self> {
        let history = Rc::new(Self::default());
        history
            .entries
            .borrow_mut()
            .push((None, fragment.to_owned()));
        history
    }

    fn entries(&self) -> Vec<(Option<usize>, String)> {
        self.entries.borrow().clone()
    }

    fn current(&self) -> (Option<usize>, String) {
        self.entries.borrow()[self.current.get()].clone()
    }

    fn deliver(&self) {
        while self.pending_moves.get() > 0 {
            self.pending_moves.set(self.pending_moves.get() - 1);
            let on_move = self.on_move.borrow().clone();
            if let Some(on_move) = on_move {
                on_move();
            }
        }
    }

    /// The browser's Back or Forward button.
    fn press(&self, delta: isize) {
        self.go(delta);
        self.deliver();
    }

    /// The user types a fragment into the address bar.
    fn type_fragment(&self, fragment: &str) {
        self.push_entry(None, fragment);
        self.pending_moves.set(self.pending_moves.get() + 1);
        self.deliver();
    }

    fn push_entry(&self, depth: Option<usize>, fragment: &str) {
        let mut entries = self.entries.borrow_mut();
        entries.truncate(self.current.get() + 1);
        entries.push((depth, fragment.to_owned()));
        self.current.set(entries.len() - 1);
    }
}

impl BrowserHistory for FakeHistory {
    fn fragment(&self) -> String {
        self.current().1
    }

    fn depth(&self) -> Option<usize> {
        self.current().0
    }

    fn push(&self, depth: usize, fragment: &str) {
        self.push_entry(Some(depth), fragment);
    }

    fn replace(&self, depth: usize, fragment: &str) {
        self.entries.borrow_mut()[self.current.get()] = (Some(depth), fragment.to_owned());
    }

    fn go(&self, delta: isize) {
        let target = self.current.get() as isize + delta;
        let last = self.entries.borrow().len() as isize - 1;
        self.current.set(target.clamp(0, last) as usize);
        self.pending_moves.set(self.pending_moves.get() + 1);
    }

    fn observe(&self, on_move: Rc<dyn Fn()>) -> BrowserHistoryObserver {
        *self.on_move.borrow_mut() = Some(on_move);
        BrowserHistoryObserver::new(())
    }
}

fn routes() -> Saver<Screen> {
    Saver::new(
        |screen: &Screen| match screen {
            Screen::Home => "home".to_owned(),
            Screen::Detail(id) => format!("detail {id}"),
            Screen::Settings => "settings".to_owned(),
        },
        |saved| match saved {
            "home" => Some(Screen::Home),
            "settings" => Some(Screen::Settings),
            _ => saved
                .strip_prefix("detail ")?
                .parse()
                .ok()
                .map(Screen::Detail),
        },
    )
}

struct App {
    host: Host,
    nav: Rc<Cell<Option<NavController<Screen>>>>,
    history: Rc<FakeHistory>,
}

impl App {
    fn opened_at(fragment: &str) -> Self {
        let history = FakeHistory::opened_at(fragment);
        set_platform_browser_history(history.clone());
        let nav = Rc::new(Cell::new(None));
        let captured = Rc::clone(&nav);
        let host = Host::new(move || {
            let controller = rememberNavController(Screen::Home);
            captured.set(Some(controller));
            BindBrowserHistory(controller, routes());
            NavHost(controller, |_| {});
        });
        Self { host, nav, history }
    }

    fn nav(&self) -> NavController<Screen> {
        self.nav.get().expect("composed controller")
    }

    /// Delivers the history's moves and settles the frames they cause.
    fn settle(&mut self) {
        self.host.settle();
        self.history.deliver();
        self.host.settle();
    }
}

#[test]
fn navigating_adds_history_entries_with_the_route_in_the_fragment() {
    let mut app = App::opened_at("");
    assert_eq!(app.history.entries(), [(Some(1), "/home".to_owned())]);
    app.nav().navigate(Screen::Detail(3));
    app.settle();
    assert_eq!(
        app.history.entries(),
        [
            (Some(1), "/home".to_owned()),
            (Some(2), "/detail%203".to_owned())
        ]
    );
    assert_eq!(app.history.current().1, "/detail%203");
}

#[test]
fn back_and_forward_in_the_browser_pop_and_navigate_again() {
    let mut app = App::opened_at("");
    app.nav().navigate(Screen::Detail(3));
    app.settle();
    app.history.press(-1);
    app.settle();
    assert_eq!(app.nav().back_stack(), [Screen::Home]);
    app.history.press(1);
    app.settle();
    assert_eq!(app.nav().back_stack(), [Screen::Home, Screen::Detail(3)]);
    assert_eq!(
        app.history.entries().len(),
        2,
        "Back and Forward add no entries"
    );
}

#[test]
fn a_pop_in_the_app_goes_back_in_the_history() {
    let mut app = App::opened_at("");
    app.nav().navigate(Screen::Detail(3));
    app.settle();
    app.nav().navigate(Screen::Settings);
    app.settle();
    assert!(app.nav().pop_back_stack_to(&Screen::Home, false));
    app.settle();
    assert_eq!(app.history.current(), (Some(1), "/home".to_owned()));
    assert_eq!(
        app.nav().back_stack(),
        [Screen::Home],
        "the history's move does not pop again"
    );
}

#[test]
fn a_page_opened_at_a_route_shows_it_over_the_start() {
    let mut app = App::opened_at("/detail%207");
    app.settle();
    assert_eq!(app.nav().back_stack(), [Screen::Home, Screen::Detail(7)]);
    assert_eq!(app.history.entries(), [(Some(2), "/detail%207".to_owned())]);
}

#[test]
fn a_typed_fragment_navigates_and_an_unreadable_one_does_nothing() {
    let mut app = App::opened_at("");
    app.history.type_fragment("/settings");
    app.settle();
    assert_eq!(app.nav().back_stack(), [Screen::Home, Screen::Settings]);
    app.history.type_fragment("/nowhere");
    app.settle();
    assert_eq!(app.nav().back_stack(), [Screen::Home, Screen::Settings]);
}

#[test]
fn a_navigation_that_replaces_screens_replaces_the_current_entry() {
    let mut app = App::opened_at("");
    app.nav().navigate(Screen::Detail(3));
    app.settle();
    app.nav().navigate_with(
        Screen::Settings,
        NavOptions::new().pop_up_to(Screen::Home, false),
    );
    app.settle();
    assert_eq!(
        app.history.entries(),
        [
            (Some(1), "/home".to_owned()),
            (Some(2), "/settings".to_owned())
        ]
    );
}
