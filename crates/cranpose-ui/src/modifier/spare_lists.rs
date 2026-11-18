// Emptied list blocks of modifiers, kept for the next lists the thread
// makes. A frame of the feed makes and lets go of about 96 list blocks, an
// allocation and a free each.

use std::{cell::RefCell, rc::Rc};

use super::elements::ManyElements;
use crate::thread_state;

pub(crate) struct SpareLists(RefCell<Vec<Rc<ManyElements>>>);

impl SpareLists {
    // At most this many blocks.
    const BLOCKS: usize = 8;

    pub(crate) const fn new() -> Self {
        Self(RefCell::new(Vec::new()))
    }
}

// An empty block from the thread's spares, `None` when it has none.
pub(super) fn take() -> Option<Rc<ManyElements>> {
    thread_state::try_with(|state| state.spare_lists.0.try_borrow_mut().ok()?.pop()).flatten()
}

// Empties the block of the last holder of `list` and keeps it among the
// thread's spares while they have room. Out of line and not generic: every
// drop of a list's last holder comes here.
#[inline(never)]
pub(super) fn give_back(list: &mut Rc<ManyElements>) {
    let Some(owned) = Rc::get_mut(list) else {
        return;
    };
    owned.clear();
    thread_state::try_with(|state| {
        if let Ok(mut spares) = state.spare_lists.0.try_borrow_mut()
            && spares.len() < SpareLists::BLOCKS
        {
            spares.push(Rc::clone(list));
        }
    });
}
