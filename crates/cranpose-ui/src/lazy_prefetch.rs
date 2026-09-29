use std::{
    cell::{Cell, RefCell},
    time::Duration,
};

use cranpose_core::NodeId;

use crate::render_state::{current_app_context, require_current_app_context};

#[derive(Default)]
pub(crate) struct LazyPrefetchState {
    requests: RefCell<Vec<NodeId>>,
    idle_pass: Cell<bool>,
    item_cost_nanos: Cell<u64>,
    /// Root nodes of the items idle prefetch passes composed, whose modifier
    /// slices [`warm_prefetched_slices`] builds.
    prefetched: RefCell<Vec<NodeId>>,
}

impl LazyPrefetchState {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

fn with_state<R>(f: impl FnOnce(&LazyPrefetchState) -> R) -> R {
    let context = require_current_app_context("lazy prefetch access");
    f(context.lazy_prefetch())
}

pub(crate) fn request_lazy_prefetch(node_id: NodeId) {
    with_state(|state| {
        let mut requests = state.requests.borrow_mut();
        if !requests.contains(&node_id) {
            requests.push(node_id);
        }
    });
}

pub(crate) fn in_lazy_prefetch_pass() -> bool {
    current_app_context().is_some_and(|context| context.lazy_prefetch().idle_pass.get())
}

pub(crate) fn record_lazy_item_cost(cost: Duration) {
    let sample = u64::try_from(cost.as_nanos()).unwrap_or(u64::MAX);
    with_state(|state| {
        let previous = state.item_cost_nanos.get();
        let blended = if previous == 0 {
            sample
        } else {
            previous.saturating_mul(3).saturating_add(sample) / 4
        };
        state.item_cost_nanos.set(blended);
    });
}

/// Notes the root nodes of an item an idle prefetch pass composed.
pub(crate) fn note_prefetched_item(nodes: &[u64]) {
    with_state(|state| {
        state
            .prefetched
            .borrow_mut()
            .extend(nodes.iter().filter_map(|&node| NodeId::try_from(node).ok()));
    });
}

/// Builds the modifier slices of the items idle prefetch passes composed
/// since the last call, and of every node under them. A node's slices are
/// built the first time a frame draws it, which for a list's rows put that
/// work in the frame each row entered the screen. Call it after an idle
/// prefetch pass, while waiting for the next frame.
pub fn warm_prefetched_slices(applier: &mut cranpose_core::MemoryApplier) {
    let mut stack = with_state(|state| std::mem::take(&mut *state.prefetched.borrow_mut()));
    while let Some(id) = stack.pop() {
        let layout = applier.with_node::<crate::widgets::nodes::LayoutNode, _>(id, |node| {
            let _ = node.modifier_slices_snapshot();
            stack.extend_from_slice(&node.children);
        });
        if layout.is_err() {
            let _ = applier.with_node::<crate::subcompose_layout::SubcomposeLayoutNode, _>(
                id,
                |node| {
                    let _ = node.modifier_slices_snapshot();
                    node.with_active_children(|children| stack.extend_from_slice(children));
                },
            );
        }
    }
    with_state(|state| {
        let mut prefetched = state.prefetched.borrow_mut();
        if prefetched.is_empty() {
            *prefetched = stack;
        }
    });
}

/// Hands `schedule` each lazy list that left an item beyond its viewport
/// uncomposed during a frame, for [`with_lazy_prefetch_pass`] to compose
/// before the next frame, and clears the requests.
pub fn drain_lazy_prefetch_requests(schedule: impl FnMut(NodeId)) {
    with_state(|state| state.requests.borrow_mut().drain(..).for_each(schedule));
}

/// Whether any lazy list waits for an idle prefetch pass.
pub fn has_lazy_prefetch_requests() -> bool {
    with_state(|state| !state.requests.borrow().is_empty())
}

/// The recent cost of composing and measuring one lazy list item: the time
/// an idle prefetch pass needs before the next frame starts.
pub fn lazy_prefetch_item_cost() -> Duration {
    with_state(|state| Duration::from_nanos(state.item_cost_nanos.get()))
}

/// Runs `pass`, a layout pass, as an idle prefetch pass: each lazy list it
/// measures composes the next item beyond its viewport that a frame left
/// uncomposed.
pub fn with_lazy_prefetch_pass<R>(pass: impl FnOnce() -> R) -> R {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            with_state(|state| state.idle_pass.set(false));
        }
    }
    with_state(|state| state.idle_pass.set(true));
    let _reset = Reset;
    pass()
}

#[cfg(test)]
#[path = "tests/lazy_prefetch_tests.rs"]
mod tests;
