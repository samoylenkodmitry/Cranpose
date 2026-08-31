use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::FocusState;

pub(crate) trait FocusTargetHandle {
    fn set_focus_state(&self, state: FocusState);
}

struct FocusInvalidationManager {
    dirty_nodes: HashSet<NodeId>,
    is_processing: bool,
    active_focus_target: Option<NodeId>,
    focus_targets: HashMap<NodeId, Vec<Rc<dyn FocusTargetHandle>>>,
    pending_focus_requests: VecDeque<NodeId>,
    dispatching_focus: bool,
}

impl FocusInvalidationManager {
    fn new() -> Self {
        Self {
            dirty_nodes: HashSet::new(),
            is_processing: false,
            active_focus_target: None,
            focus_targets: HashMap::new(),
            pending_focus_requests: VecDeque::new(),
            dispatching_focus: false,
        }
    }

    fn schedule_invalidation(&mut self, node_id: NodeId) {
        self.dirty_nodes.insert(node_id);
    }

    fn has_pending_invalidation(&self) -> bool {
        !self.dirty_nodes.is_empty()
    }

    fn set_active_focus_target(&mut self, node_id: Option<NodeId>) {
        self.active_focus_target = node_id;
    }

    fn active_focus_target(&self) -> Option<NodeId> {
        self.active_focus_target
    }

    fn register_focus_target(&mut self, node_id: NodeId, handle: Rc<dyn FocusTargetHandle>) {
        self.focus_targets.entry(node_id).or_default().push(handle);
    }

    fn unregister_focus_target(&mut self, node_id: NodeId, handle: &Rc<dyn FocusTargetHandle>) {
        let Some(handles) = self.focus_targets.get_mut(&node_id) else {
            return;
        };
        handles.retain(|existing| !Rc::ptr_eq(existing, handle));
        if handles.is_empty() {
            self.focus_targets.remove(&node_id);
            if self.active_focus_target == Some(node_id) {
                self.active_focus_target = None;
            }
        }
    }

    fn has_focus_target(&self, node_id: NodeId) -> bool {
        self.focus_targets.contains_key(&node_id)
    }

    fn focus_target_handles(&self, node_id: NodeId) -> Vec<Rc<dyn FocusTargetHandle>> {
        self.focus_targets
            .get(&node_id)
            .cloned()
            .unwrap_or_default()
    }

    fn swap_active_focus_target(&mut self, node_id: NodeId) -> Option<NodeId> {
        if self.active_focus_target == Some(node_id) {
            return None;
        }
        self.active_focus_target.replace(node_id)
    }

    fn take_first_focus_request_if_idle(&mut self) -> Option<NodeId> {
        if self.dispatching_focus {
            return None;
        }
        let next = self.pending_focus_requests.pop_front()?;
        self.dispatching_focus = true;
        Some(next)
    }

    fn take_next_focus_request(&mut self) -> Option<NodeId> {
        self.pending_focus_requests.pop_front()
    }

    fn finish_focus_dispatch(&mut self) {
        self.dispatching_focus = false;
    }

    fn take_pending_for_processing(&mut self) -> Option<Vec<NodeId>> {
        if self.is_processing {
            return None;
        }

        self.is_processing = true;
        Some(self.dirty_nodes.drain().collect())
    }

    fn finish_processing<I>(&mut self, remaining: I)
    where
        I: IntoIterator<Item = NodeId>,
    {
        self.dirty_nodes.extend(remaining);
        self.is_processing = false;
    }

    fn clear(&mut self) {
        self.dirty_nodes.clear();
    }
}

pub(crate) struct FocusInvalidationState {
    manager: RefCell<FocusInvalidationManager>,
}

impl FocusInvalidationState {
    pub(crate) fn new() -> Self {
        Self {
            manager: RefCell::new(FocusInvalidationManager::new()),
        }
    }

    fn schedule_invalidation(&self, node_id: NodeId) {
        self.manager.borrow_mut().schedule_invalidation(node_id);
    }

    fn has_pending_invalidation(&self) -> bool {
        self.manager.borrow().has_pending_invalidation()
    }

    fn set_active_focus_target(&self, node_id: Option<NodeId>) {
        self.manager.borrow_mut().set_active_focus_target(node_id);
    }

    fn active_focus_target(&self) -> Option<NodeId> {
        self.manager.borrow().active_focus_target()
    }

    fn register_focus_target(&self, node_id: NodeId, handle: Rc<dyn FocusTargetHandle>) {
        self.manager
            .borrow_mut()
            .register_focus_target(node_id, handle);
    }

    fn unregister_focus_target(&self, node_id: NodeId, handle: &Rc<dyn FocusTargetHandle>) {
        self.manager
            .borrow_mut()
            .unregister_focus_target(node_id, handle);
    }

    pub(crate) fn request_focus(&self, node_id: NodeId) -> bool {
        let accepted = {
            let mut manager = self.manager.borrow_mut();
            if !manager.has_focus_target(node_id) {
                false
            } else {
                manager.pending_focus_requests.push_back(node_id);
                true
            }
        };
        if accepted {
            self.drain_focus_requests();
        }
        accepted
    }

    fn drain_focus_requests(&self) {
        let Some(first) = self.manager.borrow_mut().take_first_focus_request_if_idle() else {
            return;
        };

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut current = first;
            loop {
                self.apply_focus_change(current);
                match self.manager.borrow_mut().take_next_focus_request() {
                    Some(next) => current = next,
                    None => break,
                }
            }
        }));

        self.manager.borrow_mut().finish_focus_dispatch();

        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }

    fn apply_focus_change(&self, node_id: NodeId) {
        let previous = self.manager.borrow_mut().swap_active_focus_target(node_id);

        if let Some(previous) = previous {
            let losing_handles = self.manager.borrow().focus_target_handles(previous);
            for handle in losing_handles {
                handle.set_focus_state(FocusState::Inactive);
            }
        }

        let gaining_handles = self.manager.borrow().focus_target_handles(node_id);
        for handle in gaining_handles {
            handle.set_focus_state(FocusState::Active);
        }
    }

    fn process_invalidations<F>(&self, processor: F)
    where
        F: FnMut(NodeId),
    {
        let Some(nodes) = self.manager.borrow_mut().take_pending_for_processing() else {
            return;
        };

        self.process_pending_nodes(nodes, processor);
    }

    fn clear(&self) {
        self.manager.borrow_mut().clear();
    }

    fn process_pending_nodes<F>(&self, nodes: Vec<NodeId>, mut processor: F)
    where
        F: FnMut(NodeId),
    {
        let mut remaining = nodes.into_iter();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for node_id in remaining.by_ref() {
                processor(node_id);
            }
        }));

        self.manager.borrow_mut().finish_processing(remaining);

        if let Err(payload) = result {
            std::panic::resume_unwind(payload);
        }
    }
}

/// Schedules a focus invalidation for the specified node.
///
/// This is called automatically when focus modifiers invalidate
/// and mirrors Kotlin's `FocusInvalidationManager.scheduleInvalidation`.
pub fn schedule_focus_invalidation(node_id: NodeId) {
    crate::render_state::with_focus_dispatch(|state| state.schedule_invalidation(node_id));
}

/// Returns true if any focus invalidations are pending.
pub fn has_pending_focus_invalidations() -> bool {
    crate::render_state::with_focus_dispatch(|state| state.has_pending_invalidation())
}

/// Sets the currently active focus target.
///
/// This mirrors Kotlin's `FocusOwner.activeFocusTargetNode` and allows
/// the focus system to track which node currently has focus.
pub fn set_active_focus_target(node_id: Option<NodeId>) {
    crate::render_state::with_focus_dispatch(|state| state.set_active_focus_target(node_id));
}

/// Returns the currently active focus target, if any.
pub fn active_focus_target() -> Option<NodeId> {
    crate::render_state::with_focus_dispatch(|state| state.active_focus_target())
}

pub(crate) fn register_focus_target(node_id: NodeId, handle: Rc<dyn FocusTargetHandle>) {
    crate::render_state::with_focus_dispatch(|state| state.register_focus_target(node_id, handle));
}

pub(crate) fn unregister_focus_target(node_id: NodeId, handle: &Rc<dyn FocusTargetHandle>) {
    crate::render_state::with_focus_dispatch(|state| {
        state.unregister_focus_target(node_id, handle)
    });
}

#[cfg(test)]
pub(crate) fn request_focus(node_id: NodeId) -> bool {
    crate::render_state::with_focus_dispatch(|state| state.request_focus(node_id))
}

pub(crate) fn request_focus_for(
    app_context: crate::render_state::AppContextId,
    node_id: NodeId,
) -> Option<bool> {
    crate::render_state::with_focus_dispatch_by_app_context(app_context, |state| {
        state.request_focus(node_id)
    })
}

/// Processes all pending focus invalidations.
///
/// The host (e.g., app shell or layout engine) should call this after
/// composition/layout to service focus invalidations without forcing
/// measure/layout passes.
pub fn process_focus_invalidations<F>(processor: F)
where
    F: FnMut(NodeId),
{
    crate::render_state::with_focus_dispatch(|state| state.process_invalidations(processor));
}

/// Clears all pending focus invalidations without processing them.
pub fn clear_focus_invalidations() {
    crate::render_state::with_focus_dispatch(|state| state.clear());
}

#[cfg(test)]
mod tests {
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
                if state == FocusState::Active {
                    panic!("focus target panicked while activating");
                }
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
}
