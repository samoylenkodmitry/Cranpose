use std::num::{NonZeroU16, NonZeroU32};

/// A record of a scene store: its row, one more than its index, so that an
/// absent record costs no space beside a present one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecordId(NonZeroU32);

impl RecordId {
    /// The record at row `index`; `None` past the last row a store can hold.
    pub fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(1))
            .and_then(NonZeroU32::new)
            .map(Self)
    }

    /// The row of the record.
    pub fn index(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

/// A scene store, one per surface renderer, so a layout node that moves to
/// another window knows which store holds its records.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StoreId(NonZeroU16);

impl StoreId {
    /// The store numbered `number`; `None` for zero.
    pub fn new(number: u16) -> Option<Self> {
        NonZeroU16::new(number).map(Self)
    }

    /// The store's number.
    pub fn number(self) -> u16 {
        self.0.get()
    }
}

/// A record held outside a frame: its row and the generation the row had,
/// which tells a holder that the row went to another record since.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RecordRef {
    /// The record's row.
    pub id: RecordId,
    /// The generation of the row when the holder took it.
    pub generation: u32,
}

/// A scene update's number. It wraps: of two numbers less than 2^31 apart,
/// the one reached by counting on from the other is the newer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SceneUpdate(u32);

impl SceneUpdate {
    /// The update after this one.
    #[must_use]
    pub fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }

    /// Whether this update came after `other`.
    pub fn is_after(self, other: Self) -> bool {
        self != other && self.0.wrapping_sub(other.0) < 1 << 31
    }

    /// How many updates came after `earlier` up to this one.
    pub fn since(self, earlier: Self) -> u32 {
        self.0.wrapping_sub(earlier.0)
    }
}

#[cfg(test)]
#[path = "tests/scene_records_tests.rs"]
mod tests;
