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

use cranpose_core::{compositionLocalOf, CompositionLocal, CompositionLocalProvider};

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
#[allow(non_snake_case)]
pub fn ProvideLazyItemKey(key: Option<u64>, content: impl FnOnce()) {
    CompositionLocalProvider(vec![local_lazy_item_key().provides(key)], content);
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use cranpose_core::{location_key, Composition, MemoryApplier};

    use super::*;

    /// Runs `body` once inside a real composition and returns what it recorded.
    fn composed<T: Copy + Default + 'static>(body: impl Fn(&Cell<T>) + 'static) -> T {
        let mut composition = Composition::new(MemoryApplier::new());
        let seen = Rc::new(Cell::new(T::default()));
        let recorder = Rc::clone(&seen);
        let mut render = move || body(&recorder);
        composition
            .render(location_key(file!(), line!(), column!()), &mut render)
            .expect("render");
        seen.get()
    }

    #[test]
    fn an_item_key_reaches_what_the_row_composes() {
        // Outside a lazy item there is no identity to carry, inside there is,
        // and it ends with the item so the next row does not inherit it.
        let seen: (Option<u64>, Option<u64>, Option<u64>) = composed(|cell| {
            let before = lazy_item_key();
            let inside = Cell::new(None);
            ProvideLazyItemKey(Some(17), || inside.set(lazy_item_key()));
            cell.set((before, inside.get(), lazy_item_key()));
        });
        assert_eq!(seen, (None, Some(17), None));
    }

    #[test]
    fn an_unkeyed_list_reports_no_identity_rather_than_its_position() {
        // An index is the identity of the slot, which is the identity that
        // leaks state between rows; a list with no user key says so.
        let seen: Option<u64> = composed(|cell| {
            cell.set(Some(0));
            ProvideLazyItemKey(None, || cell.set(lazy_item_key()));
        });
        assert_eq!(seen, None);
    }

    #[test]
    fn the_composition_local_is_one_instance_per_thread() {
        // Two calls that returned different locals would each carry their own
        // value, and a provision through one would be invisible to the other.
        let seen: bool = composed(|cell| {
            ProvideLazyItemKey(Some(3), || {
                cell.set(local_lazy_item_key().current() == Some(3));
            });
        });
        assert!(seen);
    }
}
