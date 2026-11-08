// What a record draws, in final draw order: its draws and its children, as
// handles into the store, each record's in one span of the store's items.

use std::num::NonZeroU32;

use cranpose_core::RecordId;

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
            _ => ItemKind::Run,
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

/// A record's items: `len` of them from `start` in the store's items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ItemSpan {
    pub(crate) start: u32,
    pub(crate) len: u32,
}

impl ItemSpan {
    /// How many items the span holds.
    pub fn len(self) -> usize {
        self.len as usize
    }

    /// Whether the span holds no item.
    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start + self.len) as usize
    }
}

// The store's items. A span takes room of the power of two at or above its
// length, so it grows in place up to that room; a freed span's room goes to
// the next span of its size class.
#[derive(Clone, Default)]
pub(crate) struct ItemPool {
    items: Vec<Item>,
    free: Vec<Vec<u32>>,
}

fn class_of(len: u32) -> usize {
    len.next_power_of_two().trailing_zeros() as usize
}

impl ItemPool {
    pub(crate) fn span(&self, span: ItemSpan) -> &[Item] {
        &self.items[span.range()]
    }

    // Writes `items` into `span`'s room when they fit it, else into new
    // room, giving the old back.
    pub(crate) fn write(&mut self, span: ItemSpan, items: &[Item]) -> ItemSpan {
        let len = u32::try_from(items.len()).expect("a record holds fewer than 2^32 items");
        let span = if len == 0 {
            self.free_span(span);
            ItemSpan::default()
        } else if span.len > 0 && class_of(span.len) == class_of(len) {
            ItemSpan {
                start: span.start,
                len,
            }
        } else {
            self.free_span(span);
            ItemSpan {
                start: self.take_room(len),
                len,
            }
        };
        self.items[span.range()].copy_from_slice(items);
        span
    }

    pub(crate) fn free_span(&mut self, span: ItemSpan) {
        if span.len == 0 {
            return;
        }
        let class = class_of(span.len);
        if self.free.len() <= class {
            self.free.resize_with(class + 1, Vec::new);
        }
        self.free[class].push(span.start);
    }

    fn take_room(&mut self, len: u32) -> u32 {
        let class = class_of(len);
        if let Some(start) = self.free.get_mut(class).and_then(Vec::pop) {
            return start;
        }
        let start = u32::try_from(self.items.len()).expect("fewer than 2^32 items");
        let room = 1usize << class;
        self.items
            .resize(self.items.len() + room, Item::new(ItemKind::Draw, 0));
        start
    }

    pub(crate) fn clear(&mut self) {
        self.items.clear();
        self.free.iter_mut().for_each(Vec::clear);
    }
}
