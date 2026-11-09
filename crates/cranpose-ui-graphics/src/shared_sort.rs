//! One sort routine for the sorts off a frame's path. Each call of a
//! standard library sort takes its own copy of the routine for its item type
//! and its comparison, 4 to 11 KB of code on an ESP32; the callers of this
//! module share one copy. A comparison costs an indirect call more.

use std::cmp::Ordering;

/// Sorts `positions`, positions in a list the caller compares, by `compare`
/// and keeps equal positions in their order, as [`slice::sort_by`] does.
/// Every caller shares one sort routine. It allocates nothing for up to
/// 1024 positions.
pub fn sort_positions(positions: &mut [usize], compare: &mut dyn FnMut(usize, usize) -> Ordering) {
    positions.sort_by(|&a, &b| compare(a, b));
}

/// Sorts `items` by `compare` and keeps equal items in their order, as
/// [`slice::sort_by`] does, through [`sort_positions`]. It allocates a list
/// of `items.len()` positions.
pub fn sort_by<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
    if items.len() < 2 {
        return;
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    sort_positions(&mut order, &mut |a, b| compare(&items[a], &items[b]));
    place_in_order(items, &mut order);
}

// Moves the item at `order[place]` to `place` for every place, following
// each cycle of the order once; marks each place done in `order`.
fn place_in_order<T>(items: &mut [T], order: &mut [usize]) {
    for start in 0..order.len() {
        let mut place = start;
        loop {
            let source = order[place];
            order[place] = place;
            if source == start {
                break;
            }
            items.swap(place, source);
            place = source;
        }
    }
}
