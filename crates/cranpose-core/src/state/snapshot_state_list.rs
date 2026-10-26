use std::fmt;

use super::{MutableState, OwnedMutableState, State};
use crate::RuntimeHandle;

/// A list whose reads subscribe the current scope and whose writes notify its readers, as Jetpack
/// Compose's `SnapshotStateList`. Clones share the same list.
#[derive(Clone)]
pub struct SnapshotStateList<T: Clone + 'static> {
    state: OwnedMutableState<Vec<T>>,
}

impl<T: Clone + 'static> SnapshotStateList<T> {
    /// Creates a list holding `values` in `runtime`.
    pub fn with_runtime<I>(values: I, runtime: RuntimeHandle) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let initial: Vec<T> = values.into_iter().collect();
        Self {
            state: OwnedMutableState::with_runtime(initial, runtime),
        }
    }

    /// Returns a read-only view of the state that holds the elements.
    pub fn as_state(&self) -> State<Vec<T>> {
        self.state.as_state()
    }

    /// Returns the writable state that holds the elements.
    pub fn as_mutable_state(&self) -> MutableState<Vec<T>> {
        self.state.handle()
    }

    /// Returns the number of elements.
    pub fn len(&self) -> usize {
        self.state.with(Vec::len)
    }

    /// Returns `true` if the list has no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a copy of the elements.
    pub fn to_vec(&self) -> Vec<T> {
        self.state.with(Clone::clone)
    }

    /// Returns a copy of the elements for iteration. Later writes do not change that copy.
    pub fn iter(&self) -> Vec<T> {
        self.to_vec()
    }

    /// Returns a copy of the element at `index`. Panics if `index` is out of range.
    pub fn get(&self, index: usize) -> T {
        self.state.with(|values| values[index].clone())
    }

    /// Returns a copy of the element at `index`, or `None` if it is out of range.
    pub fn get_opt(&self, index: usize) -> Option<T> {
        self.state.with(|values| values.get(index).cloned())
    }

    /// Returns a copy of the first element, or `None` if the list is empty.
    pub fn first(&self) -> Option<T> {
        self.get_opt(0)
    }

    /// Returns a copy of the last element, or `None` if the list is empty.
    pub fn last(&self) -> Option<T> {
        self.state.with(|values| values.last().cloned())
    }

    /// Appends `value` at the end.
    pub fn push(&self, value: T) {
        self.state.update(|values| values.push(value));
    }

    /// Appends all elements of `iter` in one write.
    pub fn extend<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.state.update(|values| values.extend(iter));
    }

    /// Inserts `value` at `index`, moving later elements back. Panics if `index` is past the end.
    pub fn insert(&self, index: usize, value: T) {
        self.state.update(|values| values.insert(index, value));
    }

    /// Replaces the element at `index` and returns the old one. Panics if `index` is out of range.
    pub fn set(&self, index: usize, value: T) -> T {
        self.state
            .update(|values| std::mem::replace(&mut values[index], value))
    }

    /// Removes and returns the element at `index`. Panics if `index` is out of range.
    pub fn remove(&self, index: usize) -> T {
        self.state.update(|values| values.remove(index))
    }

    /// Removes and returns the last element, or `None` if the list is empty.
    pub fn pop(&self) -> Option<T> {
        self.state.update(Vec::pop)
    }

    /// Removes all elements.
    pub fn clear(&self) {
        self.state.replace(Vec::new());
    }

    /// Keeps the elements for which `predicate` returns `true`.
    pub fn retain<F>(&self, mut predicate: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.state
            .update(|values| values.retain(|value| predicate(value)));
    }

    /// Replaces all elements with those of `iter` in one write.
    pub fn replace_with<I>(&self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.state.replace(iter.into_iter().collect());
    }
}

impl<T: fmt::Debug + Clone + 'static> fmt::Debug for SnapshotStateList<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let contents = self.to_vec();
        f.debug_struct("SnapshotStateList")
            .field("values", &contents)
            .finish()
    }
}
