//! A table of values by an id that a counter hands out, such as a stable
//! node id: live ids cluster and retire roughly in the order they were made,
//! so a two-level radix table holds them. A lookup reads two arrays where a
//! hash map hashed the id and probed its groups. A page is freed once all of
//! its values are vacant, and one spare page waits for the next range the
//! counter reaches.

/// The ids one page of an [`IdPages`] holds unless it names another count.
pub const PAGE_LEN: usize = 128;
/// The pages one directory of an [`IdPages`] lists unless it names another
/// count.
pub const DIRECTORY_LEN: usize = 128;

/// A value an [`IdPages`] entry holds, with the state an entry without a
/// value is in.
pub trait Vacant {
    /// The state of an entry that holds nothing.
    fn vacant() -> Self;

    /// Whether the entry holds nothing.
    fn is_vacant(&self) -> bool;
}

impl<T> Vacant for Option<T> {
    fn vacant() -> Self {
        None
    }

    fn is_vacant(&self) -> bool {
        self.is_none()
    }
}

struct Page<T, const PAGE: usize> {
    entries: [T; PAGE],
    occupied: usize,
}

impl<T: Vacant, const PAGE: usize> Page<T, PAGE> {
    fn new() -> Box<Self> {
        Box::new(Self {
            entries: std::array::from_fn(|_| T::vacant()),
            occupied: 0,
        })
    }
}

struct Directory<T, const PAGE: usize, const DIRECTORY: usize> {
    pages: [Option<Box<Page<T, PAGE>>>; DIRECTORY],
    occupied: usize,
}

impl<T, const PAGE: usize, const DIRECTORY: usize> Directory<T, PAGE, DIRECTORY> {
    fn new() -> Box<Self> {
        Box::new(Self {
            pages: std::array::from_fn(|_| None),
            occupied: 0,
        })
    }
}

// The directory, page and entry an id lives at, in pages of `PAGE` ids and
// directories of `DIRECTORY` pages.
#[inline]
fn split<const PAGE: usize, const DIRECTORY: usize>(id: usize) -> (usize, usize, usize) {
    const {
        assert!(
            PAGE.is_power_of_two() && DIRECTORY.is_power_of_two(),
            "pages and directories split an id by its bits"
        );
    };
    let page_bits = PAGE.trailing_zeros();
    let directory_bits = DIRECTORY.trailing_zeros();
    (
        id >> (page_bits + directory_bits),
        (id >> page_bits) & (DIRECTORY - 1),
        id & (PAGE - 1),
    )
}

/// Values by id in pages of `PAGE` ids, which come and go with the values,
/// listed in directories of `DIRECTORY` pages. Both counts are powers of
/// two: a table of few values takes smaller ones.
pub struct IdPages<T, const PAGE: usize = PAGE_LEN, const DIRECTORY: usize = DIRECTORY_LEN> {
    directories: Vec<Option<Box<Directory<T, PAGE, DIRECTORY>>>>,
    occupied: usize,
    pages: usize,
    spare: Option<Box<Page<T, PAGE>>>,
}

impl<T, const PAGE: usize, const DIRECTORY: usize> Default for IdPages<T, PAGE, DIRECTORY> {
    fn default() -> Self {
        Self {
            directories: Vec::new(),
            occupied: 0,
            pages: 0,
            spare: None,
        }
    }
}

impl<T: Vacant, const PAGE: usize, const DIRECTORY: usize> IdPages<T, PAGE, DIRECTORY> {
    /// The entry of `id`, vacant or not; `None` when no page holds it, so
    /// it is vacant.
    #[inline]
    pub fn entry(&self, id: usize) -> Option<&T> {
        let (directory, page, entry) = split::<PAGE, DIRECTORY>(id);
        self.directories
            .get(directory)
            .and_then(Option::as_deref)
            .and_then(|directory| directory.pages[page].as_deref())
            .map(|page| &page.entries[entry])
    }

    /// The entry of `id` to change in place, which must stay as vacant or
    /// held as it was: [`Self::update`] changes that. `None` when no page
    /// holds it.
    #[inline]
    pub fn entry_mut(&mut self, id: usize) -> Option<&mut T> {
        let (directory, page, entry) = split::<PAGE, DIRECTORY>(id);
        self.directories
            .get_mut(directory)
            .and_then(Option::as_deref_mut)
            .and_then(|directory| directory.pages[page].as_deref_mut())
            .map(|page| &mut page.entries[entry])
    }

    /// Changes `id`'s entry, allocating its page if it has none and freeing
    /// the page once all of its entries are vacant.
    pub fn update(&mut self, id: usize, change: impl FnOnce(&mut T)) {
        let (directory_index, page_index, entry_index) = split::<PAGE, DIRECTORY>(id);
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
        let held_before = usize::from(!entry.is_vacant());
        change(entry);
        let held_after = usize::from(!entry.is_vacant());
        page.occupied = page.occupied + held_after - held_before;
        self.occupied = self.occupied + held_after - held_before;
        if page.occupied == 0 {
            free_page(directory, page_index, &mut self.pages, &mut self.spare);
            if directory.occupied == 0 {
                self.directories[directory_index] = None;
            }
        }
    }

    /// Makes vacant every held entry for which `keep` says no, freeing the
    /// pages that empty.
    pub fn retain(&mut self, mut keep: impl FnMut(usize, &T) -> bool) {
        for directory_index in 0..self.directories.len() {
            let Some(directory) = self.directories[directory_index].as_deref_mut() else {
                continue;
            };
            for page_index in 0..DIRECTORY {
                let Some(page) = directory.pages[page_index].as_deref_mut() else {
                    continue;
                };
                let page_bits = PAGE.trailing_zeros();
                let base = (directory_index << (page_bits + DIRECTORY.trailing_zeros()))
                    | (page_index << page_bits);
                for (offset, entry) in page.entries.iter_mut().enumerate() {
                    if entry.is_vacant() || keep(base | offset, entry) {
                        continue;
                    }
                    *entry = T::vacant();
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

    /// The entries that hold a value.
    pub fn occupied(&self) -> usize {
        self.occupied
    }

    /// The ids the allocated pages can hold.
    pub fn capacity(&self) -> usize {
        self.pages * PAGE
    }
}

// Frees an empty page, keeping it as the spare when there is none.
fn free_page<T, const PAGE: usize, const DIRECTORY: usize>(
    directory: &mut Directory<T, PAGE, DIRECTORY>,
    page_index: usize,
    pages: &mut usize,
    spare: &mut Option<Box<Page<T, PAGE>>>,
) {
    let freed = directory.pages[page_index].take();
    directory.occupied -= 1;
    *pages -= 1;
    if spare.is_none() {
        *spare = freed;
    }
}
