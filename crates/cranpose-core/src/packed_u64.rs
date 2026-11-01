//! A 64-bit number kept at the alignment of a 32-bit word.

use std::fmt;

/// A 64-bit number kept at the alignment of a 32-bit word on ESP-IDF. There a
/// `u64` takes 8-byte alignment, and so would every record that holds one:
/// padding around the field, and on a heap that hands out 4-byte aligned
/// blocks, a block 8 bytes longer. Elsewhere it is a `u64`, which the
/// compiler reads and compares without copying it out of a packed field.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(target_os = "espidf", repr(C, packed(4)))]
#[cfg_attr(not(target_os = "espidf"), repr(transparent))]
pub struct PackedU64(u64);

impl PackedU64 {
    /// `value`, kept at a word's alignment.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl From<u64> for PackedU64 {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl fmt::Debug for PackedU64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.get().fmt(f)
    }
}
