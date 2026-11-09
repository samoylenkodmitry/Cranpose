// Whether an update put the children a record keeps in another order. A
// builder may empty a record's items and fill them again within an update,
// so the first change of a record's items in an update notes its children,
// and the end of the update compares them with the children it holds then.

use cranpose_core::{RecordId, RecordRef};

use super::{Item, RecordEvent, SceneStore};

#[derive(Clone, Default)]
pub(crate) struct ChildOrder {
    // By row, one past the round of notes that noted the record's children:
    // a round lasts an update.
    noted: Vec<u64>,
    round: u64,
    // The records the update noted, each with the span of `before` that
    // holds its children before the update.
    records: Vec<(RecordRef, usize, usize)>,
    before: Vec<RecordId>,
    stamps: Stamps,
}

// By row, the comparison that last found the row among a record's new
// children, and where among them.
#[derive(Clone, Default)]
struct Stamps {
    rows: Vec<(u64, usize)>,
    stamp: u64,
}

impl ChildOrder {
    // Notes the children among `items`, the items of `record` before the
    // first change of them in the update in progress.
    pub(super) fn note(&mut self, record: RecordRef, items: &[Item]) {
        let row = record.id.index();
        if row >= self.noted.len() {
            self.noted.resize(row + 1, 0);
        }
        let mark = self.round + 1;
        if self.noted[row] == mark {
            return;
        }
        self.noted[row] = mark;
        let start = self.before.len();
        self.before
            .extend(items.iter().filter_map(|item| item.as_child()));
        // A record that kept no children cannot put them in another order.
        if self.before.len() > start {
            self.records.push((record, start, self.before.len()));
        }
    }
}

// Logs as changed each record the update noted whose kept children it put
// in another order, and forgets the notes.
pub(super) fn settle(store: &mut SceneStore) {
    // Most updates note no record's children: each round still ends.
    if store.order.records.is_empty() {
        store.order.round += 1;
        return;
    }
    let mut order = std::mem::take(&mut store.order);
    for &(record, start, end) in &order.records {
        if let Some(id) = store.resolve(record)
            && order
                .stamps
                .reorders(&order.before[start..end], store.items(id))
        {
            store.log(RecordEvent::Changed, id);
        }
    }
    order.records.clear();
    order.before.clear();
    order.round += 1;
    store.order = order;
}

impl Stamps {
    // Whether `new` holds the children of `old` it keeps in another order.
    fn reorders(&mut self, old: &[RecordId], new: &[Item]) -> bool {
        // Most often the record keeps the same children in the same order.
        let mut new_children = new.iter().filter_map(|item| item.as_child());
        let mut old_children = old.iter().copied();
        loop {
            match (old_children.next(), new_children.next()) {
                (Some(old), Some(new)) if old == new => {}
                (None, None) => return false,
                _ => break,
            }
        }
        self.reorders_after_a_change(old, new)
    }

    // Whether `new` holds the children of `old` it keeps in another order,
    // once their lists differ. Out of line: most updates keep a record's
    // children as they were.
    #[cold]
    #[inline(never)]
    fn reorders_after_a_change(&mut self, old: &[RecordId], new: &[Item]) -> bool {
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
        for child in old {
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
