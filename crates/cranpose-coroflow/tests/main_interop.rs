use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex, PoisonError},
    thread::{self, ThreadId},
};

mod support;

use coroflow::{Dispatchers, coroutine_scope, with_context};
use cranpose_core::spawn_ui_task;
use cranpose_coroflow::main_dispatcher;
use support::{composition, pump_until};

#[test]
fn coroflow_finds_the_main_dispatcher_inside_cranpose_tasks_and_from_the_background() {
    let mut composition = composition();
    composition
        .render(1, || {
            assert!(main_dispatcher().is_some());
        })
        .expect("render");
    let main_thread = thread::current().id();
    let Some(main) = Dispatchers::main() else {
        panic!("rendering registered the main dispatcher");
    };

    let child_thread: Arc<Mutex<Option<ThreadId>>> = Arc::default();
    let record = Arc::clone(&child_thread);
    let spawned = spawn_ui_task(async move {
        let _ = coroutine_scope(|scope| async move {
            scope.launch(async move {
                *record.lock().unwrap_or_else(PoisonError::into_inner) =
                    Some(thread::current().id());
            });
        })
        .await;
    });
    assert!(spawned.is_some());
    let done = Arc::clone(&child_thread);
    assert!(pump_until(
        &mut composition,
        |_| {},
        move || {
            done.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_some()
        }
    ));
    assert_eq!(
        *child_thread.lock().unwrap_or_else(PoisonError::into_inner),
        Some(main_thread),
        "a child launched from a Cranpose task inherits Main"
    );

    let hopped: Rc<RefCell<Option<ThreadId>>> = Rc::default();
    let background = thread::spawn(move || {
        pollster::block_on(with_context(&main, async { thread::current().id() }))
    });
    let result = Rc::clone(&hopped);
    let mut background = Some(background);
    assert!(pump_until(
        &mut composition,
        |_| {},
        move || {
            if background
                .as_ref()
                .is_some_and(thread::JoinHandle::is_finished)
                && let Some(handle) = background.take()
                && let Ok(Ok(thread)) = handle.join()
            {
                *result.borrow_mut() = Some(thread);
            }
            result.borrow().is_some()
        }
    ));
    assert_eq!(
        *hopped.borrow(),
        Some(main_thread),
        "with_context(Main) ran on the UI thread"
    );
}
