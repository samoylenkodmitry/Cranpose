//! The lists a scene keeps from one update to the next for the work it does
//! again. Most hold one or two entries, so a full list doubles its room
//! from one entry, with no room for four first.

// Pushes `entry` onto `list`, doubling its room from one entry when it is
// full.
pub(crate) fn push_kept<T>(list: &mut Vec<T>, entry: T) {
    if list.len() == list.capacity() {
        list.reserve_exact(list.capacity().max(1));
    }
    list.push(entry);
}
