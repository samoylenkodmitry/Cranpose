use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod support;

use cranpose_animation::{Easing, tween};
use cranpose_navigation::{NavController, NavHost, NavHostWith, NavOptions, rememberNavController};
use support::{Host, Probe, Screen, screens};

fn controller(nav: &Rc<Cell<Option<NavController<Screen>>>>) -> NavController<Screen> {
    nav.get().expect("the host composed its controller")
}

#[test]
fn navigate_pushes_and_pop_back_stack_pops() {
    let (_host, nav) = screens(&Probe::default());
    let nav = controller(&nav);
    assert_eq!(nav.back_stack(), [Screen::Home]);
    nav.navigate(Screen::Detail(1));
    nav.navigate(Screen::Detail(1));
    assert_eq!(
        nav.back_stack(),
        [Screen::Home, Screen::Detail(1), Screen::Detail(1)],
        "navigating to the same route again pushes another entry"
    );
    assert_eq!(nav.current_route(), Some(Screen::Detail(1)));
    assert!(nav.pop_back_stack());
    assert!(nav.pop_back_stack());
    assert!(nav.pop_back_stack(), "the last entry can be popped too");
    assert_eq!(nav.current_route(), None);
    assert!(!nav.pop_back_stack(), "nothing is left to pop");
}

#[test]
fn navigate_with_pops_up_to_and_launches_single_top() {
    let (_host, nav) = screens(&Probe::default());
    let nav = controller(&nav);
    nav.navigate(Screen::Detail(1));
    nav.navigate(Screen::Detail(2));
    nav.navigate_with(
        Screen::Settings,
        NavOptions::new().pop_up_to(Screen::Home, false),
    );
    assert_eq!(nav.back_stack(), [Screen::Home, Screen::Settings]);
    nav.navigate_with(Screen::Settings, NavOptions::new().launch_single_top());
    assert_eq!(
        nav.back_stack(),
        [Screen::Home, Screen::Settings],
        "the route is already on top"
    );
    nav.navigate_with(
        Screen::Home,
        NavOptions::new()
            .pop_up_to(Screen::Home, true)
            .launch_single_top(),
    );
    assert_eq!(
        nav.back_stack(),
        [Screen::Home],
        "a fresh start destination"
    );
    nav.navigate_with(
        Screen::Settings,
        NavOptions::new().pop_up_to(Screen::Detail(9), true),
    );
    assert_eq!(
        nav.back_stack(),
        [Screen::Home, Screen::Settings],
        "popping up to a route that is not there pops nothing"
    );
}

#[test]
fn pop_back_stack_to_and_navigate_up() {
    let (_host, nav) = screens(&Probe::default());
    let nav = controller(&nav);
    for screen in [Screen::Detail(1), Screen::Detail(2), Screen::Settings] {
        nav.navigate(screen);
    }
    assert!(nav.pop_back_stack_to(&Screen::Detail(1), false));
    assert_eq!(nav.back_stack(), [Screen::Home, Screen::Detail(1)]);
    assert!(!nav.pop_back_stack_to(&Screen::Settings, false));
    assert!(
        !nav.pop_back_stack_to(&Screen::Detail(1), false),
        "already on top"
    );
    assert!(nav.pop_back_stack_to(&Screen::Detail(1), true));
    assert_eq!(nav.back_stack(), [Screen::Home]);
    nav.navigate(Screen::Settings);
    assert!(nav.navigate_up());
    assert!(!nav.navigate_up(), "the start destination stays");
    assert_eq!(nav.back_stack(), [Screen::Home]);
}

#[test]
fn the_host_shows_the_top_screen_and_crossfades_to_the_next() {
    let probe = Probe::default();
    let (mut host, nav) = screens(&probe);
    assert_eq!(probe.live(), [Screen::Home]);
    controller(&nav).navigate(Screen::Detail(1));
    host.rule.pump_until_idle().expect("pump");
    assert_eq!(
        probe.live(),
        [Screen::Home, Screen::Detail(1)],
        "both screens are shown while they crossfade"
    );
    host.settle();
    assert_eq!(probe.live(), [Screen::Detail(1)]);
}

#[test]
fn a_host_with_its_own_transition_fades_for_that_long() {
    let (probe, nav) = (Probe::default(), Rc::new(Cell::new(None)));
    let (sink, screens) = (Rc::clone(&nav), probe.clone());
    let mut host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        sink.set(Some(controller));
        let screens = screens.clone();
        NavHostWith(
            controller,
            tween(100, Easing::LinearEasing),
            move |screen| screens.screen(screen),
        );
    });
    controller(&nav).navigate(Screen::Settings);
    host.run_frames(12);
    assert_eq!(
        probe.live(),
        [Screen::Settings],
        "a 100 ms fade is over after 200 ms, well before the default 700 ms"
    );
}

#[test]
fn a_screen_keeps_its_view_models_while_covered_and_drops_them_once_popped() {
    let probe = Probe::default();
    let (mut host, nav) = screens(&probe);
    let nav = controller(&nav);
    nav.navigate(Screen::Detail(1));
    host.settle();
    assert_eq!(probe.live(), [Screen::Detail(1)]);
    assert_eq!(
        probe.dropped(),
        0,
        "the covered screen keeps its view model"
    );

    assert!(nav.pop_back_stack());
    host.rule.pump_until_idle().expect("pump");
    assert_eq!(probe.dropped(), 0, "the popped screen is still fading out");
    host.settle();
    assert_eq!(probe.live(), [Screen::Home]);
    assert_eq!(
        probe.built(),
        [Screen::Home, Screen::Detail(1)],
        "home got its view model back"
    );
    assert_eq!(probe.dropped(), 1, "the popped screen's view model is gone");

    nav.navigate(Screen::Detail(1));
    host.settle();
    assert_eq!(
        probe.built(),
        [Screen::Home, Screen::Detail(1), Screen::Detail(1)],
        "a new visit gets a new view model"
    );
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tab {
    First,
    Second,
}

#[test]
fn a_nested_nav_host_keeps_its_back_stack_while_its_screen_is_covered() {
    let outer = Rc::new(Cell::new(None));
    let inner = Rc::new(Cell::new(None));
    let (outer_sink, inner_sink) = (Rc::clone(&outer), Rc::clone(&inner));
    let mut host = Host::new(move || {
        let nav = rememberNavController(Screen::Home);
        outer_sink.set(Some(nav));
        let inner_sink = Rc::clone(&inner_sink);
        NavHost(nav, move |screen| {
            if screen == Screen::Home {
                let tabs = rememberNavController(Tab::First);
                inner_sink.set(Some(tabs));
                NavHost(tabs, |_| {});
            }
        });
    });
    let tabs = |cell: &Rc<Cell<Option<NavController<Tab>>>>| cell.get().expect("tabs composed");
    tabs(&inner).navigate(Tab::Second);
    let nav = controller(&outer);
    nav.navigate(Screen::Settings);
    host.settle();
    assert!(nav.pop_back_stack());
    host.settle();
    assert_eq!(
        tabs(&inner).back_stack(),
        [Tab::First, Tab::Second],
        "the inner back stack lives in the home entry's store"
    );
}

#[test]
fn reading_the_current_route_recomposes_on_navigation() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let nav = Rc::new(Cell::new(None));
    let (sink, captured) = (Rc::clone(&seen), Rc::clone(&nav));
    let mut host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        sink.borrow_mut().push(controller.current_route());
    });
    controller(&nav).navigate(Screen::Settings);
    host.settle();
    assert_eq!(
        seen.borrow().last(),
        Some(&Some(Screen::Settings)),
        "the reader recomposed"
    );
}
