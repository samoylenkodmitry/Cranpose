//! The recycled shells of one node type and what sizes their pool.

use std::collections::VecDeque;

use crate::{Node, NodeId, RecycleOrigin, RecycledNode, collections::pass_aged::IDLE_PASSES};

// The most shells a pool with no limit makes ahead of demand.
const MADE_AHEAD: usize = 32;

// The recycled shells of one node type, and what sizes its pool: the shells
// warm from the last pass, the ones returned this pass by a node that came
// warm or cold, the limit its nodes ask for, the warm count it keeps, the
// shells this pass had to create fresh, what its recent frames asked for
// and a prototype to make more from. Each list keeps its oldest shell first
// and drops it first: a ring, so a full list drops one without moving the
// others.
#[derive(Default)]
pub(crate) struct RecyclePool {
    pub(crate) warm: VecDeque<RecycledNode>,
    pub(crate) returning: VecDeque<RecycledNode>,
    cold: VecDeque<RecycledNode>,
    pub(crate) limit: Option<usize>,
    warm_target: Option<usize>,
    fresh_creations: usize,
    demand: RecentDemand,
    prototype: Option<Box<dyn Node>>,
}

// The most nodes of the type one frame asked for, over the current span of
// `IDLE_PASSES` frames and the span before: the shells the pool keeps. A
// burst that recurs within two spans finds its shells; a pool whose frames
// ask for none for two spans keeps none.
#[derive(Default)]
struct RecentDemand {
    // The nodes this frame asked for: the shells it took and the nodes it
    // made fresh.
    frame: usize,
    // The most of a frame in this span, and in the span before.
    span: usize,
    span_before: usize,
    // The frames since this span began.
    frames: u64,
}

impl RecentDemand {
    fn bound(&self) -> usize {
        self.frame.max(self.span).max(self.span_before)
    }

    fn end_frame(&mut self) {
        self.span = self.span.max(std::mem::take(&mut self.frame));
        self.frames += 1;
        if self.frames >= IDLE_PASSES {
            self.frames = 0;
            self.span_before = std::mem::take(&mut self.span);
        }
    }
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

    // The shell that came back last, then the warm ones, then the ones of
    // nodes made fresh. The pool's pass ends only at a compaction of the
    // applier, after a pass removed many nodes: until then a removed node
    // made fresh waits in the cold list.
    pub(crate) fn take(&mut self) -> Option<RecycledNode> {
        let taken = self
            .returning
            .pop_back()
            .or_else(|| self.warm.pop_back())
            .or_else(|| self.cold.pop_back());
        if taken.is_some() {
            self.demand.frame += 1;
        }
        taken
    }

    // A node of the type was made fresh: the pool had no shell for it.
    pub(crate) fn note_fresh_creation(&mut self) {
        self.fresh_creations += 1;
        self.demand.frame += 1;
    }

    // The most shells the pool makes ahead of demand.
    pub(crate) fn made_ahead_limit(limit: Option<usize>) -> usize {
        limit.unwrap_or(MADE_AHEAD)
    }

    fn warm_target_limit(&self) -> usize {
        match self.limit {
            None => MADE_AHEAD,
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
            Some(limit) if limit <= 8 => observed_demand.min(target_limit),
            _ => target_limit,
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

    // Gives back the room of a list that holds far fewer shells than its
    // room and than the pool keeps: the room a burst took.
    fn compact(&mut self, kept: usize) {
        for list in [&mut self.warm, &mut self.returning, &mut self.cold] {
            if list.capacity() > list.len().saturating_mul(4).max(64).max(kept) {
                list.shrink_to_fit();
            }
        }
    }

    // The shells the pool keeps: as many as its recent frames asked for,
    // with the warm count it keeps while they ask for any, within its limit.
    fn kept(&self) -> usize {
        let demand = self.demand.bound();
        let kept = if demand == 0 {
            0
        } else {
            demand.max(self.warm_target.unwrap_or(0))
        };
        kept.min(self.limit.unwrap_or(usize::MAX))
    }

    // Lets the oldest shells go until the pool holds `kept`: the shells of
    // nodes made fresh first, then the returned ones, then the warm ones.
    fn give_back_past(&mut self, kept: usize) {
        let mut excess = self.len().saturating_sub(kept);
        for list in [&mut self.cold, &mut self.returning, &mut self.warm] {
            let dropped = excess.min(list.len());
            drop_oldest(list, list.len() - dropped);
            excess -= dropped;
        }
    }

    // A frame ends: the pool keeps as many shells as its recent frames
    // asked for and lets the oldest of the rest go.
    pub(crate) fn end_frame(&mut self) {
        self.demand.end_frame();
        let kept = self.kept();
        self.give_back_past(kept);
        self.compact(kept);
    }

    // The tree rests: the pool lets every shell, its prototype and the room
    // of its lists go, and forgets what its frames asked for.
    pub(crate) fn rest(&mut self) {
        *self = Self {
            limit: self.limit,
            ..Self::default()
        };
    }

    // At the end of an apply pass that compacted the applier: the returned
    // shells and those of nodes made fresh join the warm ones, the warm pool
    // grows to its target, and the pool keeps as many shells as its recent
    // frames asked for.
    pub(crate) fn end_pass(&mut self, next_stable_id: &mut NodeId) {
        // Shell by shell, as below: `VecDeque::append` and `split_off` bring
        // code of their own for a ring that wraps.
        let returning = std::mem::take(&mut self.returning);
        let cold = std::mem::take(&mut self.cold);
        self.warm.reserve(returning.len() + cold.len());
        for recycled in returning {
            self.warm.push_back(recycled);
        }
        for mut recycled in cold {
            recycled.set_origin(RecycleOrigin::Warm);
            self.warm.push_back(recycled);
        }
        let needed = std::mem::take(&mut self.fresh_creations);
        // A type that only had shells created fresh has nothing to size yet.
        if self.warm.is_empty()
            && self.limit.is_none()
            && self.warm_target.is_none()
            && self.prototype.is_none()
        {
            return;
        }
        let target = self.update_warm_target(needed);
        let kept = self.kept();
        self.replenish_warm(target.min(kept), next_stable_id);
        self.give_back_past(kept);
        self.compact(kept);
    }
}

// Drops the oldest shells of `pool` past `limit`, oldest first.
fn drop_oldest_past(pool: &mut VecDeque<RecycledNode>, limit: Option<usize>) {
    if let Some(limit) = limit {
        drop_oldest(pool, limit);
    }
}

// Drops the oldest shells of `pool` until it holds `kept`. One at a time:
// `VecDeque::drain` brings code of its own for a ring that wraps.
fn drop_oldest(pool: &mut VecDeque<RecycledNode>, kept: usize) {
    while pool.len() > kept {
        pool.pop_front();
    }
}
