// Which of the 128 MIDI keys are down.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeySet(u128);

impl KeySet {
    pub const EMPTY: Self = Self(0);

    // Adds `key`; a number past the highest MIDI key adds nothing.
    pub fn insert(&mut self, key: u8) {
        if let Some(bit) = 1u128.checked_shl(u32::from(key)) {
            self.0 |= bit;
        }
    }

    pub fn contains(self, key: u8) -> bool {
        1u128
            .checked_shl(u32::from(key))
            .is_some_and(|bit| self.0 & bit != 0)
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn len(self) -> u32 {
        self.0.count_ones()
    }
}
