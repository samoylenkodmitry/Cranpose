//! The nodes of deactivated slots that later slots can take, and how many
//! of them stay.

use std::collections::VecDeque;

use super::SlotId;
use crate::{NodeId, collections::map::HashMap};

const DEFAULT_MAX_PER_TYPE: usize = 5;

const DEFAULT_MAX_UNTYPED: usize = 10;

// The nodes of deactivated slots, oldest first: a queue for each content type
// and one for the slots without a type, each bounded by its limit.
pub(super) struct ReusePool {
    by_type: HashMap<u64, VecDeque<(SlotId, NodeId)>>,
    untyped: VecDeque<(SlotId, NodeId)>,
    max_per_type: usize,
    max_untyped: usize,
}

impl ReusePool {
    pub(super) fn new() -> Self {
        Self {
            by_type: HashMap::default(),
            untyped: VecDeque::new(),
            max_per_type: DEFAULT_MAX_PER_TYPE,
            max_untyped: DEFAULT_MAX_UNTYPED,
        }
    }

    pub(super) fn set_limits(&mut self, per_type: usize, untyped: usize) {
        self.max_per_type = per_type;
        self.max_untyped = untyped;
    }

    pub(super) fn type_count(&self) -> usize {
        self.by_type.len()
    }

    pub(super) fn untyped_len(&self) -> usize {
        self.untyped.len()
    }

    // Adds the nodes of `slot`, deactivated, to the queue of `content_type`.
    pub(super) fn push(
        &mut self,
        content_type: Option<u64>,
        slot: SlotId,
        nodes: impl IntoIterator<Item = NodeId>,
    ) {
        let queue = match content_type {
            Some(content_type) => self.by_type.entry(content_type).or_default(),
            None => &mut self.untyped,
        };
        queue.extend(nodes.into_iter().map(|node| (slot, node)));
    }

    // Takes the oldest node of the queue of `content_type`, the untyped one
    // for `None`, whose slot `accepts`.
    pub(super) fn take(
        &mut self,
        content_type: Option<u64>,
        mut accepts: impl FnMut(SlotId) -> bool,
    ) -> Option<(SlotId, NodeId)> {
        let Some(content_type) = content_type else {
            let index = self.untyped.iter().position(|(slot, _)| accepts(*slot))?;
            return self.untyped.remove(index);
        };
        let queue = self.by_type.get_mut(&content_type)?;
        let index = queue.iter().position(|(slot, _)| accepts(*slot))?;
        let taken = queue.remove(index);
        if queue.is_empty() {
            self.by_type.remove(&content_type);
        }
        taken
    }

    // Takes `node` out of whichever queue holds it, and returns its slot.
    pub(super) fn remove_node(&mut self, node: NodeId) -> Option<SlotId> {
        let typed = self.by_type.iter().find_map(|(&content_type, queue)| {
            queue
                .iter()
                .position(|(_, pooled)| *pooled == node)
                .map(|position| (content_type, position))
        });
        if let Some((content_type, position)) = typed {
            let queue = self.by_type.get_mut(&content_type)?;
            let (slot, _) = queue.remove(position)?;
            if queue.is_empty() {
                self.by_type.remove(&content_type);
            }
            return Some(slot);
        }
        let position = self
            .untyped
            .iter()
            .position(|(_, pooled)| *pooled == node)?;
        self.untyped.remove(position).map(|(slot, _)| slot)
    }

    // Takes out the oldest nodes past each queue's limit: the typed queues'
    // first, then the untyped one's.
    pub(super) fn take_excess(&mut self) -> Vec<(SlotId, NodeId)> {
        let mut excess = Vec::new();
        for queue in self.by_type.values_mut() {
            while queue.len() > self.max_per_type {
                excess.extend(queue.pop_front());
            }
        }
        while self.untyped.len() > self.max_untyped {
            excess.extend(self.untyped.pop_front());
        }
        self.by_type.retain(|_, queue| !queue.is_empty());
        excess
    }

    // Every node the pool holds: the typed queues' first.
    pub(super) fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.by_type
            .values()
            .flat_map(|queue| queue.iter().map(|(_, node)| *node))
            .chain(self.untyped.iter().map(|(_, node)| *node))
    }
}
