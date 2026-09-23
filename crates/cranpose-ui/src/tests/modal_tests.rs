use super::*;
use crate::render_state::{AppContext, app_context_test_scope};

#[test]
fn the_innermost_modal_takes_the_back_request() {
    let _scope = app_context_test_scope();
    clear_modals();
    let outer = Rc::new(Cell::new(0u32));
    let inner = Rc::new(Cell::new(0u32));
    let outer_counter = Rc::clone(&outer);
    let inner_counter = Rc::clone(&inner);

    let _outer = register_modal(Rc::new(move || outer_counter.set(outer_counter.get() + 1)));
    let inner_registration =
        register_modal(Rc::new(move || inner_counter.set(inner_counter.get() + 1)));

    assert!(dispatch_modal_back());
    assert_eq!(inner.get(), 1);
    assert_eq!(outer.get(), 0);

    drop(inner_registration);
    assert!(dispatch_modal_back());
    assert_eq!(outer.get(), 1);
    clear_modals();
}

#[test]
fn a_back_request_with_no_modal_open_is_not_taken() {
    let _scope = app_context_test_scope();
    clear_modals();
    assert!(!dispatch_modal_back());
}

#[test]
fn the_depth_counts_what_is_open_and_falls_back_to_zero() {
    let _scope = app_context_test_scope();
    clear_modals();
    assert_eq!(modal_depth(), 0);

    let outer = register_modal(Rc::new(|| {}));
    assert_eq!(modal_depth(), 1);
    let inner = register_modal(Rc::new(|| {}));
    assert_eq!(modal_depth(), 2);

    drop(inner);
    assert_eq!(modal_depth(), 1);
    drop(outer);
    assert_eq!(modal_depth(), 0);
}

#[test]
fn registrations_leave_the_stack_when_dropped() {
    let _scope = app_context_test_scope();
    clear_modals();
    {
        let _first = register_modal(Rc::new(|| {}));
        let _second = register_modal(Rc::new(|| {}));
        assert_eq!(current_modal_depth(), 2);
    }
    assert_eq!(current_modal_depth(), 0);
    assert!(!dispatch_modal_back());
}

#[test]
fn modal_registrations_belong_to_their_app_context() {
    let first = AppContext::new();
    let second = AppContext::new();
    let first_count = Rc::new(Cell::new(0usize));
    let second_count = Rc::new(Cell::new(0usize));
    let first_callback = Rc::clone(&first_count);
    let second_callback = Rc::clone(&second_count);
    let first_registration = first.enter(|| {
        register_modal(Rc::new(move || {
            first_callback.set(first_callback.get() + 1);
        }))
    });
    let second_registration = second.enter(|| {
        assert_eq!(modal_depth(), 0);
        register_modal(Rc::new(move || {
            second_callback.set(second_callback.get() + 1);
        }))
    });
    first.enter(|| {
        assert_eq!(modal_depth(), 1);
        assert!(dispatch_modal_back());
    });
    assert_eq!(first_count.get(), 1);
    assert_eq!(second_count.get(), 0);
    second.enter(|| {
        drop(first_registration);
        assert_eq!(modal_depth(), 1);
        assert!(dispatch_modal_back());
    });
    first.enter(|| assert_eq!(modal_depth(), 0));
    assert_eq!(second_count.get(), 1);
    drop(second_registration);
    second.enter(|| assert_eq!(modal_depth(), 0));
}

#[test]
fn a_registration_can_outlive_its_app_context() {
    let context = AppContext::new();
    let registration = context.enter(|| register_modal(Rc::new(|| {})));
    drop(context);
    drop(registration);
}

#[test]
fn clearing_modal_registrations_preserves_other_apps() {
    let first = AppContext::new();
    let second = AppContext::new();
    let first_registration = first.enter(|| register_modal(Rc::new(|| {})));
    let second_registration = second.enter(|| register_modal(Rc::new(|| {})));
    first.enter(clear_modals);
    first.enter(|| assert_eq!(modal_depth(), 0));
    second.enter(|| assert_eq!(modal_depth(), 1));
    drop(first_registration);
    drop(second_registration);
}

#[test]
fn modal_depth_local_has_one_identity_on_the_thread() {
    assert!(local_modal_depth() == local_modal_depth());
    assert_eq!(local_modal_depth().default_value(), 0);
}
