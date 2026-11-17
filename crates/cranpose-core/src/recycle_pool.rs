//! The recycled shells of one node type and what sizes their pool.

use std::collections::VecDeque;

use crate::{Node, NodeId, RecycleOrigin, RecycledNode};

// The recycled shells of one node type, and what sizes its pool: the shells
// warm from the last pass, the ones returned this pass by a node that came
// warm or cold, the limit its nodes ask for, the warm count it keeps, the
// shells this pass had to create fresh and a prototype to make more from.
// Each list keeps its oldest shell first and drops it first: a ring, so a
// full list drops one without moving the others.
#[derive(Default)]
pub(crate) struct RecyclePool {
    pub(crate) warm: VecDeque<RecycledNode>,
    pub(crate) returning: VecDeque<RecycledNode>,
    cold: VecDeque<RecycledNode>,
    pub(crate) limit: Option<usize>,
    warm_target: Option<usize>,
    pub(crate) fresh_creations: usize,
    prototype: Option<Box<dyn Node>>,
}

impl RecyclePool {
    pub(crate) fn len(&self) -> usize {
        self.warm.len() + self.returning.len() + self.cold.len()
    }

    pub(crate) fn capacity(&self) -> usize {
        self.warm.capacity() + self.returning.capacity() + self.cold.capacity()
    }

    pub(crate) fn nodes(&self) -> impl Iterator<Item = &RecycledNode> {
        self.warm.iter().chain(&self.returning).chain(&self.cold)
    }

    pub(crate) fn remember_prototype(&mut self, shell: &dyn Node) {
        if self.prototype.is_none() {
            self.prototype = shell.rehouse_for_recycle();
        }
    }

    pub(crate) fn push(&mut self, recycled: RecycledNode) {
        let pool = if recycled.origin() == RecycleOrigin::Warm {
            &mut self.returning
        } else {
            &mut self.cold
        };
        pool.push_back(recycled);
        drop_oldest_past(pool, self.limit);
    }

    pub(crate) fn push_warm(&mut self, mut recycled: RecycledNode) {
        recycled.set_origin(RecycleOrigin::Warm);
        self.warm.push_back(recycled);
        drop_oldest_past(&mut self.warm, self.limit);
    }

    pub(crate) fn take(&mut self) -> Option<RecycledNode> {
        self.returning.pop_back().or_else(|| self.warm.pop_back())
    }

    fn warm_target_limit(&self) -> usize {
        match self.limit {
            None => usize::MAX,
            Some(limit) if limit <= 8 => limit,
            Some(limit) => limit / 4,
        }
    }

    fn update_warm_target(&mut self, observed_demand: usize) -> usize {
        let target_limit = self.warm_target_limit();
        let existing = self.warm_target.unwrap_or(0).min(target_limit);
        if observed_demand == 0 {
            return existing;
        }
        let target = match self.limit {
            Some(limit) if limit > 8 => target_limit,
            Some(_) => observed_demand.min(target_limit),
            None => observed_demand,
        };
        self.warm_target = Some(target);
        target
    }

    // Seeds warm shells made from the prototype until the warm pool holds
    // `target`, each under a new stable id.
    fn replenish_warm(&mut self, target: usize, next_stable_id: &mut NodeId) {
        let missing = target.saturating_sub(self.warm.len());
        let Some(prototype) = &self.prototype else {
            return;
        };
        let mut shells = Vec::with_capacity(missing);
        for _ in 0..missing {
            let Some(shell) = prototype.rehouse_for_recycle() else {
                break;
            };
            shells.push(shell);
        }
        for shell in shells {
            if self.warm.len() >= self.limit.unwrap_or(usize::MAX) {
                return;
            }
            let stable_id = *next_stable_id;
            *next_stable_id = next_stable_id.next();
            self.push_warm(RecycledNode::from_shell(
                stable_id,
                shell,
                RecycleOrigin::Warm,
            ));
        }
    }

    // Each list keeps a buffer of up to 64 shells, or four times what it
    // holds, across passes; a larger one goes.
    fn compact_lists(&mut self) {
        for list in [&mut self.warm, &mut self.returning, &mut self.cold] {
            if list.capacity() > list.len().saturating_mul(4).max(64) {
                list.shrink_to_fit();
            }
        }
    }

    // At the end of an apply pass: the returned shells join the warm ones,
    // the cold ones this pass needed fresh shells for turn warm and the rest
    // go, and the warm pool moves to its target. The returned and cold lists
    // keep their buffers for the next pass.
    pub(crate) fn end_pass(&mut self, next_stable_id: &mut NodeId) {
        self.warm.append(&mut self.returning);
        let needed = std::mem::take(&mut self.fresh_creations);
        if needed > 0 {
            let remaining_limit = self
                .limit
                .unwrap_or(usize::MAX)
                .saturating_sub(self.warm.len());
            let promote = self.cold.len().min(needed).min(remaining_limit);
            let split_at = self.cold.len() - promote;
            for mut recycled in self.cold.drain(split_at..) {
                recycled.set_origin(RecycleOrigin::Warm);
                self.warm.push_back(recycled);
            }
        }
        self.cold.clear();
        // A type that only had shells created fresh has nothing to size yet.
        let nothing_to_size = self.warm.is_empty()
            && self.limit.is_none()
            && self.warm_target.is_none()
            && self.prototype.is_none();
        if !nothing_to_size {
            let target = self.update_warm_target(needed);
            self.replenish_warm(target, next_stable_id);
            drop_oldest_past(&mut self.warm, Some(target));
        }
        self.compact_lists();
    }
}

// Drops the oldest shells of `pool` past `limit`, oldest first.
fn drop_oldest_past(pool: &mut VecDeque<RecycledNode>, limit: Option<usize>) {
    if let Some(limit) = limit
        && pool.len() > limit
    {
        let excess = pool.len() - limit;
        pool.drain(..excess);
    }
}
