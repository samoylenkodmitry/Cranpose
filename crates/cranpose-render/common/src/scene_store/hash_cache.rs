// The raster cache hashes of the records a reader asked for, each kept with
// the subtree revision it was worked out at: a change to the record or to a
// record beneath it raises that revision, which leaves the hashes stale.
// Only isolated layers and the records beneath them are hashed, so the
// cache keeps no row for every other record.

use std::{cell::RefCell, mem::size_of};

use cranpose_core::{SceneUpdate, collections::map::HashMap};

use crate::raster_cache::LayerRasterCacheHashes;

type Entry = (SceneUpdate, LayerRasterCacheHashes);

#[derive(Clone, Default)]
pub(crate) struct HashCache {
    entries: RefCell<HashMap<u32, Entry>>,
}

impl HashCache {
    // Drops the hashes of the record at `index`, which leaves its row.
    pub(super) fn remove(&mut self, index: usize) {
        let entries = self.entries.get_mut();
        if !entries.is_empty() {
            entries.remove(&row(index));
        }
    }

    // The hashes of the record at `index`, whose subtree revision is
    // `subtree_rev`, worked out by `work_out` when the cache holds none of
    // that revision. `work_out` asks for the hashes of the records beneath,
    // so no borrow of the cache spans it.
    pub(super) fn hashes(
        &self,
        index: usize,
        subtree_rev: SceneUpdate,
        work_out: impl FnOnce() -> LayerRasterCacheHashes,
    ) -> LayerRasterCacheHashes {
        let kept = self.entries.borrow().get(&row(index)).copied();
        if let Some((rev, hashes)) = kept
            && rev == subtree_rev
        {
            return hashes;
        }
        let hashes = work_out();
        self.entries
            .borrow_mut()
            .insert(row(index), (subtree_rev, hashes));
        hashes
    }

    pub(super) fn clear(&mut self) {
        self.entries.get_mut().clear();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        // A table keeps one control byte beside each slot.
        self.entries.borrow().capacity() * (size_of::<(u32, Entry)>() + 1)
    }
}

fn row(index: usize) -> u32 {
    u32::try_from(index).expect("fewer than 2^32 records")
}
