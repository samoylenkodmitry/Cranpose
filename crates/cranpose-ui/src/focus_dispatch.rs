use std::{
    cell::{Cell, RefCell},
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
        if self.active_focus_target == node_id {
            return;
        }
        for changed in self.active_focus_target.into_iter().chain(node_id) {
            crate::semantics_dispatch::schedule_semantics_invalidation(changed);
        }
        self.active_focus_target = node_id;
        crate::request_render_invalidation();
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
                self.set_active_focus_target(None);
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
        let previous = self.active_focus_target;
        self.set_active_focus_target(Some(node_id));
        previous
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
    order: RefCell<Vec<crate::FocusEntry>>,
}

impl FocusInvalidationState {
    pub(crate) fn new() -> Self {
        Self {
            manager: RefCell::new(FocusInvalidationManager::new()),
            order: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn set_focus_order(&self, entries: Vec<crate::FocusEntry>) {
        *self.order.borrow_mut() = entries;
    }

    pub(crate) fn with_focus_order<T>(&self, reader: impl FnOnce(&[crate::FocusEntry]) -> T) -> T {
        reader(&self.order.borrow())
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

    pub(crate) fn has_focus_target(&self, node_id: NodeId) -> bool {
        self.manager.borrow().has_focus_target(node_id)
    }

    pub(crate) fn clear_active_focus(&self) -> bool {
        let previous = {
            let mut manager = self.manager.borrow_mut();
            let previous = manager.active_focus_target();
            manager.set_active_focus_target(None);
            previous
        };
        let Some(previous) = previous else {
            return false;
        };
        let handles = self.manager.borrow().focus_target_handles(previous);
        for handle in handles {
            handle.set_focus_state(FocusState::Inactive);
        }
        true
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
    crate::render_state::with_focus_dispatch(FocusInvalidationState::has_pending_invalidation)
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
    crate::render_state::with_focus_dispatch(FocusInvalidationState::active_focus_target)
}

pub(crate) fn register_focus_target(node_id: NodeId, handle: Rc<dyn FocusTargetHandle>) {
    crate::render_state::with_focus_dispatch(|state| state.register_focus_target(node_id, handle));
}

pub(crate) fn unregister_focus_target(node_id: NodeId, handle: &Rc<dyn FocusTargetHandle>) {
    crate::render_state::with_focus_dispatch(|state| {
        state.unregister_focus_target(node_id, handle);
    });
}

#[cfg(test)]
pub(crate) fn request_focus(node_id: NodeId) -> bool {
    crate::render_state::with_focus_dispatch(|state| state.request_focus(node_id))
}

/// Whether `node_id` registered a focus target in this app context.
pub(crate) fn has_focus_target(node_id: NodeId) -> bool {
    crate::render_state::with_focus_dispatch(|state| state.has_focus_target(node_id))
}

pub(crate) fn request_focus_in_context(node_id: NodeId) -> bool {
    let Some(app_context) = crate::render_state::current_app_context_id_opt() else {
        return false;
    };
    request_focus_for(app_context, node_id).unwrap_or(false)
}

/// Drops focus from the active target and answers whether one held it.
pub(crate) fn clear_active_focus() -> bool {
    crate::render_state::with_focus_dispatch(FocusInvalidationState::clear_active_focus)
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
    crate::render_state::with_focus_dispatch(FocusInvalidationState::clear);
}

#[cfg(test)]
#[path = "tests/focus_dispatch_tests.rs"]
mod tests;

thread_local! {
    static KEYBOARD_FOCUS_VISIBLE: Cell<bool> = const { Cell::new(false) };
}

/// Records how focus last moved: true after Tab or an arrow key, false after
/// a pointer press. A [`Modifier::focusable`](crate::Modifier::focusable)
/// draws its ring only while this is true. Returns whether the value changed,
/// so the caller can ask for a redraw.
pub fn set_keyboard_focus_visible(visible: bool) -> bool {
    KEYBOARD_FOCUS_VISIBLE.with(|cell| cell.replace(visible) != visible)
}

/// Whether the keyboard, and not a pointer, made the last focus move.
pub fn keyboard_focus_visible() -> bool {
    KEYBOARD_FOCUS_VISIBLE.with(Cell::get)
}
