use std::{any::Any, rc::Rc};

use cranpose_ui_layout::MeasurePolicy;

const SHARED_POLICIES: usize = 8;

// The immutable policies layout nodes took last: a node given a policy
// equal to one of them takes that one, so equal Rows, Columns and Boxes
// share one allocation.
pub(crate) struct SharedPolicies {
    recent: [Option<Rc<dyn MeasurePolicy>>; SHARED_POLICIES],
    next: usize,
}

impl SharedPolicies {
    pub(crate) const fn new() -> Self {
        Self {
            recent: [const { None }; SHARED_POLICIES],
            next: 0,
        }
    }

    // The held policy equal to `policy`, or a new one, which takes the place
    // of the one held longest.
    pub(crate) fn shared<P: MeasurePolicy + Clone + PartialEq>(
        &mut self,
        policy: &P,
    ) -> Rc<dyn MeasurePolicy> {
        let held = self.recent.iter().flatten().find(|held| {
            let held: &dyn Any = &***held;
            held.downcast_ref::<P>() == Some(policy)
        });
        if let Some(held) = held {
            return Rc::clone(held);
        }
        let made: Rc<dyn MeasurePolicy> = Rc::new(policy.clone());
        self.recent[self.next] = Some(Rc::clone(&made));
        self.next = (self.next + 1) % SHARED_POLICIES;
        made
    }
}
