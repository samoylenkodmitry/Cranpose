use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError},
    time::Duration,
};

mod support;

use cranpose_core::MutableState;
use cranpose_navigation::{
    NavController, NavHost, rememberNavController, rememberSaveableNavController,
};
use cranpose_services::{
    DurableSaveOutcome, MemoryPreferences, PreferencesStore, Saver, rememberSaveable,
    run_durable_saves, set_platform_preferences,
};
use support::{Host, Screen};

/// The tests share the platform preferences store.
fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn fresh_store() -> Arc<MemoryPreferences> {
    let store = Arc::new(MemoryPreferences::new());
    set_platform_preferences(store.clone());
    store
}

fn save() {
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
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

/// A saver from a build that no longer has the settings screen.
fn routes_without_settings() -> Saver<Screen> {
    let routes = routes();
    Saver::new(
        move |screen: &Screen| routes.save(screen),
        |saved| match saved {
            "home" => Some(Screen::Home),
            _ => saved
                .strip_prefix("detail ")?
                .parse()
                .ok()
                .map(Screen::Detail),
        },
    )
}

/// The `rememberSaveable` counter of the screen composed last for each route.
#[derive(Clone, Default)]
struct Counts(Rc<RefCell<HashMap<Screen, MutableState<i32>>>>);

impl Counts {
    fn screen(&self, screen: Screen) {
        let count = rememberSaveable("count", Saver::of_display(), || 0);
        self.0.borrow_mut().insert(screen, count);
    }

    fn of(&self, screen: Screen) -> i32 {
        self.0.borrow()[&screen].get()
    }

    fn set(&self, screen: Screen, value: i32) {
        self.0.borrow()[&screen].set(value);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    InMemory,
    Saveable(fn() -> Saver<Screen>),
}

struct App {
    host: Host,
    nav: Rc<Cell<Option<NavController<Screen>>>>,
    counts: Counts,
}

impl App {
    fn new(kind: Kind) -> Self {
        let (nav, counts) = (Rc::new(Cell::new(None)), Counts::default());
        let (captured, screens) = (Rc::clone(&nav), counts.clone());
        let host = Host::new(move || {
            let controller = match kind {
                Kind::InMemory => rememberNavController(Screen::Home),
                Kind::Saveable(saver) => {
                    rememberSaveableNavController("nav", Screen::Home, saver())
                }
            };
            captured.set(Some(controller));
            let screens = screens.clone();
            NavHost(controller, move |screen| screens.screen(screen));
        });
        Self { host, nav, counts }
    }

    fn nav(&self) -> NavController<Screen> {
        self.nav.get().expect("the host composed its controller")
    }

    fn navigate(&mut self, screen: Screen) {
        self.nav().navigate(screen);
        self.host.settle();
    }

    fn pop(&mut self) {
        assert!(self.nav().pop_back_stack());
        self.host.settle();
    }
}

#[test]
fn a_covered_screen_gets_its_saveable_state_back() {
    let _serial = serial();
    fresh_store();
    let mut app = App::new(Kind::InMemory);
    app.counts.set(Screen::Home, 5);
    app.navigate(Screen::Detail(1));
    app.pop();
    assert_eq!(app.counts.of(Screen::Home), 5);
}

#[test]
fn each_visit_of_a_route_keeps_its_own_saveable_state() {
    let _serial = serial();
    fresh_store();
    let mut app = App::new(Kind::InMemory);
    app.navigate(Screen::Detail(1));
    app.counts.set(Screen::Detail(1), 7);
    app.navigate(Screen::Detail(1));
    assert_eq!(
        app.counts.of(Screen::Detail(1)),
        0,
        "a second visit starts fresh"
    );
    app.counts.set(Screen::Detail(1), 9);
    app.pop();
    assert_eq!(
        app.counts.of(Screen::Detail(1)),
        7,
        "the first visit keeps its own"
    );
    app.pop();
    app.navigate(Screen::Detail(1));
    assert_eq!(
        app.counts.of(Screen::Detail(1)),
        0,
        "a popped visit forgot its state"
    );
}

#[test]
fn an_in_memory_back_stack_saves_nothing() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(Kind::InMemory);
    app.counts.set(Screen::Home, 5);
    app.navigate(Screen::Detail(1));
    app.counts.set(Screen::Detail(1), 7);
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Nothing
    );
    drop(app);
    assert!(store.keys().is_empty());
    let app = App::new(Kind::InMemory);
    assert_eq!(app.nav().back_stack(), [Screen::Home]);
    assert_eq!(app.counts.of(Screen::Home), 0);
}

#[test]
fn a_saved_back_stack_and_its_screens_state_survive_recreation() {
    let _serial = serial();
    fresh_store();
    let mut app = App::new(Kind::Saveable(routes));
    app.counts.set(Screen::Home, 1);
    app.navigate(Screen::Detail(3));
    app.counts.set(Screen::Detail(3), 3);
    app.navigate(Screen::Detail(3));
    app.counts.set(Screen::Detail(3), 33);
    app.navigate(Screen::Settings);
    app.counts.set(Screen::Settings, 4);
    save();
    drop(app);

    let mut app = App::new(Kind::Saveable(routes));
    assert_eq!(
        app.nav().back_stack(),
        [
            Screen::Home,
            Screen::Detail(3),
            Screen::Detail(3),
            Screen::Settings
        ]
    );
    assert_eq!(app.counts.of(Screen::Settings), 4);
    app.pop();
    assert_eq!(app.counts.of(Screen::Detail(3)), 33);
    app.pop();
    assert_eq!(app.counts.of(Screen::Detail(3)), 3);
    app.pop();
    assert_eq!(app.counts.of(Screen::Home), 1);
    app.navigate(Screen::Settings);
    assert_eq!(
        app.counts.of(Screen::Settings),
        0,
        "a new visit after restoring does not reuse a restored id"
    );
}

#[test]
fn popped_screens_leave_nothing_in_the_store() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(Kind::Saveable(routes));
    save();
    let saved = store.keys();
    app.navigate(Screen::Detail(3));
    app.counts.set(Screen::Detail(3), 3);
    app.navigate(Screen::Settings);
    app.counts.set(Screen::Settings, 4);
    save();
    assert!(store.keys().len() > saved.len());
    assert!(app.nav().pop_back_stack_to(&Screen::Home, false));
    app.host.settle();
    save();
    assert_eq!(store.keys(), saved);
}

#[test]
fn a_screen_popped_during_its_transition_saves_nothing_more() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(Kind::Saveable(routes));
    save();
    let saved = store.keys();
    app.nav().navigate(Screen::Detail(3));
    app.host.run_frames(5);
    app.counts.set(Screen::Detail(3), 3);
    assert!(app.nav().pop_back_stack());
    app.host.run_frames(5);
    save();
    app.host.settle();
    save();
    assert_eq!(store.keys(), saved);
    drop(app);
    let app = App::new(Kind::Saveable(routes));
    assert_eq!(app.nav().back_stack(), [Screen::Home]);
}

#[test]
fn restoring_keeps_the_entries_beneath_a_route_the_saver_cannot_read() {
    let _serial = serial();
    fresh_store();
    let mut app = App::new(Kind::Saveable(routes));
    app.navigate(Screen::Detail(3));
    app.counts.set(Screen::Detail(3), 3);
    app.navigate(Screen::Settings);
    app.navigate(Screen::Detail(4));
    save();
    drop(app);

    let app = App::new(Kind::Saveable(routes_without_settings));
    assert_eq!(app.nav().back_stack(), [Screen::Home, Screen::Detail(3)]);
    assert_eq!(app.counts.of(Screen::Detail(3)), 3);
}

#[test]
fn a_back_stack_saved_in_an_unreadable_form_starts_fresh() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(Kind::Saveable(routes));
    app.counts.set(Screen::Home, 1);
    app.navigate(Screen::Detail(3));
    save();
    drop(app);
    store
        .set("nav", "a form from another build")
        .expect("write");

    let app = App::new(Kind::Saveable(routes));
    assert_eq!(app.nav().back_stack(), [Screen::Home]);
    assert_eq!(
        app.counts.of(Screen::Home),
        0,
        "the fresh start screen does not take the old one's state"
    );
}

/// A host whose settings screen holds a nested saveable `NavHost`.
struct NestedApp {
    host: Host,
    outer: Rc<Cell<Option<NavController<Screen>>>>,
    inner: Rc<Cell<Option<NavController<Screen>>>>,
    counts: Counts,
}

impl NestedApp {
    fn new() -> Self {
        let (outer, inner, counts) = (
            Rc::new(Cell::new(None)),
            Rc::new(Cell::new(None)),
            Counts::default(),
        );
        let (captured_outer, captured_inner, screens) =
            (Rc::clone(&outer), Rc::clone(&inner), counts.clone());
        let host = Host::new(move || {
            let controller = rememberSaveableNavController("nav", Screen::Home, routes());
            captured_outer.set(Some(controller));
            let (captured_inner, screens) = (Rc::clone(&captured_inner), screens.clone());
            NavHost(controller, move |screen| {
                if screen != Screen::Settings {
                    return;
                }
                let nested = rememberSaveableNavController("inner", Screen::Home, routes());
                captured_inner.set(Some(nested));
                let screens = screens.clone();
                NavHost(nested, move |screen| screens.screen(screen));
            });
        });
        Self {
            host,
            outer,
            inner,
            counts,
        }
    }

    fn outer(&self) -> NavController<Screen> {
        self.outer.get().expect("outer controller")
    }

    fn inner(&self) -> NavController<Screen> {
        self.inner.get().expect("inner controller")
    }
}

#[test]
fn a_nested_saved_back_stack_restores_inside_its_screen_and_goes_with_it() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = NestedApp::new();
    save();
    let saved = store.keys();
    app.outer().navigate(Screen::Settings);
    app.host.settle();
    app.inner().navigate(Screen::Detail(5));
    app.host.settle();
    app.counts.set(Screen::Detail(5), 5);
    save();
    drop(app);

    let mut app = NestedApp::new();
    assert_eq!(app.outer().back_stack(), [Screen::Home, Screen::Settings]);
    assert_eq!(app.inner().back_stack(), [Screen::Home, Screen::Detail(5)]);
    assert_eq!(app.counts.of(Screen::Detail(5)), 5);
    assert!(app.outer().pop_back_stack());
    app.host.settle();
    save();
    assert_eq!(store.keys(), saved);
}
