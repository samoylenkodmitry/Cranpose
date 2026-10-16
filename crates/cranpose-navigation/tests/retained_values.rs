//! A value a screen retains outlives the screen's leaving the composition
//! while its back-stack entry lives: the screen gets it back when it shows
//! again, and the value is dropped when the entry is popped. Outside any
//! entry, the value goes as soon as its composable leaves.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod support;

use cranpose_navigation::{NavController, NavHost, rememberNavController};
use cranpose_services::retain;
use support::{DropMarker, Host, Probe, Screen, serial};

/// Something a screen keeps that no saver could write down.
struct Decoder {
    _dropped: DropMarker,
}

/// What the screens retained: how many decoders were made and dropped,
/// and which decoder each screen holds now.
#[derive(Clone, Default)]
struct Retained {
    made: Rc<Cell<usize>>,
    dropped: Arc<AtomicUsize>,
    held: Rc<RefCell<HashMap<Screen, usize>>>,
}

impl Retained {
    fn screen(&self, screen: Screen) {
        let (made, dropped) = (Rc::clone(&self.made), Arc::clone(&self.dropped));
        let decoder = retain("decoder", move || {
            made.set(made.get() + 1);
            Rc::new(Decoder {
                _dropped: DropMarker(dropped),
            })
        });
        self.held
            .borrow_mut()
            .insert(screen, Rc::as_ptr(&decoder).addr());
    }

    fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }
}

#[test]
fn a_screen_gets_its_retained_value_back_until_its_entry_is_popped() {
    let _serial = serial();
    let retained = Retained::default();
    let probe = Probe::default();
    let nav = Rc::new(Cell::new(None::<NavController<Screen>>));
    let (captured, screens, live) = (Rc::clone(&nav), retained.clone(), probe.clone());
    let mut host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        let (screens, live) = (screens.clone(), live.clone());
        NavHost(controller, move |screen| {
            live.screen(screen);
            screens.screen(screen);
        });
    });
    let nav = nav.get().expect("the host composed its controller");
    let home = retained.held.borrow()[&Screen::Home];

    nav.navigate(Screen::Detail(1));
    host.settle();
    assert_eq!(
        probe.live(),
        [Screen::Detail(1)],
        "home left the composition"
    );
    assert_eq!(retained.made.get(), 2, "each entry retains its own value");
    assert_eq!(retained.dropped(), 0, "home's entry still holds its value");

    assert!(nav.pop_back_stack());
    host.settle();
    assert_eq!(retained.made.get(), 2, "home comes back to its value");
    assert_eq!(retained.held.borrow()[&Screen::Home], home);
    assert_eq!(retained.dropped(), 1, "the popped entry's value is dropped");
}

#[test]
fn outside_any_entry_a_retained_value_goes_with_its_composable() {
    let _serial = serial();
    let retained = Retained::default();
    let toggle = Rc::new(Cell::new(None));
    let (captured, screens) = (Rc::clone(&toggle), retained.clone());
    let mut host = Host::new(move || {
        let shown = cranpose_core::rememberMutableStateOf(|| true);
        captured.set(Some(shown));
        if shown.get() {
            screens.screen(Screen::Home);
        }
    });
    let shown = toggle.get().expect("the host composed its toggle");
    shown.set(false);
    host.settle();
    assert_eq!(
        retained.dropped(),
        1,
        "the root scope keeps nothing that left"
    );
    shown.set(true);
    host.settle();
    assert_eq!(retained.made.get(), 2, "the composable makes a new value");
}
