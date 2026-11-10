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

    // The held policy `equal` finds, or the one `make` makes, which takes the
    // place of the one held longest.
    pub(crate) fn shared(
        &mut self,
        equal: &dyn Fn(&dyn Any) -> bool,
        make: &dyn Fn() -> Rc<dyn MeasurePolicy>,
    ) -> Rc<dyn MeasurePolicy> {
        let held = self.recent.iter().flatten().find(|held| equal(&***held));
        if let Some(held) = held {
            return Rc::clone(held);
        }
        let made = make();
        self.recent[self.next] = Some(Rc::clone(&made));
        self.next = (self.next + 1) % SHARED_POLICIES;
        made
    }
}
