use crate::{runtime, AnchorId, NodeId};

struct ChunkedPending<T> {
    sealed: Vec<Vec<T>>,
    active: Vec<T>,
    len: usize,
}

impl<T> Default for ChunkedPending<T> {
    fn default() -> Self {
        Self {
            sealed: Vec::new(),
            active: Vec::new(),
            len: 0,
        }
    }
}

impl<T> ChunkedPending<T> {
    fn len(&self) -> usize {
        self.len
    }

    fn capacity(&self) -> usize {
        self.active.capacity() + self.sealed.iter().map(Vec::capacity).sum::<usize>()
    }

    fn push(&mut self, value: T, initial_capacity: usize, chunk_capacity: usize) {
        if self.active.capacity() == 0 {
            self.active = Vec::with_capacity(initial_capacity);
        } else if self.active.len() == self.active.capacity() {
            let next_capacity = chunk_capacity.max(initial_capacity);
            let full_chunk = std::mem::replace(&mut self.active, Vec::with_capacity(next_capacity));
            self.sealed.push(full_chunk);
        }

        self.active.push(value);
        self.len += 1;
    }

    fn clear_and_drop_reverse(&mut self) {
        while let Some(value) = self.active.pop() {
            drop(value);
        }

        while let Some(mut chunk) = self.sealed.pop() {
            while let Some(value) = chunk.pop() {
                drop(value);
            }
        }

        self.len = 0;
    }

    fn drain_forward(&mut self, mut visitor: impl FnMut(T)) {
        let sealed = std::mem::take(&mut self.sealed);
        for chunk in sealed {
            for value in chunk {
                visitor(value);
            }
        }
        let active = std::mem::take(&mut self.active);
        for value in active {
            visitor(value);
        }
        self.len = 0;
    }

    fn trim_retained_capacity(&mut self, retained: usize, initial_capacity: usize) {
        self.sealed = Vec::new();
        if self.active.capacity() > retained.saturating_mul(4) {
            self.active = Vec::with_capacity(retained.max(initial_capacity));
        }
    }
}

pub(in crate::slot_table) struct PendingDrops<T> {
    inner: ChunkedPending<T>,
}

impl<T> Default for PendingDrops<T> {
    fn default() -> Self {
        Self {
            inner: ChunkedPending::default(),
        }
    }
}

impl<T> PendingDrops<T> {
    const CHUNK_CAPACITY: usize = 1024;
    const INITIAL_CAPACITY: usize = 4;

    pub(in crate::slot_table) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(in crate::slot_table) fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub(in crate::slot_table) fn push(&mut self, value: T) {
        self.inner
            .push(value, Self::INITIAL_CAPACITY, Self::CHUNK_CAPACITY);
    }

    pub(in crate::slot_table) fn clear_and_drop_reverse(&mut self) {
        let _teardown = runtime::enter_state_teardown_scope();
        self.inner.clear_and_drop_reverse();
    }

    pub(in crate::slot_table) fn trim_retained_capacity(&mut self, retained: usize) {
        self.inner
            .trim_retained_capacity(retained, Self::INITIAL_CAPACITY);
    }
}

#[derive(Default)]
pub(in crate::slot_table) struct OrphanedNodeIds {
    inner: ChunkedPending<OrphanedNode>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OrphanedNode {
    pub id: NodeId,
    pub generation: u32,
    pub(crate) anchor: AnchorId,
}

impl OrphanedNode {
    pub(in crate::slot_table) fn new(id: NodeId, generation: u32, anchor: AnchorId) -> Self {
        Self {
            id,
            generation,
            anchor,
        }
    }
}

impl OrphanedNodeIds {
    const CHUNK_CAPACITY: usize = 1024;
    pub(in crate::slot_table) const INITIAL_CAPACITY: usize = 32;

    pub(in crate::slot_table) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(in crate::slot_table) fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub(in crate::slot_table) fn push(&mut self, orphaned: OrphanedNode) {
        self.inner
            .push(orphaned, Self::INITIAL_CAPACITY, Self::CHUNK_CAPACITY);
    }

    pub(in crate::slot_table) fn drain_forward(&mut self, visitor: impl FnMut(OrphanedNode)) {
        self.inner.drain_forward(visitor);
        self.trim_retained_capacity(Self::INITIAL_CAPACITY);
    }

    pub(in crate::slot_table) fn trim_retained_capacity(&mut self, retained: usize) {
        self.inner
            .trim_retained_capacity(retained, Self::INITIAL_CAPACITY);
    }
}
