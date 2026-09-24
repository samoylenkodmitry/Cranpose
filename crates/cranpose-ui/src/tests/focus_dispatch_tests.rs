use super::*;

#[test]
fn schedule_and_process_invalidations() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();

    let node1: NodeId = 1;
    let node2: NodeId = 2;

    schedule_focus_invalidation(node1);
    schedule_focus_invalidation(node2);

    assert!(has_pending_focus_invalidations());

    let mut processed = Vec::new();
    process_focus_invalidations(|node_id| {
        processed.push(node_id);
    });

    assert_eq!(processed.len(), 2);
    assert!(processed.contains(&node1));
    assert!(processed.contains(&node2));
    assert!(!has_pending_focus_invalidations());
}

#[test]
fn active_focus_target_tracking() {
    let _app_context = crate::render_state::app_context_test_scope();
    set_active_focus_target(None);
    assert_eq!(active_focus_target(), None);

    let node: NodeId = 42;
    set_active_focus_target(Some(node));
    assert_eq!(active_focus_target(), Some(node));

    set_active_focus_target(None);
    assert_eq!(active_focus_target(), None);
}

#[test]
fn duplicate_invalidations_deduplicated() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();

    let node: NodeId = 42;
    schedule_focus_invalidation(node);
    schedule_focus_invalidation(node);
    schedule_focus_invalidation(node);

    let mut count = 0;
    process_focus_invalidations(|_| {
        count += 1;
    });

    assert_eq!(count, 1);
}

#[test]
fn process_invalidations_recovers_after_processor_panic() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();

    schedule_focus_invalidation(1);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_focus_invalidations(|_| panic!("focus processor panic"));
    }));
    assert!(result.is_err());

    schedule_focus_invalidation(2);
    let mut processed = Vec::new();
    process_focus_invalidations(|node_id| processed.push(node_id));

    assert!(
        processed.contains(&2),
        "focus invalidation processing must not stay stuck after a processor panic"
    );
    assert!(!has_pending_focus_invalidations());
}

#[test]
fn process_invalidations_allows_processor_to_schedule_more_work() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();

    schedule_focus_invalidation(1);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        process_focus_invalidations(|_| schedule_focus_invalidation(2));
    }));
    assert!(
        result.is_ok(),
        "focus processors must be able to enqueue follow-up invalidations"
    );
    assert!(has_pending_focus_invalidations());

    let mut processed = Vec::new();
    process_focus_invalidations(|node_id| processed.push(node_id));

    assert_eq!(processed, vec![2]);
    assert!(!has_pending_focus_invalidations());
}

#[test]
fn focus_state_is_scoped_by_app_context() {
    let _app_context = crate::render_state::app_context_test_scope();
    let first = crate::render_state::AppContext::new_with_density(1.0);
    let second = crate::render_state::AppContext::new_with_density(1.0);

    first.enter(|| {
        clear_focus_invalidations();
        schedule_focus_invalidation(7);
        set_active_focus_target(Some(17));
        assert!(has_pending_focus_invalidations());
        assert_eq!(active_focus_target(), Some(17));
    });

    second.enter(|| {
        clear_focus_invalidations();
        assert!(!has_pending_focus_invalidations());
        assert_eq!(active_focus_target(), None);
        schedule_focus_invalidation(9);
        set_active_focus_target(Some(19));
    });

    first.enter(|| {
        let mut processed = Vec::new();
        process_focus_invalidations(|node_id| processed.push(node_id));
        assert_eq!(processed, vec![7]);
        assert_eq!(active_focus_target(), Some(17));
    });

    second.enter(|| {
        let mut processed = Vec::new();
        process_focus_invalidations(|node_id| processed.push(node_id));
        assert_eq!(processed, vec![9]);
        assert_eq!(active_focus_target(), Some(19));
    });
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
    register_focus_target(1, as_handle(&a));
    register_focus_target(2, as_handle(&b));

    assert!(request_focus(1));
    assert_eq!(a.states(), vec![FocusState::Active]);
    assert_eq!(active_focus_target(), Some(1));

    assert!(request_focus(2));
    assert_eq!(a.states(), vec![FocusState::Active, FocusState::Inactive]);
    assert_eq!(b.states(), vec![FocusState::Active]);
    assert_eq!(active_focus_target(), Some(2));
}

#[test]
fn request_focus_on_an_unregistered_node_fails_without_changing_anything() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    assert!(!request_focus(99));
    assert_eq!(active_focus_target(), None);
}

#[test]
fn unregistering_the_active_targets_last_handle_clears_active_focus() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    let a = RecordingTarget::new();
    let handle = as_handle(&a);
    register_focus_target(1, Rc::clone(&handle));
    assert!(request_focus(1));
    assert_eq!(active_focus_target(), Some(1));

    unregister_focus_target(1, &handle);
    assert_eq!(active_focus_target(), None);
    assert!(!request_focus(1));
}

#[test]
fn request_focus_from_inside_a_callback_does_not_double_borrow_or_recurse() {
    let _app_context = crate::render_state::app_context_test_scope();
    clear_focus_invalidations();
    set_active_focus_target(None);

    let a = RecordingTarget::new();
    let b = RecordingTarget::new();
    register_focus_target(1, as_handle(&a));
    register_focus_target(2, as_handle(&b));

    *a.on_active.borrow_mut() = Some(Box::new({
        let bounced_back = RefCell::new(false);
        move || {
            if !*bounced_back.borrow() {
                *bounced_back.borrow_mut() = true;
                assert!(request_focus(2));
            }
        }
    }));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(request_focus(1));
    }));
    assert!(
        result.is_ok(),
        "a request_focus call from inside a callback must not panic or overflow the stack"
    );

    assert_eq!(a.states(), vec![FocusState::Active, FocusState::Inactive]);
    assert_eq!(b.states(), vec![FocusState::Active]);
    assert_eq!(active_focus_target(), Some(2));
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

    register_focus_target(1, Rc::new(PanicsOnActivate) as Rc<dyn FocusTargetHandle>);
    let b = RecordingTarget::new();
    register_focus_target(2, as_handle(&b));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        request_focus(1);
    }));
    assert!(result.is_err());

    assert!(request_focus(2));
    assert_eq!(
        b.states(),
        vec![FocusState::Active],
        "the dispatch lock must be released after a callback panic so later requests proceed"
    );
}
