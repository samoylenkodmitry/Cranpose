//! The recycled shells of one node type and what sizes their pool.

use crate::{Node, NodeId, RecycledNode};

// The recycled shells of one node type, and what sizes its pool: the shells
// warm from the last pass, the ones returned this pass by a node that came
// warm or cold, the limit its nodes ask for, the warm count it keeps, the
// shells this pass had to create fresh and a prototype to make more from.
#[derive(Default)]
pub(crate) struct RecyclePool {
    pub(crate) warm: Vec<RecycledNode>,
    pub(crate) returning: Vec<RecycledNode>,
    cold: Vec<RecycledNode>,
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
        let pool = if recycled.warm_origin() {
            &mut self.returning
        } else {
            &mut self.cold
        };
        pool.push(recycled);
        drop_oldest_past(pool, self.limit);
    }

    pub(crate) fn push_warm(&mut self, mut recycled: RecycledNode) {
        recycled.set_warm_origin(true);
        self.warm.push(recycled);
        drop_oldest_past(&mut self.warm, self.limit);
    }

    pub(crate) fn take(&mut self) -> Option<RecycledNode> {
        self.returning.pop().or_else(|| self.warm.pop())
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
            *next_stable_id = next_stable_id.saturating_add(1);
            self.push_warm(RecycledNode::from_shell(stable_id, shell, true));
        }
    }

    fn trim_warm_to(&mut self, target: usize) {
        let removable = self.warm.len().saturating_sub(target);
        let dropped: Vec<_> = self.warm.drain(0..removable).collect();
        drop(dropped);
    }

    fn compact_warm(&mut self) {
        if self.warm.capacity() <= self.warm.len().saturating_mul(4).max(64) {
            return;
        }
        let mut compacted = Vec::with_capacity(self.warm.len());
        compacted.append(&mut self.warm);
        self.warm = compacted;
    }

    // At the end of an apply pass: the returned shells join the warm ones,
    // the cold ones this pass needed fresh shells for turn warm and the rest
    // go, and the warm pool moves to its target.
    pub(crate) fn end_pass(&mut self, next_stable_id: &mut NodeId) {
        self.warm.append(&mut std::mem::take(&mut self.returning));
        let needed = std::mem::take(&mut self.fresh_creations);
        let mut cold = std::mem::take(&mut self.cold);
        if needed > 0 {
            let remaining_limit = self
                .limit
                .unwrap_or(usize::MAX)
                .saturating_sub(self.warm.len());
            let promote = cold.len().min(needed).min(remaining_limit);
            let split_at = cold.len().saturating_sub(promote);
            for mut recycled in cold.split_off(split_at) {
                recycled.set_warm_origin(true);
                self.warm.push(recycled);
            }
        }
        drop(cold);
        // A type that only had shells created fresh has nothing to size yet.
        if self.warm.is_empty()
            && self.limit.is_none()
            && self.warm_target.is_none()
            && self.prototype.is_none()
        {
            return;
        }
        let target = self.update_warm_target(needed);
        self.replenish_warm(target, next_stable_id);
        self.trim_warm_to(target);
        self.compact_warm();
    }
}

// Drops the oldest shells of `pool` past `limit`.
fn drop_oldest_past(pool: &mut Vec<RecycledNode>, limit: Option<usize>) {
    if let Some(limit) = limit
        && pool.len() > limit
    {
        let excess = pool.len() - limit;
        let dropped: Vec<_> = pool.drain(0..excess).collect();
        drop(dropped);
    }
}
