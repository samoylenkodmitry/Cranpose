// Lists of items a lowering fills before it writes them to their record,
// kept between updates so a lowering allocates no list of its own.

use crate::scene_store::Item;

pub(super) fn take() -> Vec<Item> {
    crate::thread_state::with(|state| state.item_lists.borrow_mut().pop()).unwrap_or_default()
}

pub(super) fn give(mut list: Vec<Item>) {
    list.clear();
    if list.capacity() > 0 {
        crate::thread_state::with(|state| state.item_lists.borrow_mut().push(list));
    }
}
