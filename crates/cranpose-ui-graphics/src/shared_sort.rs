//! One sort routine for the sorts off a frame's path. Each call of a
//! standard library sort takes its own copy of the routine for its item type
//! and its comparison, 4 to 11 KB of code on an ESP32; the callers of this
//! module share one copy. A comparison costs an indirect call more.

use std::cmp::Ordering;

/// Sorts `positions`, positions in a list the caller compares, by `compare`,
/// and positions that compare equal by their own values: positions handed
/// over in ascending order keep equal items in their order, as
/// [`slice::sort_by`] does. Every caller shares one sort routine, which
/// allocates nothing.
pub fn sort_positions(positions: &mut [usize], compare: &mut dyn FnMut(usize, usize) -> Ordering) {
    positions.sort_unstable_by(|&a, &b| compare(a, b).then(a.cmp(&b)));
}

// The longest list `sort_by` sorts in place, with no list of positions.
const SORTED_IN_PLACE: usize = 20;

/// Sorts `items` by `compare` and keeps equal items in their order, as
/// [`slice::sort_by`] does. A list of up to 20 items sorts in place; a
/// longer one sorts through [`sort_positions`] and allocates a list of
/// `items.len()` positions.
pub fn sort_by<T>(items: &mut [T], mut compare: impl FnMut(&T, &T) -> Ordering) {
    if items.len() <= SORTED_IN_PLACE {
        insert_in_order(items, &mut compare);
        return;
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    sort_positions(&mut order, &mut |a, b| compare(&items[a], &items[b]));
    place_in_order(items, &mut order);
}

// Moves each item back past the items before it that compare greater.
fn insert_in_order<T>(items: &mut [T], compare: &mut impl FnMut(&T, &T) -> Ordering) {
    for end in 1..items.len() {
        let mut at = end;
        while at > 0 && compare(&items[at - 1], &items[at]) == Ordering::Greater {
            items.swap(at - 1, at);
            at -= 1;
        }
    }
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
