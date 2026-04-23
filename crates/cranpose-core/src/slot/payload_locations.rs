use super::dense_id_map::DenseIdMap;
use crate::AnchorId;

#[derive(Default)]
pub(super) struct PayloadLocationRegistry {
    locations: DenseIdMap<(AnchorId, usize)>,
}

impl PayloadLocationRegistry {
    pub(super) fn new() -> Self {
        Self {
            locations: DenseIdMap::new(),
        }
    }

    pub(super) fn get(&self, payload_anchor: usize) -> Option<(AnchorId, usize)> {
        self.locations.get(payload_anchor).copied()
    }

    pub(super) fn insert(&mut self, payload_anchor: usize, owner: AnchorId, index: usize) {
        self.locations.insert(payload_anchor, (owner, index));
    }

    pub(super) fn remove(&mut self, payload_anchor: usize) -> Option<(AnchorId, usize)> {
        self.locations.remove(payload_anchor)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (usize, (AnchorId, usize))> + '_ {
        self.locations
            .iter()
            .map(|(payload_anchor, &(owner, payload_index))| {
                (payload_anchor, (owner, payload_index))
            })
    }

    pub(super) fn len(&self) -> usize {
        self.locations.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.locations.capacity()
    }

    pub(super) fn clear(&mut self) {
        self.locations.clear();
    }

    pub(super) fn shrink_to_fit(&mut self) {
        self.locations.shrink_to_fit();
    }
}
