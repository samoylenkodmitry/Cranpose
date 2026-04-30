pub(super) struct DenseIdMap<T> {
    entries: Vec<Option<T>>,
    len: usize,
}

impl<T> Default for DenseIdMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DenseIdMap<T> {
    pub(super) fn new() -> Self {
        Self {
            entries: Vec::new(),
            len: 0,
        }
    }

    pub(super) fn get(&self, id: usize) -> Option<&T> {
        self.entries.get(id).and_then(Option::as_ref)
    }

    pub(super) fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        self.entries.get_mut(id).and_then(Option::as_mut)
    }

    pub(super) fn insert(&mut self, id: usize, value: T) -> Option<T> {
        self.ensure_index(id);
        let entry = &mut self.entries[id];
        if entry.is_none() {
            self.len += 1;
        }
        entry.replace(value)
    }

    pub(super) fn remove(&mut self, id: usize) -> Option<T> {
        let entry = self.entries.get_mut(id)?;
        let removed = entry.take();
        if removed.is_some() {
            self.len -= 1;
            if id + 1 == self.entries.len() {
                self.trim_trailing_empty();
            }
        }
        removed
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.len = 0;
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, &T)> + '_ {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(id, entry)| entry.as_ref().map(|value| (id, value)))
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.trim_trailing_empty();
        self.entries.shrink_to_fit();
    }

    fn ensure_index(&mut self, id: usize) {
        if id >= self.entries.len() {
            self.entries.resize_with(id + 1, || None);
        }
    }

    fn trim_trailing_empty(&mut self) {
        let retained = self
            .entries
            .iter()
            .rposition(|entry| entry.is_some())
            .map_or(0, |index| index + 1);
        self.entries.truncate(retained);
    }
}
