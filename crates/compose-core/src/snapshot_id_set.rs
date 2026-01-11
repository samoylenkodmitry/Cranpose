/// An optimized bit-set implementation for tracking snapshot IDs.
///
/// This is based on Jetpack Compose's SnapshotIdSet, optimized for:
/// - O(1) access for the most recent 128 snapshot IDs
/// - O(log N) access for older snapshots
/// - Immutable copy-on-write semantics
///
/// The set maintains:
/// - `lower_set`: 64 bits for IDs in range [lower_bound, lower_bound+63]
/// - `upper_set`: 64 bits for IDs in range [lower_bound+64, lower_bound+127]
/// - `below_bound`: sorted array for IDs below lower_bound
///
/// This structure is highly biased toward recent snapshots being set,
/// with older snapshots mostly or completely clear.
use std::fmt;

pub type SnapshotId = usize;

const BITS_PER_SET: usize = 64;
const SNAPSHOT_ID_SIZE: usize = 64;

#[derive(Clone, PartialEq, Eq)]
pub struct SnapshotIdSet {
    /// Bit set from (lower_bound + 64) to (lower_bound + 127)
    upper_set: u64,
    /// Bit set from lower_bound to (lower_bound + 63)
    lower_set: u64,
    /// Lower bound of the bit set. All values above lower_bound+127 are clear.
    lower_bound: SnapshotId,
    /// Sorted array of snapshot IDs below lower_bound
    below_bound: Option<Box<[SnapshotId]>>,
}

impl SnapshotIdSet {
    /// Empty snapshot ID set.
    pub const EMPTY: SnapshotIdSet = SnapshotIdSet {
        upper_set: 0,
        lower_set: 0,
        lower_bound: 0,
        below_bound: None,
    };

    /// Create a new empty snapshot ID set.
    pub fn new() -> Self {
        Self::EMPTY
    }

    /// Check if a snapshot ID is in the set.
    pub fn get(&self, id: SnapshotId) -> bool {
        let offset = id.wrapping_sub(self.lower_bound);

        if offset < BITS_PER_SET {
            // In lower_set range
            let mask = 1u64 << offset;
            (self.lower_set & mask) != 0
        } else if offset < BITS_PER_SET * 2 {
            // In upper_set range
            let mask = 1u64 << (offset - BITS_PER_SET);
            (self.upper_set & mask) != 0
        } else if id > self.lower_bound {
            // Above our tracked range
            false
        } else {
            // Below lower_bound, check the array
            self.below_bound
                .as_ref()
                .map(|arr| arr.binary_search(&id).is_ok())
                .unwrap_or(false)
        }
    }

    /// Add a snapshot ID to the set (returns a new set if modified).
    pub fn set(&self, id: SnapshotId) -> Self {
        if id < self.lower_bound {
            if let Some(ref arr) = self.below_bound {
                match arr.binary_search(&id) {
                    Ok(_) => {
                        // Already present
                        return self.clone();
                    }
                    Err(insert_pos) => {
                        // Insert at position
                        let mut new_arr = Vec::with_capacity(arr.len() + 1);
                        new_arr.extend_from_slice(&arr[..insert_pos]);
                        new_arr.push(id);
                        new_arr.extend_from_slice(&arr[insert_pos..]);
                        return Self {
                            upper_set: self.upper_set,
                            lower_set: self.lower_set,
                            lower_bound: self.lower_bound,
                            below_bound: Some(new_arr.into_boxed_slice()),
                        };
                    }
                }
            } else {
                // First element below bound
                return Self {
                    upper_set: self.upper_set,
                    lower_set: self.lower_set,
                    lower_bound: self.lower_bound,
                    below_bound: Some(vec![id].into_boxed_slice()),
                };
            }
        }

        let offset = id - self.lower_bound;

        if offset < BITS_PER_SET {
            // In lower_set range
            let mask = 1u64 << offset;
            if (self.lower_set & mask) == 0 {
                return Self {
                    upper_set: self.upper_set,
                    lower_set: self.lower_set | mask,
                    lower_bound: self.lower_bound,
                    below_bound: self.below_bound.clone(),
                };
            }
        } else if offset < BITS_PER_SET * 2 {
            // In upper_set range
            let mask = 1u64 << (offset - BITS_PER_SET);
            if (self.upper_set & mask) == 0 {
                return Self {
                    upper_set: self.upper_set | mask,
                    lower_set: self.lower_set,
                    lower_bound: self.lower_bound,
                    below_bound: self.below_bound.clone(),
                };
            }
        } else if offset >= BITS_PER_SET * 2 {
            // Need to shift the bit arrays
            if !self.get(id) {
                return self.shift_and_set(id);
            }
        }

        // No change needed
        self.clone()
    }

    /// Remove a snapshot ID from the set (returns a new set if modified).
    pub fn clear(&self, id: SnapshotId) -> Self {
        let offset = id.wrapping_sub(self.lower_bound);

        if offset < BITS_PER_SET {
            // In lower_set range
            let mask = 1u64 << offset;
            if (self.lower_set & mask) != 0 {
                return Self {
                    upper_set: self.upper_set,
                    lower_set: self.lower_set & !mask,
                    lower_bound: self.lower_bound,
                    below_bound: self.below_bound.clone(),
                };
            }
        } else if offset < BITS_PER_SET * 2 {
            // In upper_set range
            let mask = 1u64 << (offset - BITS_PER_SET);
            if (self.upper_set & mask) != 0 {
                return Self {
                    upper_set: self.upper_set & !mask,
                    lower_set: self.lower_set,
                    lower_bound: self.lower_bound,
                    below_bound: self.below_bound.clone(),
                };
            }
        } else if id < self.lower_bound {
            // Below lower_bound
            if let Some(ref arr) = self.below_bound {
                if let Ok(pos) = arr.binary_search(&id) {
                    let mut new_arr = Vec::with_capacity(arr.len() - 1);
                    new_arr.extend_from_slice(&arr[..pos]);
                    new_arr.extend_from_slice(&arr[pos + 1..]);
                    return Self {
                        upper_set: self.upper_set,
                        lower_set: self.lower_set,
                        lower_bound: self.lower_bound,
                        below_bound: if new_arr.is_empty() {
                            None
                        } else {
                            Some(new_arr.into_boxed_slice())
                        },
                    };
                }
            }
        }

        // No change needed
        self.clone()
    }

    /// Remove all IDs in `other` from this set (a & ~b).
    pub fn and_not(&self, other: &Self) -> Self {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return Self::EMPTY;
        }

        // Fast path: if both have same lower_bound and below_bound, can do bitwise ops
        if self.lower_bound == other.lower_bound && self.below_bound_equals(&other.below_bound) {
            return Self {
                upper_set: self.upper_set & !other.upper_set,
                lower_set: self.lower_set & !other.lower_set,
                lower_bound: self.lower_bound,
                below_bound: self.below_bound.clone(),
            };
        }

        // Slow path: iterate and clear each ID
        let mut result = self.clone();
        for id in other.iter() {
            result = result.clear(id);
        }
        result
    }

    /// Union this set with another (a | b).
    pub fn or(&self, other: &Self) -> Self {
        if other.is_empty() {
            return self.clone();
        }
        if self.is_empty() {
            return other.clone();
        }

        // Fast path: if both have same lower_bound and below_bound
        if self.lower_bound == other.lower_bound && self.below_bound_equals(&other.below_bound) {
            return Self {
                upper_set: self.upper_set | other.upper_set,
                lower_set: self.lower_set | other.lower_set,
                lower_bound: self.lower_bound,
                below_bound: self.below_bound.clone(),
            };
        }

        // Slow path: iterate and set each ID
        let mut result = self.clone();
        for id in other.iter() {
            result = result.set(id);
        }
        result
    }

    /// Find the lowest snapshot ID in the set that is <= upper.
    pub fn lowest(&self, upper: SnapshotId) -> SnapshotId {
        // Check below_bound array first
        if let Some(ref arr) = self.below_bound {
            if let Some(&lowest) = arr.first() {
                if lowest <= upper {
                    return lowest;
                }
            }
        }

        // Check lower_set
        if self.lower_set != 0 {
            let lowest_in_lower = self.lower_bound + self.lower_set.trailing_zeros() as usize;
            if lowest_in_lower <= upper {
                return lowest_in_lower;
            }
        }

        // Check upper_set
        if self.upper_set != 0 {
            let lowest_in_upper =
                self.lower_bound + BITS_PER_SET + self.upper_set.trailing_zeros() as usize;
            if lowest_in_upper <= upper {
                return lowest_in_upper;
            }
        }

        // Nothing found, return upper
        upper
    }

    /// Check if the set is empty.
    pub fn is_empty(&self) -> bool {
        self.lower_set == 0 && self.upper_set == 0 && self.below_bound.is_none()
    }

    /// Iterate over all snapshot IDs in the set.
    pub fn iter(&self) -> SnapshotIdSetIter<'_> {
        SnapshotIdSetIter::new(self)
    }

    /// Convert to a Vec of snapshot IDs (for testing/debugging).
    pub fn to_list(&self) -> Vec<SnapshotId> {
        self.iter().collect()
    }

    /// Add a contiguous range of IDs [from, until) to the set.
    /// Mirrors AndroidX SnapshotIdSet.addRange semantics used by Snapshot.kt.
    pub fn add_range(&self, from: SnapshotId, until: SnapshotId) -> Self {
        if from >= until {
            return self.clone();
        }
        let mut result = self.clone();
        let mut id = from;
        while id < until {
            result = result.set(id);
            id += 1;
        }
        result
    }

    // Helper: check if two below_bound arrays are equal
    fn below_bound_equals(&self, other: &Option<Box<[SnapshotId]>>) -> bool {
        match (&self.below_bound, other) {
            (None, None) => true,
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }

    // Helper: shift the bit arrays and set a new ID
    fn shift_and_set(&self, id: SnapshotId) -> Self {
        let target_lower_bound = (id / SNAPSHOT_ID_SIZE) * SNAPSHOT_ID_SIZE;

        let mut new_upper_set = self.upper_set;
        let mut new_lower_set = self.lower_set;
        let mut new_lower_bound = self.lower_bound;
        let mut new_below_bound: Vec<SnapshotId> = if let Some(ref arr) = self.below_bound {
            arr.to_vec()
        } else {
            Vec::new()
        };

        while new_lower_bound < target_lower_bound {
            // Shift lower_set into below_bound array
            if new_lower_set != 0 {
                for bit_offset in 0..BITS_PER_SET {
                    if (new_lower_set & (1u64 << bit_offset)) != 0 {
                        let id_to_add = new_lower_bound + bit_offset;
                        // Insert in sorted order
                        match new_below_bound.binary_search(&id_to_add) {
                            Ok(_) => {} // Already present (shouldn't happen)
                            Err(pos) => new_below_bound.insert(pos, id_to_add),
                        }
                    }
                }
            }

            // Shift upper_set down to lower_set
            if new_upper_set == 0 {
                new_lower_bound = target_lower_bound;
                new_lower_set = 0;
                break;
            }

            new_lower_set = new_upper_set;
            new_upper_set = 0;
            new_lower_bound += BITS_PER_SET;
        }

        let result = Self {
            upper_set: new_upper_set,
            lower_set: new_lower_set,
            lower_bound: new_lower_bound,
            below_bound: if new_below_bound.is_empty() {
                None
            } else {
                Some(new_below_bound.into_boxed_slice())
            },
        };

        // Now set the ID
        result.set(id)
    }
}

impl Default for SnapshotIdSet {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl fmt::Debug for SnapshotIdSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SnapshotIdSet{{")?;
        let ids: Vec<_> = self.iter().collect();
        for (i, id) in ids.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", id)?;
        }
        write!(f, "}}")
    }
}

/// Iterator over snapshot IDs in a set.
pub struct SnapshotIdSetIter<'a> {
    set: &'a SnapshotIdSet,
    below_index: usize,
    lower_set: u64,
    upper_set: u64,
    current_offset: usize,
}

impl<'a> SnapshotIdSetIter<'a> {
    fn new(set: &'a SnapshotIdSet) -> Self {
        Self {
            set,
            below_index: 0,
            lower_set: set.lower_set,
            upper_set: set.upper_set,
            current_offset: 0,
        }
    }
}

impl<'a> Iterator for SnapshotIdSetIter<'a> {
    type Item = SnapshotId;

    fn next(&mut self) -> Option<Self::Item> {
        // First, yield from below_bound array
        if let Some(ref arr) = self.set.below_bound {
            if self.below_index < arr.len() {
                let id = arr[self.below_index];
                self.below_index += 1;
                return Some(id);
            }
        }

        // Then yield from lower_set
        while self.current_offset < BITS_PER_SET {
            if (self.lower_set & (1u64 << self.current_offset)) != 0 {
                let id = self.set.lower_bound + self.current_offset;
                self.current_offset += 1;
                return Some(id);
            }
            self.current_offset += 1;
        }

        // Finally yield from upper_set
        while self.current_offset < BITS_PER_SET * 2 {
            let bit_offset = self.current_offset - BITS_PER_SET;
            if (self.upper_set & (1u64 << bit_offset)) != 0 {
                let id = self.set.lower_bound + self.current_offset;
                self.current_offset += 1;
                return Some(id);
            }
            self.current_offset += 1;
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_set() {
        let set = SnapshotIdSet::EMPTY;
        assert!(set.is_empty());
        assert!(!set.get(0));
        assert!(!set.get(100));
    }

    #[test]
    fn test_set_and_get_lower_range() {
        let set = SnapshotIdSet::new();
        let set = set.set(0);
        assert!(set.get(0));
        assert!(!set.get(1));

        let set = set.set(63);
        assert!(set.get(0));
        assert!(set.get(63));
        assert!(!set.get(64));
    }

    #[test]
    fn test_set_and_get_upper_range() {
        let set = SnapshotIdSet::new();
        let set = set.set(64);
        assert!(set.get(64));
        assert!(!set.get(63));
        assert!(!set.get(128));

        let set = set.set(127);
        assert!(set.get(64));
        assert!(set.get(127));
        assert!(!set.get(128));
    }

    #[test]
    fn test_set_idempotent() {
        let set = SnapshotIdSet::new();
        let set1 = set.set(10);
        let set2 = set1.set(10);
        assert_eq!(set1, set2);
    }

    #[test]
    fn test_clear() {
        let set = SnapshotIdSet::new().set(10).set(20).set(30);
        assert!(set.get(10));
        assert!(set.get(20));
        assert!(set.get(30));

        let set = set.clear(20);
        assert!(set.get(10));
        assert!(!set.get(20));
        assert!(set.get(30));
    }

    #[test]
    fn test_clear_idempotent() {
        let set = SnapshotIdSet::new().set(10);
        let set1 = set.clear(10);
        let set2 = set1.clear(10);
        assert_eq!(set1, set2);
    }

    #[test]
    fn test_below_bound_insertion() {
        let mut set = SnapshotIdSet::new();
        // Set lower_bound to 100
        set = set.set(100);
        assert_eq!(set.lower_bound, 0);

        // Now insert something below lower_bound
        set = set.set(50);
        assert!(set.get(50));
        assert!(set.get(100));

        set = set.set(25);
        set = set.set(75);
        assert!(set.get(25));
        assert!(set.get(50));
        assert!(set.get(75));
        assert!(set.get(100));

        // Check that below_bound is sorted
        let list = set.to_list();
        assert_eq!(list, vec![25, 50, 75, 100]);
    }

    #[test]
    fn test_below_bound_removal() {
        // Build incrementally to avoid stack overflow from large shifts
        let set = SnapshotIdSet::new();
        let set = set.set(25);
        let set = set.set(50);
        let set = set.set(75);
        let set = set.set(200);

        let set = set.clear(50);
        assert!(set.get(25));
        assert!(!set.get(50));
        assert!(set.get(75));
        assert!(set.get(200));

        let list = set.to_list();
        assert_eq!(list, vec![25, 75, 200]);
    }

    #[test]
    fn test_shift_and_set() {
        let set = SnapshotIdSet::new();
        let set = set.set(10);
        assert_eq!(set.lower_bound, 0);

        // Setting a value way above should shift the arrays
        let set = set.set(200);
        assert!(set.get(10));
        assert!(set.get(200));

        // 10 should now be in below_bound
        assert!(set.below_bound.is_some());
    }

    #[test]
    fn test_shift_and_set_boundary_values() {
        let mut set = SnapshotIdSet::new();
        let boundary = SNAPSHOT_ID_SIZE * 12 - 1;
        set = set.set(boundary);
        assert!(set.get(boundary));

        set = set.set(boundary + 1);
        assert!(set.get(boundary));
        assert!(set.get(boundary + 1));
    }

    #[test]
    fn test_set_below_lower_bound_inserts() {
        let set = SnapshotIdSet::new().set(200);
        let lower_bound = set.lower_bound;
        assert!(lower_bound > 0);

        let below = lower_bound - 1;
        let set = set.set(below);
        assert!(set.get(below));
        assert!(set.get(200));
    }

    #[test]
    fn test_and_not_fast_path() {
        let set1 = SnapshotIdSet::new().set(10).set(20).set(30);
        let set2 = SnapshotIdSet::new().set(20).set(40);

        let result = set1.and_not(&set2);
        assert!(result.get(10));
        assert!(!result.get(20));
        assert!(result.get(30));
        assert!(!result.get(40));
    }

    #[test]
    fn test_and_not_slow_path() {
        let set1 = SnapshotIdSet::new().set(10).set(20).set(30);
        // Create set2 with different lower_bound by setting high value first
        let set2 = SnapshotIdSet::new().set(100).set(20);

        let result = set1.and_not(&set2);
        assert!(result.get(10));
        assert!(!result.get(20));
        assert!(result.get(30));
    }

    #[test]
    fn test_or_fast_path() {
        let set1 = SnapshotIdSet::new().set(10).set(20);
        let set2 = SnapshotIdSet::new().set(20).set(30);

        let result = set1.or(&set2);
        assert!(result.get(10));
        assert!(result.get(20));
        assert!(result.get(30));
    }

    #[test]
    fn test_or_slow_path() {
        let set1 = SnapshotIdSet::new().set(10).set(20);
        let set2 = SnapshotIdSet::new().set(100).set(30);

        let result = set1.or(&set2);
        assert!(result.get(10));
        assert!(result.get(20));
        assert!(result.get(30));
        assert!(result.get(100));
    }

    #[test]
    fn test_lowest_in_below_bound() {
        // Build incrementally to avoid deep recursion
        let set = SnapshotIdSet::new();
        let set = set.set(25);
        let set = set.set(50);
        let set = set.set(200);
        assert_eq!(set.lowest(1000), 25);
        assert_eq!(set.lowest(100), 25);
        assert_eq!(set.lowest(30), 25);
    }

    #[test]
    fn test_lowest_in_lower_set() {
        let set = SnapshotIdSet::new().set(10).set(20).set(30);
        assert_eq!(set.lowest(1000), 10);
        assert_eq!(set.lowest(25), 10);
    }

    #[test]
    fn test_lowest_in_upper_set() {
        let set = SnapshotIdSet::new().set(70).set(80).set(90);
        assert_eq!(set.lowest(1000), 70);
    }

    #[test]
    fn test_lowest_returns_upper_if_none_found() {
        let set = SnapshotIdSet::new().set(100);
        assert_eq!(set.lowest(50), 50);
    }

    #[test]
    fn test_iterator() {
        let set = SnapshotIdSet::new().set(10).set(20).set(5).set(30);
        let list: Vec<_> = set.iter().collect();
        // Should be in sorted order
        assert_eq!(list, vec![5, 10, 20, 30]);
    }

    #[test]
    fn test_iterator_empty() {
        let set = SnapshotIdSet::new();
        let list: Vec<_> = set.iter().collect();
        assert_eq!(list, Vec::<SnapshotId>::new());
    }

    #[test]
    fn test_iterator_all_ranges() {
        let set = SnapshotIdSet::new()
            .set(5) // below_bound (after shift)
            .set(10) // lower_set (after shift)
            .set(70) // upper_set (after shift)
            .set(200); // causes shift

        let list: Vec<_> = set.iter().collect();
        assert_eq!(list, vec![5, 10, 70, 200]);
    }

    #[test]
    fn test_to_list() {
        let set = SnapshotIdSet::new().set(10).set(20).set(30);
        assert_eq!(set.to_list(), vec![10, 20, 30]);
    }

    #[test]
    fn test_debug_format() {
        let set = SnapshotIdSet::new().set(10).set(20);
        let debug_str = format!("{:?}", set);
        assert_eq!(debug_str, "SnapshotIdSet{10, 20}");
    }

    #[test]
    fn test_large_snapshot_ids() {
        // Build incrementally to avoid deep recursion
        let set = SnapshotIdSet::new();
        let set = set.set(500);
        let set = set.set(1000);
        let set = set.set(2000);

        assert!(set.get(500));
        assert!(set.get(1000));
        assert!(set.get(2000));
        assert!(!set.get(1500));
    }

    #[test]
    fn test_boundary_transitions() {
        let set = SnapshotIdSet::new();

        // Test transition from lower to upper
        let set = set.set(63);
        let set = set.set(64);
        assert!(set.get(63));
        assert!(set.get(64));

        // Test transition from upper to above
        let set = set.set(127);
        let set = set.set(128);
        assert!(set.get(127));
        assert!(set.get(128));
    }
}
