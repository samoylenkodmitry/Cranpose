// The raster cache hashes of each record, kept once worked out until the
// update notes a change to the record or to a record beneath it.

use std::{cell::Cell, mem::size_of};

use crate::raster_cache::LayerRasterCacheHashes;

#[derive(Clone, Default)]
pub(crate) struct HashCache {
    rows: Vec<Cell<Option<LayerRasterCacheHashes>>>,
}

impl HashCache {
    // Makes the row at `index` hold nothing: a new record's, or a freed row a
    // new record takes.
    pub(super) fn open_row(&mut self, index: usize) {
        match self.rows.get(index) {
            Some(row) => row.set(None),
            None => self.rows.resize_with(index + 1, Cell::default),
        }
    }

    pub(super) fn forget(&self, index: usize) {
        self.rows[index].set(None);
    }

    #[cfg(test)]
    pub(super) fn forget_all(&self) {
        for row in &self.rows {
            row.set(None);
        }
    }

    // The hashes of the record at `index`, worked out by `work_out` when the
    // row holds none.
    pub(super) fn hashes(
        &self,
        index: usize,
        work_out: impl FnOnce() -> LayerRasterCacheHashes,
    ) -> LayerRasterCacheHashes {
        let row = &self.rows[index];
        if let Some(hashes) = row.get() {
            return hashes;
        }
        let hashes = work_out();
        row.set(Some(hashes));
        hashes
    }

    pub(super) fn clear(&mut self) {
        self.rows.clear();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.rows.capacity() * size_of::<Cell<Option<LayerRasterCacheHashes>>>()
    }
}
