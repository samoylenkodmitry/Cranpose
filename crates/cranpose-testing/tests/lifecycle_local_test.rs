// A screen that reads `local_lifecycle_state()` follows the lifecycle of the
// host in every shell, as a screen that reads Compose's `LocalLifecycleOwner`
// follows its activity. No shell wraps its content for it: the headless shell
// of the robot stands for them all.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Mutex, MutexGuard, PoisonError},
};

use cranpose::{LifecycleState, ProvideLifecycle, dispatch_lifecycle_state, local_lifecycle_state};
use cranpose_macros::composable;
use cranpose_testing::{
    ComposeTestRule,
    robot::{RobotTestRule, TestRenderer, create_headless_robot_test},
};

// The lifecycle state belongs to the process, so tests that move it run one
// at a time, each from a stopped host.
static PROCESS_LIFECYCLE: Mutex<()> = Mutex::new(());

fn stopped_host() -> MutexGuard<'static, ()> {
    let guard = PROCESS_LIFECYCLE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    dispatch_lifecycle_state(LifecycleState::Stopped);
    guard
}

// The states a screen read, in the order it read them.
type Reads = Rc<RefCell<Vec<LifecycleState>>>;

#[composable]
fn Screen(reads: Reads) {
    reads.borrow_mut().push(local_lifecycle_state().current());
}

#[composable]
fn Bystander(runs: Rc<Cell<usize>>) {
    runs.set(runs.get() + 1);
}

fn shell_with_screen(reads: &Reads) -> RobotTestRule<TestRenderer> {
    let screen = Rc::clone(reads);
    create_headless_robot_test(100, 100, move || Screen(Rc::clone(&screen)))
}

// An app of a screen that reads the local and a screen that reads nothing,
// with a count of the runs of its content and of the second screen.
#[derive(Default)]
struct App {
    reads: Reads,
    content_runs: Cell<usize>,
    bystander_runs: Rc<Cell<usize>>,
}

impl App {
    fn compose(&self) {
        self.content_runs.set(self.content_runs.get() + 1);
        Screen(Rc::clone(&self.reads));
        Bystander(Rc::clone(&self.bystander_runs));
    }

    // Moves the host to `Resumed` and checks that only the screen that reads
    // the local composed again.
    fn resume_and_check(&self, shell: &mut RobotTestRule<TestRenderer>) {
        dispatch_lifecycle_state(LifecycleState::Resumed);
        shell.wait_for_idle();
        assert_eq!(
            *self.reads.borrow(),
            [LifecycleState::Stopped, LifecycleState::Resumed],
            "the screen that reads the local did not follow the move"
        );
        assert_eq!(self.content_runs.get(), 1, "the app content ran again");
        assert_eq!(
            self.bystander_runs.get(),
            1,
            "a screen that reads nothing ran again"
        );
    }
}

#[test]
fn a_screen_reading_the_lifecycle_local_follows_the_host() {
    use LifecycleState::{Paused, Resumed, Stopped};
    let _host = stopped_host();
    let reads = Reads::default();
    let mut shell = shell_with_screen(&reads);
    assert_eq!(*reads.borrow(), [Stopped]);

    for state in [Resumed, Paused, Stopped] {
        dispatch_lifecycle_state(state);
        shell.wait_for_idle();
    }
    assert_eq!(*reads.borrow(), [Stopped, Resumed, Paused, Stopped]);

    shell.wait_for_idle();
    assert_eq!(
        reads.borrow().len(),
        4,
        "a frame with no move reads nothing"
    );
}

#[test]
fn a_lifecycle_move_composes_only_the_screens_that_read_it() {
    let _host = stopped_host();
    let app = Rc::new(App::default());
    let content = Rc::clone(&app);
    let mut shell = create_headless_robot_test(100, 100, move || content.compose());
    app.resume_and_check(&mut shell);
}

#[test]
fn an_explicit_provider_composes_only_the_screens_that_read_the_local() {
    let _host = stopped_host();
    let app = Rc::new(App::default());
    let content = Rc::clone(&app);
    let mut shell = create_headless_robot_test(100, 100, move || {
        let content = Rc::clone(&content);
        ProvideLifecycle(move || content.compose());
    });
    app.resume_and_check(&mut shell);
}

#[test]
fn every_shell_of_a_thread_follows_the_host() {
    use LifecycleState::{Paused, Resumed, Stopped};
    let _host = stopped_host();
    let (first_reads, second_reads, third_reads) =
        (Reads::default(), Reads::default(), Reads::default());
    let mut first = shell_with_screen(&first_reads);
    let mut second = shell_with_screen(&second_reads);

    dispatch_lifecycle_state(Resumed);
    first.wait_for_idle();
    second.wait_for_idle();
    assert_eq!(*first_reads.borrow(), [Stopped, Resumed]);
    assert_eq!(*second_reads.borrow(), [Stopped, Resumed]);

    // The first shell is gone before the host moves, and a third one starts.
    drop(first);
    dispatch_lifecycle_state(Paused);
    let mut third = shell_with_screen(&third_reads);
    dispatch_lifecycle_state(Stopped);
    second.wait_for_idle();
    third.wait_for_idle();
    assert_eq!(*second_reads.borrow(), [Stopped, Resumed, Stopped]);
    assert_eq!(*third_reads.borrow(), [Paused, Stopped]);
}

#[test]
fn the_compose_test_rule_follows_the_host() {
    let _host = stopped_host();
    let reads = Reads::default();
    let screen = Rc::clone(&reads);
    let mut rule = ComposeTestRule::new();
    rule.set_content(move || Screen(Rc::clone(&screen)))
        .expect("first composition");
    assert_eq!(*reads.borrow(), [LifecycleState::Stopped]);

    dispatch_lifecycle_state(LifecycleState::Resumed);
    rule.pump_until_idle().expect("recomposition");
    assert_eq!(
        *reads.borrow(),
        [LifecycleState::Stopped, LifecycleState::Resumed]
    );
}

#[test]
fn a_move_made_on_another_thread_reaches_the_screen() {
    let _host = stopped_host();
    let reads = Reads::default();
    let mut shell = shell_with_screen(&reads);

    std::thread::spawn(|| dispatch_lifecycle_state(LifecycleState::Resumed))
        .join()
        .expect("the host thread");
    shell.wait_for_idle();
    assert_eq!(
        *reads.borrow(),
        [LifecycleState::Stopped, LifecycleState::Resumed]
    );
}
