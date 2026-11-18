// The thread's table of the modifier chains of constants that
// `#[composable]` bodies keep.

use std::cell::RefCell;

use cranpose_core::Key;

use super::Modifier;
use crate::thread_state;

// Slots of the table, as a power of two.
const SLOTS: usize = 128;

// The kept chains of the thread, each with the key of the expression that
// made it; empty until the first keep.
pub(crate) type KeptModifiers = RefCell<Vec<Option<(Key, Modifier)>>>;

/// The modifier that `make` gives, for a modifier chain of constants that
/// `#[composable]` found to give the same modifier every time it runs: `key`
/// names the chain's expression. The thread keeps the modifier it made last
/// for `key` in a table of 128 slots, so the chain is made again only when
/// another chain took its slot. It needs no composition: the chain may sit
/// in a closure that runs in an event handler, an effect or another thread.
#[doc(hidden)]
pub fn __kept_modifier(key: Key, make: impl FnOnce() -> Modifier) -> Modifier {
    if let Some(modifier) = kept_modifier(key) {
        return modifier;
    }
    let modifier = make();
    keep_modifier(key, &modifier);
    modifier
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the slot is the low bits of the key"
)]
fn slot(key: Key) -> usize {
    (key.0 as usize) & (SLOTS - 1)
}

// A copy of the modifier kept for `key`. Out of line and not generic:
// `__kept_modifier` has an instance for each call site, as each passes its
// own closure, so the lookup and the copy would otherwise repeat in each.
#[inline(never)]
fn kept_modifier(key: Key) -> Option<Modifier> {
    let mut copy = None;
    thread_state::try_with(|state| {
        // The borrow spans the copy: every access of the table tries its
        // borrow, so a keep that the copy makes only misses.
        if let Ok(kept) = state.kept_modifiers.try_borrow()
            && let Some(Some((kept_key, modifier))) = kept.get(slot(key))
            && *kept_key == key
        {
            copy = Some(modifier.clone());
        }
    });
    copy
}

// Keeps a copy of `modifier` for `key`. Out of line: it runs once for each
// kept chain.
#[inline(never)]
fn keep_modifier(key: Key, modifier: &Modifier) {
    let taken = thread_state::try_with(|state| {
        let Ok(mut kept) = state.kept_modifiers.try_borrow_mut() else {
            return None;
        };
        if kept.is_empty() {
            kept.resize_with(SLOTS, || None);
        }
        let entry = kept.get_mut(slot(key))?;
        entry.replace((key, modifier.clone()))
    });
    // The chain that held the slot drops after the borrow ends, as its drop
    // may reach the table.
    drop(taken);
}
