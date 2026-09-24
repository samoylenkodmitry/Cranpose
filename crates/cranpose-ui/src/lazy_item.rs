//! The identity of the lazy-list item currently being composed.
//!
//! A lazy list reuses composition slots: when a row is removed, every row below
//! it shifts up into the slot above, taking that slot's remembered state with
//! it. Anything a row remembers that belongs to the *item* rather than to the
//! *slot* — a swipe displacement, a revealed background, an expanded flag —
//! then lands on the wrong row, which is what "the items go shuffled and the
//! red boxes stick" looks like on screen.
//!
//! A row cannot fix that on its own, because the identity lives in the list's
//! `key`. This composition local carries that key down to whatever inside the
//! row needs it, so a control keys itself and the application does not repeat
//! the row's id in every widget that holds per-item state.
//!
//! Only **user** keys are carried. A list that supplies no key is keyed by
//! index, and an index is the identity of the position rather than of the item
//! — exactly the identity that causes the leak — so an unkeyed list reports
//! `None` and per-slot behaviour is unchanged.

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};

/// The [`CompositionLocal`] carrying the current lazy item's key.
pub fn local_lazy_item_key() -> CompositionLocal<Option<u64>> {
    thread_local! {
        static LOCAL: std::cell::RefCell<Option<CompositionLocal<Option<u64>>>> =
            const { std::cell::RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

/// The key of the lazy item being composed, or `None` outside a keyed lazy item.
pub fn lazy_item_key() -> Option<u64> {
    local_lazy_item_key().current()
}

/// Composes `content` as the item identified by `key`.
///
/// Lazy lists call this around each item's content; an application composing
/// its own reusable slots can call it too.
#[expect(non_snake_case)]
#[track_caller]
pub fn ProvideLazyItemKey(key: Option<u64>, content: impl FnOnce()) {
    CompositionLocalProvider(vec![local_lazy_item_key().provides(key)], content);
}

#[cfg(test)]
#[path = "tests/lazy_item_tests.rs"]
mod tests;
