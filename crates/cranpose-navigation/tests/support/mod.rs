#![allow(dead_code)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use cranpose_core::DisposableEffect;
use cranpose_coroflow::viewModel;
use cranpose_navigation::{NavController, NavHost, rememberNavController};
use cranpose_testing::ComposeTestRule;

/// Long enough for Compose's 700 ms fade to finish.
const SETTLE_FRAMES: u64 = 60;
const FRAME_NANOS: u64 = 16_666_667;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Screen {
    Home,
    Detail(u32),
    Settings,
}

/// Counts drops, so a test sees when a view model is dropped.
pub struct DropMarker(pub Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A screen's view model: it records which screen built it.
pub struct ScreenModel {
    pub screen: Screen,
    _dropped: DropMarker,
}

/// What the screens of a test host saw.
#[derive(Clone, Default)]
pub struct Probe {
    /// The screens in the composition right now.
    pub live: Rc<RefCell<Vec<Screen>>>,
    /// The screens whose view model was built, in order.
    pub built: Rc<RefCell<Vec<Screen>>>,
    /// How many view models were dropped.
    pub dropped: Arc<AtomicUsize>,
}

impl Probe {
    pub fn live(&self) -> Vec<Screen> {
        self.live.borrow().clone()
    }

    pub fn built(&self) -> Vec<Screen> {
        self.built.borrow().clone()
    }

    pub fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }

    /// A screen that reports when it enters and leaves the composition and
    /// asks for its view model.
    pub fn screen(&self, screen: Screen) {
        let live = Rc::clone(&self.live);
        DisposableEffect(screen, move |scope| {
            live.borrow_mut().push(screen);
            scope.on_dispose(move || live.borrow_mut().retain(|shown| *shown != screen))
        });
        let (built, dropped) = (Rc::clone(&self.built), Arc::clone(&self.dropped));
        let model = viewModel((), move |_| {
            built.borrow_mut().push(screen);
            ScreenModel {
                screen,
                _dropped: DropMarker(dropped),
            }
        });
        assert_eq!(model.get().screen, screen, "each entry has its own store");
    }
}

/// A composition driven frame by frame.
pub struct Host {
    pub rule: ComposeTestRule,
    now: u64,
}

impl Host {
    pub fn new(content: impl FnMut() + 'static) -> Self {
        let mut rule = ComposeTestRule::new();
        rule.set_content(content).expect("compose");
        let mut host = Self { rule, now: 0 };
        host.settle();
        host
    }

    /// Delivers queued UI work, such as back requests, and runs frames until
    /// every transition has finished.
    pub fn settle(&mut self) {
        for _ in 0..SETTLE_FRAMES {
            self.rule.runtime_handle().drain_ui();
            self.now += FRAME_NANOS;
            self.rule.advance_frame(self.now).expect("frame");
        }
    }
}

/// A host whose `NavHost` starts at [`Screen::Home`] and shows every screen
/// through `probe`, with its controller in `nav`.
pub fn screens(probe: &Probe) -> (Host, Rc<Cell<Option<NavController<Screen>>>>) {
    let nav = Rc::new(Cell::new(None));
    let (captured, probe) = (Rc::clone(&nav), probe.clone());
    let host = Host::new(move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        let probe = probe.clone();
        NavHost(controller, move |screen| probe.screen(screen));
    });
    (host, nav)
}
