//! A view model built in a back-stack entry gets that entry's
//! `SavedStateHandle`: two visits of one route keep their keys apart, a
//! restored back stack brings the values back, and a popped entry leaves
//! nothing in the store.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

mod support;

use cranpose_coroflow::{SavedStateHandle, viewModel};
use cranpose_navigation::{
    NavController, NavHost, NavOptions, rememberNavController, rememberSaveableNavController,
};
use cranpose_services::{DurableSaveOutcome, PreferencesStore, Saver, run_durable_saves};
use support::{Host, Screen, fresh_store, save, serial};

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

/// A screen's view model, holding its entry's saved state.
struct Draft {
    saved: SavedStateHandle,
}

/// The saved state of the screen composed last for each route.
#[derive(Clone, Default)]
struct Drafts(Rc<RefCell<HashMap<Screen, SavedStateHandle>>>);

impl Drafts {
    fn of(&self, screen: Screen) -> Option<i32> {
        self.0.borrow()[&screen].get("draft").expect("one type")
    }

    fn set(&self, screen: Screen, draft: i32) {
        self.0.borrow()[&screen]
            .set("draft", &draft)
            .expect("one type");
    }
}

struct App {
    host: Host,
    nav: Rc<Cell<Option<NavController<Screen>>>>,
    drafts: Drafts,
}

impl App {
    fn new(saveable: bool) -> Self {
        let (nav, drafts) = (Rc::new(Cell::new(None)), Drafts::default());
        let (captured, screens) = (Rc::clone(&nav), drafts.clone());
        let host = Host::new(move || {
            let controller = if saveable {
                rememberSaveableNavController("nav", Screen::Home, routes())
            } else {
                rememberNavController(Screen::Home)
            };
            captured.set(Some(controller));
            let screens = screens.clone();
            NavHost(controller, move |screen| {
                let draft = viewModel((), |_| Draft {
                    saved: SavedStateHandle::current(),
                });
                screens
                    .0
                    .borrow_mut()
                    .insert(screen, draft.get().saved.clone());
            });
        });
        Self { host, nav, drafts }
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
fn each_visit_of_a_route_has_its_own_saved_state() {
    let _serial = serial();
    fresh_store();
    for saveable in [false, true] {
        let mut app = App::new(saveable);
        app.navigate(Screen::Detail(1));
        app.drafts.set(Screen::Detail(1), 7);
        app.navigate(Screen::Detail(1));
        assert_eq!(app.drafts.of(Screen::Detail(1)), None, "a second visit");
        app.drafts.set(Screen::Detail(1), 9);
        app.pop();
        assert_eq!(app.drafts.of(Screen::Detail(1)), Some(7));
    }
}

#[test]
fn a_restored_back_stack_gets_its_entries_saved_state_back() {
    let _serial = serial();
    fresh_store();
    let mut app = App::new(true);
    app.navigate(Screen::Detail(3));
    app.drafts.set(Screen::Detail(3), 5);
    app.navigate(Screen::Settings);
    app.drafts.set(Screen::Settings, 6);
    save();
    drop(app);

    let mut app = App::new(true);
    assert_eq!(app.drafts.of(Screen::Settings), Some(6));
    app.pop();
    assert_eq!(app.drafts.of(Screen::Detail(3)), Some(5));
}

#[test]
fn a_popped_entry_leaves_no_saved_state_in_the_store() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(true);
    save();
    let saved = store.keys();
    app.navigate(Screen::Detail(3));
    app.drafts.set(Screen::Detail(3), 5);
    save();
    assert!(store.keys().len() > saved.len());
    // The popped screen's handle outlives its entry here.
    let held = app.drafts.0.borrow()[&Screen::Detail(3)].clone();
    app.pop();
    save();
    held.set("draft", &8).expect("one type");
    save();
    assert_eq!(store.keys(), saved);
}

#[test]
fn an_in_memory_back_stack_saves_no_saved_state() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(false);
    app.navigate(Screen::Detail(1));
    app.drafts.set(Screen::Detail(1), 7);
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Nothing
    );
    assert!(store.keys().is_empty());
}

#[test]
fn a_kept_tab_comes_back_after_a_restart() {
    let _serial = serial();
    fresh_store();
    let tab = |nav: NavController<Screen>, tab: Screen| {
        nav.navigate_with(
            tab,
            NavOptions::new()
                .pop_up_to(Screen::Home, false)
                .save_state()
                .launch_single_top()
                .restore_state(),
        );
    };
    let mut app = App::new(true);
    tab(app.nav(), Screen::Settings);
    app.navigate(Screen::Detail(2));
    app.drafts.set(Screen::Detail(2), 4);
    tab(app.nav(), Screen::Home);
    app.host.settle();
    save();
    drop(app);

    let mut app = App::new(true);
    assert_eq!(app.nav().back_stack(), [Screen::Home]);
    tab(app.nav(), Screen::Settings);
    app.host.settle();
    assert_eq!(
        app.nav().back_stack(),
        [Screen::Home, Screen::Settings, Screen::Detail(2)]
    );
    assert_eq!(app.drafts.of(Screen::Detail(2)), Some(4));
}

#[test]
fn a_back_stack_saved_by_an_older_build_restores() {
    let _serial = serial();
    let store = fresh_store();
    let mut app = App::new(true);
    app.navigate(Screen::Detail(3));
    save();
    drop(app);
    let key = store
        .keys()
        .into_iter()
        .find(|key| store.get(key).is_some_and(|value| value.starts_with("2 ")))
        .expect("the saved back stack");
    // The form builds before saved tab stacks wrote: no counts, no segments.
    store
        .set(&key, "1 2 0 4 home 1 8 detail 3")
        .expect("an older save");

    let app = App::new(true);
    assert_eq!(app.nav().back_stack(), [Screen::Home, Screen::Detail(3)]);
}
