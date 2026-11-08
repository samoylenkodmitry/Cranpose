// Whether a record's new items keep its children in the order it holds
// them, found in one pass over each list without allocating.

use super::Item;

// By row, the call that last put the row among a record's new items, and
// where among them.
#[derive(Clone, Default)]
pub(crate) struct ChildOrder {
    rows: Vec<(u64, usize)>,
    stamp: u64,
}

impl ChildOrder {
    // Whether `new`, as the new items of a record that holds `old`, holds
    // the children it keeps in another order.
    pub(super) fn reorders(&mut self, old: &[Item], new: &[Item]) -> bool {
        self.stamp += 1;
        let stamp = self.stamp;
        for (position, item) in new.iter().enumerate() {
            if let Some(child) = item.as_child() {
                let row = child.index();
                if row >= self.rows.len() {
                    self.rows.resize(row + 1, (0, 0));
                }
                self.rows[row] = (stamp, position);
            }
        }
        let mut last = None;
        for child in old.iter().filter_map(|item| item.as_child()) {
            let Some(&(seen, position)) = self.rows.get(child.index()) else {
                continue;
            };
            if seen != stamp {
                continue;
            }
            if last.is_some_and(|last| position < last) {
                return true;
            }
            last = Some(position);
        }
        false
    }
}
