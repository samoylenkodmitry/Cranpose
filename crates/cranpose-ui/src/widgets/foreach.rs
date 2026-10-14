//! ForEach iteration helper

use std::hash::Hash;

use crate::composable;

/// Composes `row` once for each item of `items`, keyed by the item so that remembered state follows
/// an item when the list changes order.
#[composable(no_skip)]
pub fn ForEach<T, F>(items: &[T], mut row: F)
where
    T: Hash,
    F: FnMut(&T) + 'static,
{
    for item in items {
        cranpose_core::with_key(item, || row(item));
    }
}
