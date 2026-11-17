//! The measure policy a layout node holds, shared between nodes where its
//! value allows.

use std::{any::Any, cell::RefCell, rc::Rc};

use cranpose_ui_layout::MeasurePolicy;

use crate::{
    layout::policies::{BoxMeasurePolicy, EmptyMeasurePolicy, FlexMeasurePolicy},
    text::recent_values::RecentValues,
};

// How many recent policies of one kind [`held_policy`] keeps to share.
const SHARED_POLICIES: usize = 8;

// The policies of rows, columns and boxes recently made on a thread, which
// later nodes with an equal one share.
pub(crate) struct RecentPolicies {
    flex: RefCell<RecentValues<Rc<FlexMeasurePolicy>, SHARED_POLICIES>>,
    boxes: RefCell<RecentValues<Rc<BoxMeasurePolicy>, SHARED_POLICIES>>,
}

impl RecentPolicies {
    pub(crate) const fn new() -> Self {
        Self {
            flex: RefCell::new(RecentValues::new()),
            boxes: RefCell::new(RecentValues::new()),
        }
    }
}

// `policy` as a node holds it. Every leaf shares the thread's one empty
// policy. A row's, a column's or a box's policy, plain values all, is
// shared with the nodes recently made with an equal one: a screen of rows
// and boxes in a few arrangements keeps a few policies, not one a node. Any
// other policy, which may hold what its node's caller gave it, gets an
// allocation of its own and goes with its nodes.
pub(crate) fn held_policy<P: MeasurePolicy + Clone>(policy: &P) -> Rc<dyn MeasurePolicy> {
    let any: &dyn Any = policy;
    if any.is::<EmptyMeasurePolicy>() {
        return crate::widgets::nodes::layout_node::empty_measure_policy();
    }
    if let Some(flex) = any.downcast_ref::<FlexMeasurePolicy>() {
        return crate::thread_state::with(|state| shared(&state.recent_policies.flex, flex));
    }
    if let Some(boxed) = any.downcast_ref::<BoxMeasurePolicy>() {
        return crate::thread_state::with(|state| shared(&state.recent_policies.boxes, boxed));
    }
    Rc::new(policy.clone())
}

// The kept policy equal to `policy`, or a new one kept first.
fn shared<T: MeasurePolicy + Clone + PartialEq>(
    recent: &RefCell<RecentValues<Rc<T>, SHARED_POLICIES>>,
    policy: &T,
) -> Rc<dyn MeasurePolicy> {
    let mut recent = recent.borrow_mut();
    if let Some(kept) = recent.find(|kept| **kept == *policy) {
        return Rc::clone(kept) as Rc<dyn MeasurePolicy>;
    }
    Rc::clone(recent.keep(Rc::new(policy.clone()))) as Rc<dyn MeasurePolicy>
}
