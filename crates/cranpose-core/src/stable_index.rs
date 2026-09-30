//! Where each stable node id's node lives in the applier's dense storage, and
//! how many times that id's storage was recycled.
//!
//! Stable ids come from a counter, so live ids cluster and retire roughly in
//! the order they were made. A two-level radix table indexes them: a lookup
//! reads two arrays, where a hash map hashed the id and probed its groups. A
//! page is freed once none of its ids has a slot or a generation, and one
//! spare page waits for the next range the counter reaches.

use crate::NodeId;

const PAGE_BITS: usize = 9;
const PAGE_LEN: usize = 1 << PAGE_BITS;
const DIRECTORY_BITS: usize = 10;
const DIRECTORY_LEN: usize = 1 << DIRECTORY_BITS;
const NO_SLOT: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Entry {
    slot: u32,
    generation: u32,
}

impl Entry {
    const EMPTY: Self = Self {
        slot: NO_SLOT,
        generation: 0,
    };

    fn has_slot(self) -> bool {
        self.slot != NO_SLOT
    }

    fn is_empty(self) -> bool {
        !self.has_slot() && self.generation == 0
    }
}

struct Page {
    entries: [Entry; PAGE_LEN],
    occupied: usize,
}

impl Page {
    fn new() -> Box<Self> {
        Box::new(Self {
            entries: [Entry::EMPTY; PAGE_LEN],
            occupied: 0,
        })
    }
}

struct Directory {
    pages: [Option<Box<Page>>; DIRECTORY_LEN],
    occupied: usize,
}

impl Directory {
    fn new() -> Box<Self> {
        Box::new(Self {
            pages: std::array::from_fn(|_| None),
            occupied: 0,
        })
    }
}

/// The directory, page and entry an id lives at.
fn split(id: NodeId) -> (usize, usize, usize) {
    (
        id >> (PAGE_BITS + DIRECTORY_BITS),
        (id >> PAGE_BITS) & (DIRECTORY_LEN - 1),
        id & (PAGE_LEN - 1),
    )
}

#[derive(Default)]
pub(crate) struct StableIndex {
    directories: Vec<Option<Box<Directory>>>,
    live: usize,
    occupied: usize,
    pages: usize,
    spare: Option<Box<Page>>,
}

impl StableIndex {
    fn entry(&self, id: NodeId) -> Entry {
        let (directory, page, entry) = split(id);
        self.directories
            .get(directory)
            .and_then(Option::as_deref)
            .and_then(|directory| directory.pages[page].as_deref())
            .map_or(Entry::EMPTY, |page| page.entries[entry])
    }

    /// The dense storage slot `id`'s node sits in.
    pub(crate) fn slot(&self, id: NodeId) -> Option<usize> {
        let slot = self.entry(id).slot;
        (slot != NO_SLOT).then_some(slot as usize)
    }

    /// How many times `id`'s node left the tree; `0` for an id never seen.
    pub(crate) fn generation(&self, id: NodeId) -> u32 {
        self.entry(id).generation
    }

    /// Puts `id`'s node in dense storage `slot`, keeping its generation.
    pub(crate) fn set_slot(&mut self, id: NodeId, slot: usize) {
        debug_assert!(slot < NO_SLOT as usize, "dense slot {slot} past the index");
        self.update(id, |entry| entry.slot = slot as u32);
    }

    /// Puts a newly made `id`'s node in dense storage `slot`, at generation 0.
    pub(crate) fn insert_fresh(&mut self, id: NodeId, slot: usize) {
        debug_assert!(slot < NO_SLOT as usize, "dense slot {slot} past the index");
        self.update(id, |entry| {
            entry.slot = slot as u32;
            entry.generation = 0;
        });
    }

    /// Takes `id`'s node out of dense storage and counts one more generation.
    pub(crate) fn release(&mut self, id: NodeId) {
        self.update(id, |entry| {
            entry.slot = NO_SLOT;
            entry.generation = entry.generation.wrapping_add(1);
        });
    }

    /// Forgets the generations of ids without a node for which `keep` says no.
    pub(crate) fn retain_retired(&mut self, keep: impl Fn(NodeId) -> bool) {
        for directory_index in 0..self.directories.len() {
            let Some(directory) = self.directories[directory_index].as_deref_mut() else {
                continue;
            };
            for page_index in 0..DIRECTORY_LEN {
                let Some(page) = directory.pages[page_index].as_deref_mut() else {
                    continue;
                };
                let base =
                    (directory_index << (PAGE_BITS + DIRECTORY_BITS)) | (page_index << PAGE_BITS);
                for (offset, entry) in page.entries.iter_mut().enumerate() {
                    if entry.has_slot() || entry.is_empty() || keep(base | offset) {
                        continue;
                    }
                    *entry = Entry::EMPTY;
                    page.occupied -= 1;
                    self.occupied -= 1;
                }
                if page.occupied == 0 {
                    free_page(directory, page_index, &mut self.pages, &mut self.spare);
                }
            }
            if directory.occupied == 0 {
                self.directories[directory_index] = None;
            }
        }
    }

    /// Ids with a node in dense storage.
    pub(crate) fn live(&self) -> usize {
        self.live
    }

    /// Ids with a node or a generation.
    pub(crate) fn occupied(&self) -> usize {
        self.occupied
    }

    /// Ids the allocated pages can hold.
    pub(crate) fn capacity(&self) -> usize {
        self.pages * PAGE_LEN
    }

    /// Changes `id`'s entry, allocating its page if it has none and freeing
    /// the page once it empties.
    fn update(&mut self, id: NodeId, change: impl FnOnce(&mut Entry)) {
        let (directory_index, page_index, entry_index) = split(id);
        if self.directories.len() <= directory_index {
            self.directories.resize_with(directory_index + 1, || None);
        }
        let directory = self.directories[directory_index].get_or_insert_with(Directory::new);
        if directory.pages[page_index].is_none() {
            directory.pages[page_index] = Some(self.spare.take().unwrap_or_else(Page::new));
            directory.occupied += 1;
            self.pages += 1;
        }
        let Some(page) = directory.pages[page_index].as_deref_mut() else {
            return;
        };
        let entry = &mut page.entries[entry_index];
        let before = *entry;
        change(entry);
        let after = *entry;
        self.live += usize::from(after.has_slot());
        self.live -= usize::from(before.has_slot());
        let filled = usize::from(!after.is_empty());
        let emptied = usize::from(!before.is_empty());
        page.occupied = page.occupied + filled - emptied;
        self.occupied = self.occupied + filled - emptied;
        if page.occupied == 0 {
            free_page(directory, page_index, &mut self.pages, &mut self.spare);
            if directory.occupied == 0 {
                self.directories[directory_index] = None;
            }
        }
    }
}

/// Frees an empty page, keeping it as the spare when there is none.
fn free_page(
    directory: &mut Directory,
    page_index: usize,
    pages: &mut usize,
    spare: &mut Option<Box<Page>>,
) {
    let freed = directory.pages[page_index].take();
    directory.occupied -= 1;
    *pages -= 1;
    if spare.is_none() {
        *spare = freed;
    }
}

#[cfg(test)]
#[path = "tests/stable_index_tests.rs"]
mod tests;
