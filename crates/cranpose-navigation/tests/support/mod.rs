#![allow(dead_code, reason = "each test binary uses a part of the support")]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use cranpose_services::{
    DurableSaveOutcome, MemoryPreferences, run_durable_saves, set_platform_preferences,
};

pub(crate) fn serial() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn fresh_store() -> Arc<MemoryPreferences> {
    let store = Arc::new(MemoryPreferences::new());
    set_platform_preferences(Arc::<MemoryPreferences>::clone(&store));
    store
}

pub(crate) fn save() {
    assert_eq!(
        run_durable_saves(Duration::from_secs(5)),
        DurableSaveOutcome::Completed
    );
}

use cranpose_animation::{Easing, tween};
use cranpose_core::DisposableEffect;
use cranpose_coroflow::viewModel;
use cranpose_navigation::{NavController, NavHost, NavHostWithDialogs, rememberNavController};
use cranpose_testing::{ComposeTestRule, RobotTestRule, TestRenderer, create_headless_robot_test};
use cranpose_ui::widgets::DialogSpec;

/// Long enough for Compose's 700 ms fade to finish.
const SETTLE_FRAMES: u64 = 60;
const FRAME_NANOS: u64 = 16_666_667;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Screen {
    Home,
    Detail(u32),
    Settings,
}

pub(crate) struct DropMarker(pub Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

pub(crate) struct ScreenModel {
    pub screen: Screen,
    _dropped: DropMarker,
}

#[derive(Clone, Default)]
pub(crate) struct Probe {
    /// The screens in the composition right now.
    pub live: Rc<RefCell<Vec<Screen>>>,
    /// The screens whose view model was built, in order.
    pub built: Rc<RefCell<Vec<Screen>>>,
    /// How many view models were dropped.
    pub dropped: Arc<AtomicUsize>,
}

impl Probe {
    pub(crate) fn live(&self) -> Vec<Screen> {
        self.live.borrow().clone()
    }

    pub(crate) fn built(&self) -> Vec<Screen> {
        self.built.borrow().clone()
    }

    pub(crate) fn dropped(&self) -> usize {
        self.dropped.load(Ordering::SeqCst)
    }

    pub(crate) fn screen(&self, screen: Screen) {
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

pub(crate) struct Host {
    pub rule: ComposeTestRule,
    now: u64,
}

impl Host {
    pub(crate) fn new(content: impl FnMut() + 'static) -> Self {
        let mut rule = ComposeTestRule::new();
        rule.set_content(content).expect("compose");
        let mut host = Self { rule, now: 0 };
        host.settle();
        host
    }

    pub(crate) fn settle(&mut self) {
        self.run_frames(SETTLE_FRAMES);
    }

    pub(crate) fn run_frames(&mut self, count: u64) {
        for _ in 0..count {
            self.rule.runtime_handle().drain_ui();
            self.now += FRAME_NANOS;
            self.rule.advance_frame(self.now).expect("frame");
        }
    }
}

pub(crate) fn screens(probe: &Probe) -> (Host, Rc<Cell<Option<NavController<Screen>>>>) {
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

pub(crate) type SharedNav = Rc<Cell<Option<NavController<Screen>>>>;

pub(crate) fn with_settings_dialog(probe: &Probe) -> (RobotTestRule<TestRenderer>, SharedNav) {
    let nav = Rc::new(Cell::new(None));
    let (captured, probe) = (Rc::clone(&nav), probe.clone());
    let mut robot = create_headless_robot_test(300, 400, move || {
        let controller = rememberNavController(Screen::Home);
        captured.set(Some(controller));
        let probe = probe.clone();
        NavHostWithDialogs(
            controller,
            tween(700, Easing::FastOutSlowInEasing),
            |screen| (*screen == Screen::Settings).then(DialogSpec::default),
            move |screen| probe.screen(screen),
        );
    });
    settle_robot(&mut robot);
    (robot, nav)
}

pub(crate) fn settle_robot(robot: &mut RobotTestRule<TestRenderer>) {
    for _ in 0..SETTLE_FRAMES {
        robot.advance_time(FRAME_NANOS);
        robot.wait_for_idle();
    }
}
