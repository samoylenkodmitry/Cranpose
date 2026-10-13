use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{
    LaunchArgValue, LaunchArgs, ProvideLaunchArgs, clear_platform_launch_args, is_debuggable,
    isDebuggable, launch_args, local_launch_args, set_platform_launch_args,
};

/// The platform snapshot is process-wide, so tests that install one run one at
/// a time.
static PLATFORM_SNAPSHOT: Mutex<()> = Mutex::new(());

fn platform_snapshot() -> MutexGuard<'static, ()> {
    let guard = PLATFORM_SNAPSHOT
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    clear_platform_launch_args();
    guard
}

fn run_test_composition(build: impl FnMut()) {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
}

fn debug_launch() -> Arc<LaunchArgs> {
    Arc::new(LaunchArgs::new(
        [
            ("ob_debug".to_string(), LaunchArgValue::Bool(true)),
            ("ob_level".to_string(), LaunchArgValue::Int(7)),
            (
                "ob_screen".to_string(),
                LaunchArgValue::Text("lobby".to_string()),
            ),
        ],
        true,
    ))
}

#[test]
fn integration_launch_arguments_installed_by_the_platform_reach_composition() {
    let _snapshot = platform_snapshot();
    set_platform_launch_args(debug_launch());
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let args = local_launch_args().current();
            *captured.borrow_mut() = Some((
                args.boolean("ob_debug"),
                args.int("ob_level"),
                args.string("ob_screen").map(str::to_string),
                isDebuggable(),
            ));
        });
    }

    assert_eq!(
        *captured.borrow(),
        Some((Some(true), Some(7), Some("lobby".to_string()), true))
    );
    clear_platform_launch_args();
}

#[test]
fn integration_a_replacement_snapshot_is_observed_by_the_next_render() {
    let _snapshot = platform_snapshot();
    set_platform_launch_args(debug_launch());
    assert_eq!(launch_args().int("ob_level"), Some(7));

    set_platform_launch_args(Arc::new(LaunchArgs::new(
        [("ob_level".to_string(), LaunchArgValue::Int(9))],
        true,
    )));
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let args = local_launch_args().current();
            *captured.borrow_mut() = Some((args.int("ob_level"), args.boolean("ob_debug")));
        });
    }

    assert_eq!(*captured.borrow(), Some((Some(9), None)));
    clear_platform_launch_args();
}

#[test]
fn integration_provide_launch_args_overrides_the_platform_snapshot() {
    let _snapshot = platform_snapshot();
    set_platform_launch_args(debug_launch());
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let provided = Arc::new(LaunchArgs::new(std::iter::empty(), false));
            ProvideLaunchArgs(provided, move || {
                *captured.borrow_mut() = Some((
                    local_launch_args().current().int("ob_level"),
                    isDebuggable(),
                ));
            });
        });
    }

    assert_eq!(*captured.borrow(), Some((None, false)));
    clear_platform_launch_args();
}

#[test]
fn integration_clearing_the_platform_snapshot_falls_back_to_the_command_line() {
    let _snapshot = platform_snapshot();
    set_platform_launch_args(debug_launch());
    assert!(is_debuggable());
    assert_eq!(launch_args().string("ob_screen"), Some("lobby"));

    set_platform_launch_args(Arc::new(LaunchArgs::new(std::iter::empty(), false)));
    assert!(!is_debuggable());

    clear_platform_launch_args();
    assert_eq!(launch_args().string("ob_screen"), None);
}

#[test]
fn integration_worker_threads_read_the_snapshot_the_platform_installed() {
    let _snapshot = platform_snapshot();
    set_platform_launch_args(debug_launch());

    let seen = std::thread::spawn(|| {
        let args = launch_args();
        (
            args.string("ob_screen").map(str::to_string),
            args.is_debuggable(),
        )
    })
    .join()
    .expect("worker thread finishes");

    assert_eq!(seen, (Some("lobby".to_string()), true));
    clear_platform_launch_args();
}
