// A group that did not exist enters the table before other groups: every
// record its body inserts moves every record after it. Once the body has
// inserted `PARK_AFTER_INSERTS` records, the records after the new subtree
// wait aside, each later record lands at the end of the table's vectors, and
// the end of the new group puts the waiting records back with one pass over
// their groups.

use super::{
    super::{
        CheckedU32Delta, GroupRecord, NodeRecord, checked_u32_delta, checked_usize_to_i64,
        payload_store::ParkedOrder,
    },
    SlotTable,
    metadata::StaleGroupIndexes,
};
use crate::AnchorId;

// Parking the records after a new subtree and putting them back costs about
// as much as ten inserts that move them, on a table that fits in the caches.
// A subtree with no more inserts than this, such as a typical list row,
// keeps them in place and pays nothing more than before.
const PARK_AFTER_INSERTS: u32 = 16;

// The writer ends the run when it closes the root's frame.
pub(in crate::slot) struct InsertRun {
    root: AnchorId,
    root_index: usize,
    tail: RunTail,
}

// The records after the new subtree.
enum RunTail {
    // In the table, moved by each insert.
    InPlace { inserts: u32 },
    Parked(ParkedTail),
}

struct ParkedTail {
    groups: Vec<GroupRecord>,
    nodes: Vec<NodeRecord>,
    payload_order: ParkedOrder,
    // Where the parked groups, nodes and payloads began in the table.
    group_start: usize,
    node_start: usize,
    payload_start: usize,
    // How far the parked groups' anchors could trail them when they left.
    stale: StaleGroupIndexes,
}

impl SlotTable {
    // Starts an insert run for the group the writer inserted at
    // `root_index`, when groups follow it and no run is open. `false` when
    // it started none.
    #[inline]
    pub(in crate::slot) fn open_insert_run(&mut self, root_index: usize) -> bool {
        if self.insert_run.is_some() || root_index + 1 >= self.groups.len() {
            return false;
        }
        self.insert_run = Some(InsertRun {
            root: self.groups[root_index].anchor,
            root_index,
            tail: RunTail::InPlace { inserts: 0 },
        });
        true
    }

    // Notes a record about to enter the table: the records after an open
    // run's subtree leave once its inserts reach `PARK_AFTER_INSERTS`.
    #[inline]
    pub(in crate::slot) fn note_run_insert(&mut self) {
        if let Some(InsertRun {
            tail: RunTail::InPlace { inserts },
            ..
        }) = &mut self.insert_run
        {
            *inserts += 1;
            if *inserts >= PARK_AFTER_INSERTS {
                self.park_run_tail();
            }
        }
    }

    // Ends the open run: the parked records come back after the run's
    // records, which leaves the table whole for any reader.
    pub(crate) fn settle_insert_run(&mut self) {
        if let Some(InsertRun {
            tail: RunTail::Parked(tail),
            ..
        }) = self.insert_run.take()
        {
            self.unpark_run_tail(tail);
        }
    }

    #[cfg(any(test, debug_assertions))]
    pub(in crate::slot) fn insert_run_is_parked(&self) -> bool {
        matches!(
            self.insert_run,
            Some(InsertRun {
                tail: RunTail::Parked(_),
                ..
            })
        )
    }

    #[cold]
    #[inline(never)]
    fn park_run_tail(&mut self) {
        let Some(run) = &self.insert_run else {
            return;
        };
        let tail_start = self
            .groups
            .get(run.root_index)
            .filter(|root| root.anchor == run.root)
            .map(|root| run.root_index + root.subtree_len as usize);
        let Some((tail_start, node_start, payload_start)) = tail_start
            .and_then(|start| Some((start, self.groups.get(start)?)))
            .map(|(start, first)| {
                (
                    start,
                    first.node_start as usize,
                    first.payload_start as usize,
                )
            })
            .filter(|&(_, nodes, payloads)| {
                nodes <= self.nodes.len() && payloads <= self.payloads.len()
            })
        else {
            // Nothing follows the subtree, or its root left its place.
            self.insert_run = None;
            return;
        };
        let parked = ParkedTail {
            groups: self.groups.split_off(tail_start),
            nodes: self.nodes.split_off(node_start),
            payload_order: self.payloads.split_off_order(payload_start),
            group_start: tail_start,
            node_start,
            payload_start,
            stale: self.stale_group_indexes(),
        };
        if let Some(run) = &mut self.insert_run {
            run.tail = RunTail::Parked(parked);
        }
    }

    // Puts the parked records back after the run's records and moves their
    // starts past them, in one pass over the returned groups. Their anchors
    // trail them by the groups the run added, as after that many inserts;
    // past the bound on that, the same pass writes them.
    #[cold]
    #[inline(never)]
    fn unpark_run_tail(&mut self, mut tail: ParkedTail) {
        let base = self.groups.len();
        let write_indexes =
            !self.note_groups_returned(tail.stale, tail.group_start, base - tail.group_start);
        if write_indexes {
            // Only the run's groups hold stale indexes now: write them before
            // the parked groups return, so no later flush walks those again.
            self.flush_stale_group_indexes();
        }
        let node_delta = start_delta(self.nodes.len(), tail.node_start, "node segment start");
        let payload_delta = start_delta(
            self.payloads.len(),
            tail.payload_start,
            "payload segment start",
        );
        self.groups.append(&mut tail.groups);
        self.nodes.append(&mut tail.nodes);
        self.payloads.append_order(tail.payload_order);
        for (index, group) in (base..).zip(&mut self.groups[base..]) {
            group.node_start =
                checked_u32_delta(group.node_start, node_delta, 0, "node segment start");
            group.payload_start = checked_u32_delta(
                group.payload_start,
                payload_delta,
                0,
                "payload segment start",
            );
            if write_indexes {
                self.anchors.move_active(group.anchor, index);
            }
        }
        if write_indexes {
            self.diagnostics
                .record_group_index_refresh(self.groups.len() - base);
        }
    }
}

// How far the parked records' starts move: from where they began to the end
// of the run's records.
fn start_delta(run_end: usize, parked_start: usize, field: &'static str) -> CheckedU32Delta {
    CheckedU32Delta::from_i64(
        checked_usize_to_i64(run_end, field) - checked_usize_to_i64(parked_start, field),
        field,
    )
}
