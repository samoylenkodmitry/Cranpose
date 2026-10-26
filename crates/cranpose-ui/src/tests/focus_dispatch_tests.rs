use super::*;

#[test]
fn active_focus_target_tracking() {
    let _app_context = crate::render_state::app_context_test_scope();
    set_active_focus_target(None);
    assert_eq!(active_focus_target(), None);

    let node: NodeId = NodeId(42);
    set_active_focus_target(Some(node));
    assert_eq!(active_focus_target(), Some(node));

    set_active_focus_target(None);
    assert_eq!(active_focus_target(), None);
}

#[test]
fn active_focus_target_is_scoped_by_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);

    first.enter(|| set_active_focus_target(Some(NodeId(17))));
    second.enter(|| {
        assert_eq!(active_focus_target(), None);
        set_active_focus_target(Some(NodeId(19)));
    });

    first.enter(|| assert_eq!(active_focus_target(), Some(NodeId(17))));
    second.enter(|| assert_eq!(active_focus_target(), Some(NodeId(19))));
}

struct RecordingTarget {
    states: RefCell<Vec<FocusState>>,
    on_active: RefCell<Option<Box<dyn Fn()>>>,
}

impl RecordingTarget {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            states: RefCell::new(Vec::new()),
            on_active: RefCell::new(None),
        })
    }

    fn states(&self) -> Vec<FocusState> {
        self.states.borrow().clone()
    }
}

impl FocusTargetHandle for RecordingTarget {
    fn set_focus_state(&self, state: FocusState) {
        self.states.borrow_mut().push(state);
        if state == FocusState::Active
            && let Some(callback) = self.on_active.borrow_mut().take()
        {
            callback();
        }
    }
}

fn as_handle(target: &Rc<RecordingTarget>) -> Rc<dyn FocusTargetHandle> {
    Rc::clone(target) as Rc<dyn FocusTargetHandle>
}

#[test]
fn request_focus_moves_focus_between_two_registered_targets() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    let a = RecordingTarget::new();
    let b = RecordingTarget::new();
    register_focus_target(NodeId(1), as_handle(&a));
    register_focus_target(NodeId(2), as_handle(&b));

    assert!(request_focus(NodeId(1)));
    assert_eq!(a.states(), vec![FocusState::Active]);
    assert_eq!(active_focus_target(), Some(NodeId(1)));

    assert!(request_focus(NodeId(2)));
    assert_eq!(a.states(), vec![FocusState::Active, FocusState::Inactive]);
    assert_eq!(b.states(), vec![FocusState::Active]);
    assert_eq!(active_focus_target(), Some(NodeId(2)));
}

#[test]
fn request_focus_on_an_unregistered_node_fails_without_changing_anything() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    assert!(!request_focus(NodeId(99)));
    assert_eq!(active_focus_target(), None);
}

#[test]
fn unregistering_the_active_targets_last_handle_clears_active_focus() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    let a = RecordingTarget::new();
    let handle = as_handle(&a);
    register_focus_target(NodeId(1), Rc::clone(&handle));
    assert!(request_focus(NodeId(1)));
    assert_eq!(active_focus_target(), Some(NodeId(1)));

    unregister_focus_target(NodeId(1), &handle);
    assert_eq!(active_focus_target(), None);
    assert!(!request_focus(NodeId(1)));
}

#[test]
fn request_focus_from_inside_a_callback_does_not_double_borrow_or_recurse() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    let a = RecordingTarget::new();
    let b = RecordingTarget::new();
    register_focus_target(NodeId(1), as_handle(&a));
    register_focus_target(NodeId(2), as_handle(&b));

    *a.on_active.borrow_mut() = Some(Box::new({
        let bounced_back = RefCell::new(false);
        move || {
            if !*bounced_back.borrow() {
                *bounced_back.borrow_mut() = true;
                assert!(request_focus(NodeId(2)));
            }
        }
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(request_focus(NodeId(1)));
    }));
    assert!(
        result.is_ok(),
        "a request_focus call from inside a callback must not panic or overflow the stack"
    );

    assert_eq!(a.states(), vec![FocusState::Active, FocusState::Inactive]);
    assert_eq!(b.states(), vec![FocusState::Active]);
    assert_eq!(active_focus_target(), Some(NodeId(2)));
}

#[test]
fn a_panicking_callback_still_releases_the_dispatch_lock() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    struct PanicsOnActivate;
    impl FocusTargetHandle for PanicsOnActivate {
        fn set_focus_state(&self, state: FocusState) {
            assert!(
                state != FocusState::Active,
                "focus target panicked while activating"
            );
        }
    }

    register_focus_target(
        NodeId(1),
        Rc::new(PanicsOnActivate) as Rc<dyn FocusTargetHandle>,
    );
    let b = RecordingTarget::new();
    register_focus_target(NodeId(2), as_handle(&b));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        request_focus(NodeId(1));
    }));
    assert!(result.is_err());

    assert!(request_focus(NodeId(2)));
    assert_eq!(
        b.states(),
        vec![FocusState::Active],
        "the dispatch lock must be released after a callback panic so later requests proceed"
    );
}
