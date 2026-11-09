// What a record draws, in final draw order: its draws and its children, as
// handles into the store, each record's in one span of the store's items.

use std::num::NonZeroU32;

use cranpose_core::{RecordId, collections::growth_slack::GrowthSlack};

/// One item of a record: a child record or one of the record's draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Item(NonZeroU32);

/// What an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A child record.
    Child,
    /// A draw primitive with its clip.
    Draw,
    /// A text.
    Text,
    /// A run of one draw command's primitives.
    Run,
    /// Shapes the store keeps in a chunk it shares among shape items.
    Shape,
}

const KIND_BITS: u32 = 3;
const KIND_MASK: u32 = (1 << KIND_BITS) - 1;

impl Item {
    pub(crate) fn new(kind: ItemKind, index: usize) -> Self {
        let tag = match kind {
            ItemKind::Child => 1,
            ItemKind::Draw => 2,
            ItemKind::Text => 3,
            ItemKind::Run => 4,
            ItemKind::Shape => 5,
        };
        let index = u32::try_from(index)
            .ok()
            .filter(|index| *index < 1 << (32 - KIND_BITS))
            .expect("a store holds fewer than 2^29 entries of a kind");
        Self(NonZeroU32::new(index << KIND_BITS | tag).expect("the kind tag is never zero"))
    }

    pub(crate) fn child(record: RecordId) -> Self {
        Self::new(ItemKind::Child, record.index())
    }

    /// What the item is.
    pub fn kind(self) -> ItemKind {
        match self.0.get() & KIND_MASK {
            1 => ItemKind::Child,
            2 => ItemKind::Draw,
            3 => ItemKind::Text,
            4 => ItemKind::Run,
            _ => ItemKind::Shape,
        }
    }

    pub(crate) fn index(self) -> usize {
        (self.0.get() >> KIND_BITS) as usize
    }

    /// The child record, when the item is one.
    pub fn as_child(self) -> Option<RecordId> {
        (self.kind() == ItemKind::Child)
            .then(|| RecordId::from_index(self.index()).expect("a child item names a record row"))
    }
}

// A record's entries in a pool: `len` of them from `start`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Span {
    start: u32,
    len: u32,
}

impl Span {
    pub(crate) fn len(self) -> usize {
        self.len as usize
    }

    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

// Entries of the store's records, each record's in one span. A span takes
// room of the power of two at or above its length, so it grows in place up
// to that room; a freed span's room goes to the next span of its size class.
#[derive(Clone)]
pub(crate) struct SpanPool<T> {
    items: Vec<T>,
    free: Vec<Vec<u32>>,
    // Whether the items changed length since their room was last checked:
    // only a change of length can leave room to give back.
    resized: bool,
}

impl<T> Default for SpanPool<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            free: Vec::new(),
            resized: false,
        }
    }
}

fn class_of(len: u32) -> usize {
    len.next_power_of_two().trailing_zeros() as usize
}

impl<T: Copy> SpanPool<T> {
    pub(crate) fn span(&self, span: Span) -> &[T] {
        &self.items[span.range()]
    }

    // Writes `items` into `span`'s room when they fit it, else into new
    // room, giving the old back.
    pub(crate) fn write(&mut self, span: Span, items: &[T]) -> Span {
        // Most often a record keeps as many entries as it held.
        if items.len() == span.len() {
            self.items[span.range()].copy_from_slice(items);
            return span;
        }
        let Some(&first) = items.first() else {
            self.free_span(span);
            return Span::default();
        };
        let len = u32::try_from(items.len()).expect("a record holds fewer than 2^32 entries");
        let span = if span.len > 0 && class_of(span.len) == class_of(len) {
            Span {
                start: span.start,
                len,
            }
        } else {
            self.free_span(span);
            Span {
                start: self.take_room(len, first),
                len,
            }
        };
        self.items[span.range()].copy_from_slice(items);
        span
    }

    pub(crate) fn free_span(&mut self, span: Span) {
        if span.len == 0 {
            return;
        }
        let class = class_of(span.len);
        if self.free.len() <= class {
            self.free.resize_with(class + 1, Vec::new);
        }
        self.free[class].push(span.start);
    }

    // Room for `len` entries, new room filled with `filler`.
    fn take_room(&mut self, len: u32, filler: T) -> u32 {
        let class = class_of(len);
        if let Some(start) = self.free.get_mut(class).and_then(Vec::pop) {
            return start;
        }
        let start = u32::try_from(self.items.len()).expect("fewer than 2^32 items");
        let room = 1usize << class;
        self.items.resize(self.items.len() + room, filler);
        self.resized = true;
        start
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.items.capacity() * size_of::<T>()
            + self.free.capacity() * size_of::<Vec<u32>>()
            + self
                .free
                .iter()
                .map(|class| class.capacity() * size_of::<u32>())
                .sum::<usize>()
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.free.iter_mut().for_each(Vec::clear);
        self.resized = true;
    }

    pub(crate) fn trim_growth_slack(&mut self) {
        if std::mem::take(&mut self.resized) {
            self.items.trim_growth_slack();
        }
    }
}

// Lists of items a writer fills before it writes them to their record, kept
// between updates so a writer allocates no list of its own.
#[derive(Clone, Default)]
pub(crate) struct ItemLists {
    lists: Vec<Vec<Item>>,
}

impl ItemLists {
    pub(crate) fn take(&mut self) -> Vec<Item> {
        self.lists.pop().unwrap_or_default()
    }

    pub(crate) fn give(&mut self, mut list: Vec<Item>) {
        list.clear();
        if list.capacity() > 0 {
            self.lists.push(list);
        }
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.lists.capacity() * size_of::<Vec<Item>>()
            + self
                .lists
                .iter()
                .map(|list| list.capacity() * size_of::<Item>())
                .sum::<usize>()
    }
}
