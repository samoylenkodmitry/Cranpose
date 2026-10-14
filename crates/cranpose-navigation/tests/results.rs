use std::{cell::RefCell, rc::Rc};

mod support;

use cranpose_navigation::{NavController, NavHost, NavOptions, rememberNavController};
use support::{Host, Screen, SharedNav};

#[derive(Clone, Debug, PartialEq)]
struct Picked(u32);

#[derive(Clone, Debug, PartialEq)]
struct Note(&'static str);

/// What the home screen read of its results, one item per composition.
type Seen = Rc<RefCell<Vec<Option<u32>>>>;

/// The routes of the entries that the outer and the inner host give the
/// inner screen, one pair per composition.
type Found = Rc<RefCell<Vec<(Option<Screen>, Option<Screen>)>>>;

/// A host whose home screen reads the [`Picked`] result left for its entry.
fn results_host(seen: &Seen) -> (Host, SharedNav) {
    let nav = SharedNav::default();
    let (captured, seen) = (Rc::clone(&nav), Rc::clone(seen));
    let host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        let seen = Rc::clone(&seen);
        NavHost(controller, move |screen| {
            if screen == Screen::Home {
                let entry = controller
                    .composing_entry()
                    .expect("the home screen composes in its entry");
                assert_eq!(entry.route(), Some(Screen::Home));
                seen.borrow_mut()
                    .push(entry.result::<Picked>().map(|Picked(color)| color));
            }
        });
    });
    (host, nav)
}

fn controller(nav: &SharedNav) -> NavController<Screen> {
    nav.get().expect("the host composed its controller")
}

#[test]
fn a_screen_returns_a_result_to_the_screen_beneath() {
    let seen = Seen::default();
    let (mut host, nav) = results_host(&seen);
    let nav = controller(&nav);
    assert_eq!(seen.borrow().last(), Some(&None));
    nav.navigate(Screen::Detail(1));
    host.settle();
    let home = nav
        .previous_back_stack_entry()
        .expect("home is beneath the detail screen");
    assert_eq!(home.route(), Some(Screen::Home));
    home.set_result(Picked(7));
    home.set_result(Note("kept apart"));
    nav.pop_back_stack();
    host.settle();
    assert_eq!(
        seen.borrow().last(),
        Some(&Some(7)),
        "the home screen recomposed with the result"
    );
    assert_eq!(home.result::<Note>(), Some(Note("kept apart")));

    home.set_result(Picked(8));
    host.settle();
    assert_eq!(
        seen.borrow().last(),
        Some(&Some(8)),
        "a new value replaces the one of its type"
    );
    assert_eq!(home.take_result::<Picked>(), Some(Picked(8)));
    assert_eq!(home.take_result::<Picked>(), None, "taking removes it");
    host.settle();
    assert_eq!(seen.borrow().last(), Some(&None));
    assert_eq!(home.result::<Note>(), Some(Note("kept apart")));
}

#[test]
fn a_popped_entry_drops_its_results_and_a_kept_one_keeps_them() {
    let seen = Seen::default();
    let (mut host, nav) = results_host(&seen);
    let nav = controller(&nav);
    nav.navigate(Screen::Detail(1));
    nav.navigate(Screen::Settings);
    host.settle();
    let detail = nav
        .previous_back_stack_entry()
        .expect("the detail screen is beneath settings");
    assert!(nav.back_stack_entry(&Screen::Detail(1)) == Some(detail));
    assert!(nav.back_stack_entry(&Screen::Detail(2)).is_none());
    detail.set_result(Picked(1));
    nav.navigate_with(
        Screen::Home,
        NavOptions::new()
            .pop_up_to(Screen::Detail(1), true)
            .save_state(),
    );
    host.settle();
    assert_eq!(
        detail.result::<Picked>(),
        Some(Picked(1)),
        "a save_state pop keeps the entry with its results"
    );
    nav.navigate_with(Screen::Detail(1), NavOptions::new().restore_state());
    host.settle();
    assert_eq!(
        nav.back_stack(),
        [
            Screen::Home,
            Screen::Home,
            Screen::Detail(1),
            Screen::Settings
        ]
    );
    assert_eq!(detail.result::<Picked>(), Some(Picked(1)));

    assert!(nav.pop_back_stack_to(&Screen::Detail(1), true));
    host.settle();
    assert_eq!(detail.route(), None, "the entry is popped");
    assert_eq!(detail.result::<Picked>(), None, "its results went with it");
    detail.set_result(Picked(2));
    assert_eq!(detail.result::<Picked>(), None, "a popped entry takes none");

    nav.navigate(Screen::Detail(1));
    nav.navigate(Screen::Settings);
    host.settle();
    let again = nav.previous_back_stack_entry().expect("a new detail entry");
    assert!(again != detail, "a new visit is a new entry");
    assert_eq!(again.result::<Picked>(), None);
}

#[test]
fn a_host_nested_in_a_screen_finds_the_entries_of_both_back_stacks() {
    let found = Found::default();
    let captured = Rc::clone(&found);
    let _host = Host::new(move || {
        let outer = rememberNavController(Screen::Home);
        let found = Rc::clone(&captured);
        NavHost(outer, move |screen| {
            if screen != Screen::Home {
                return;
            }
            let inner = rememberNavController(Screen::Detail(5));
            let found = Rc::clone(&found);
            NavHost(inner, move |_| {
                let route_of = |nav: NavController<Screen>| {
                    nav.composing_entry().and_then(|entry| entry.route())
                };
                found.borrow_mut().push((route_of(outer), route_of(inner)));
            });
        });
    });
    assert_eq!(
        found.borrow().last(),
        Some(&(Some(Screen::Home), Some(Screen::Detail(5)))),
        "the inner screen composes in an entry of each host"
    );
}
