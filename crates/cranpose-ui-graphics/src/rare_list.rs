// A list few values fill, kept in a box that a value holds only once an
// item joins: an empty list takes one word, where a `Vec` takes three.
#[derive(Clone, Debug)]
pub(crate) struct RareList<T>(Option<Box<Vec<T>>>);

impl<T> Default for RareList<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> RareList<T> {
    pub(crate) fn push(&mut self, item: T) {
        self.0.get_or_insert_default().push(item);
    }

    // Empties the list, keeping its room for the next items.
    pub(crate) fn clear(&mut self) {
        if let Some(list) = self.0.as_deref_mut() {
            list.clear();
        }
    }

    // The list's storage, `None` while no item ever joined.
    pub(crate) fn storage_mut(&mut self) -> Option<&mut Vec<T>> {
        self.0.as_deref_mut()
    }

    // The heap the list holds: its box and its capacity.
    pub(crate) fn heap_bytes(&self) -> usize {
        self.0.as_deref().map_or(0, |list| {
            size_of::<Vec<T>>() + list.capacity() * size_of::<T>()
        })
    }
}

impl<T> std::ops::Deref for RareList<T> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
}

impl<T: PartialEq> PartialEq for RareList<T> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
