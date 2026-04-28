use super::dense_id_map::DenseIdMap;
use crate::collections::map::HashMap;
use crate::{slot_storage::PayloadAnchor, AnchorId};
use std::mem;

#[derive(Default)]
pub(super) struct PayloadLocationRegistry {
    dense_locations: DenseIdMap<(AnchorId, usize)>,
    sparse_locations: HashMap<usize, (AnchorId, usize)>,
}

impl PayloadLocationRegistry {
    const DENSE_STORAGE_ID_LIMIT: usize = 65_536;

    pub(super) fn new() -> Self {
        Self {
            dense_locations: DenseIdMap::new(),
            sparse_locations: HashMap::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn get(&self, payload_anchor: PayloadAnchor) -> Option<(AnchorId, usize)> {
        if Self::uses_dense_storage(payload_anchor.id()) {
            self.dense_locations.get(payload_anchor.id()).copied()
        } else {
            self.sparse_locations.get(&payload_anchor.id()).copied()
        }
    }

    pub(super) fn insert(&mut self, payload_anchor: PayloadAnchor, owner: AnchorId, index: usize) {
        let location = (owner, index);
        if Self::uses_dense_storage(payload_anchor.id()) {
            self.dense_locations.insert(payload_anchor.id(), location);
        } else {
            self.sparse_locations.insert(payload_anchor.id(), location);
        }
    }

    pub(super) fn remove(&mut self, payload_anchor: PayloadAnchor) -> Option<(AnchorId, usize)> {
        if Self::uses_dense_storage(payload_anchor.id()) {
            self.dense_locations.remove(payload_anchor.id())
        } else {
            self.sparse_locations.remove(&payload_anchor.id())
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, (AnchorId, usize))> + '_ {
        self.dense_locations
            .iter()
            .map(|(payload_anchor, &(owner, payload_index))| {
                (payload_anchor, (owner, payload_index))
            })
            .chain(
                self.sparse_locations
                    .iter()
                    .map(|(&payload_anchor, &location)| (payload_anchor, location)),
            )
    }

    pub(super) fn len(&self) -> usize {
        self.dense_locations.len() + self.sparse_locations.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.dense_locations.capacity() + self.sparse_locations.capacity()
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.dense_locations.capacity() * mem::size_of::<Option<(AnchorId, usize)>>()
            + self.sparse_locations.capacity() * mem::size_of::<(usize, (AnchorId, usize))>()
    }

    pub(super) fn clear(&mut self) {
        self.dense_locations.clear();
        self.sparse_locations.clear();
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.dense_locations.shrink_to_fit();
        self.sparse_locations.shrink_to_fit();
    }

    fn uses_dense_storage(id: usize) -> bool {
        id <= Self::DENSE_STORAGE_ID_LIMIT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_payload_anchor_ids_do_not_grow_dense_storage() {
        let mut registry = PayloadLocationRegistry::new();
        let owner = AnchorId::new(1);
        let sparse_anchor = PayloadAnchor::new(2_500_000, 1);

        registry.insert(sparse_anchor, owner, 0);

        assert_eq!(registry.get(sparse_anchor), Some((owner, 0)));
        assert_eq!(registry.len(), 1);
        assert!(
            registry.capacity() < 128,
            "sparse payload ids must not allocate dense storage: capacity={}",
            registry.capacity()
        );
    }
}
