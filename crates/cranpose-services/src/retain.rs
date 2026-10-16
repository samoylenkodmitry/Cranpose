//! `retain`: a remembered value that outlives its composable's leaving the
//! composition while the saveable scope it was made in lives.

use std::{any::Any, collections::HashMap, rc::Rc};

use crate::SaveableScope;

/// The values [`retain`] keeps on this thread, by their key in their scope.
#[derive(Default)]
pub(crate) struct RetainedValues {
    values: HashMap<String, Retained>,
}

struct Retained {
    value: Rc<dyn Any>,
    /// How many composables hold the value now.
    holders: u32,
    /// Whether the value outlives its last holder: it was made in a
    /// scope that keeps what leaves it until the scope is forgotten.
    kept: bool,
}

impl RetainedValues {
    /// Takes the values under the keys `forgotten` matches, for the caller
    /// to drop once it no longer borrows the table.
    pub(crate) fn forget(&mut self, forgotten: impl Fn(&str) -> bool) -> Vec<Rc<dyn Any>> {
        self.values
            .extract_if(|key, _| forgotten(key))
            .map(|(_, retained)| retained.value)
            .collect()
    }
}

/// Compose's `retain`: returns the value `factory` made, at the first
/// composition of this call site in the current [`SaveableScope`] under
/// `key`, and the same value after the composable leaves the composition and
/// comes back, for as long as the scope lives. A navigation host gives each
/// back-stack entry its own scope, so a screen gets its value back while
/// its entry stays on the back stack, and the value is dropped when the
/// entry is popped. In the root scope the value is dropped as soon as no
/// composable holds it, as `remember` drops its value.
///
/// Unlike `rememberSaveable`, the value is never saved: it may hold anything,
/// a connection or a decoder included, and does not survive the process.
/// Dropping the value is its retirement. `key` must be unique in the scope;
/// two composables that retain under one key share the value.
#[track_caller]
pub fn retain<T: Clone + 'static>(key: &str, factory: impl FnOnce() -> T) -> T {
    cranpose_core::remember(|| Hold::new(key, factory)).with(|hold| hold.value.clone())
}

/// A composable's hold on a retained value: made when the call site enters
/// the composition, dropped when it leaves.
struct Hold<T> {
    key: String,
    value: T,
}

impl<T: Clone + 'static> Hold<T> {
    fn new(key: &str, factory: impl FnOnce() -> T) -> Self {
        let scope = SaveableScope::current();
        let key = scope.key(key);
        let held = crate::thread_state::with(|state| {
            let mut retained = state.retained.borrow_mut();
            let entry = retained.values.get_mut(&key)?;
            let value = entry.value.downcast_ref::<T>()?.clone();
            entry.holders += 1;
            Some(value)
        });
        if let Some(value) = held {
            return Self { key, value };
        }
        // The factory runs while nothing is borrowed: it may retain too.
        let value = factory();
        let replaced = crate::thread_state::with(|state| {
            state.retained.borrow_mut().values.insert(
                key.clone(),
                Retained {
                    value: Rc::new(value.clone()),
                    holders: 1,
                    kept: scope.namespace().is_some(),
                },
            )
        });
        drop(replaced);
        Self { key, value }
    }
}

impl<T> Drop for Hold<T> {
    fn drop(&mut self) {
        let retired = crate::thread_state::try_with(|state| {
            let mut retained = state.retained.borrow_mut();
            let entry = retained.values.get_mut(&self.key)?;
            entry.holders = entry.holders.saturating_sub(1);
            if entry.holders > 0 || entry.kept {
                return None;
            }
            retained.values.remove(&self.key)
        })
        .flatten();
        drop(retired);
    }
}
