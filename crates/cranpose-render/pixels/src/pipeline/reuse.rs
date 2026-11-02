use std::ops::Range;

use cranpose_core::NodeId;

use crate::scene::{ItemCounts, RasterScene};

/// The scene the frame before built, whose items a walk copies for each
/// subtree that did not change since.
pub(super) struct Reuse<'a> {
    pub(super) old: &'a RasterScene,
    /// The old record of the layer the walk reaches next, while the graph
    /// keeps the shape it had.
    pub(super) cursor: usize,
    /// The old records of unchanged subtrees, siblings one after another,
    /// to copy before the scene takes anything else, and the counts their
    /// items reach there: a run of them copies as one.
    pub(super) pending: Option<(Range<usize>, ItemCounts)>,
}

impl Reuse<'_> {
    /// The index of the old record of the node with `identity`, at the
    /// cursor or one subtree on, after a sibling before it went away.
    pub(super) fn find(&self, identity: (Option<NodeId>, Option<NodeId>)) -> Option<usize> {
        let records = &self.old.subtrees;
        let at = records.get(self.cursor)?;
        if at.identity == identity {
            return Some(self.cursor);
        }
        records
            .get(at.end)
            .is_some_and(|next| next.identity == identity)
            .then_some(at.end)
    }

    /// Queues the copy of the subtree of the old record `index`, which ends
    /// at `end` and pushed its items up to `stop`.
    pub(super) fn copy_later(
        &mut self,
        scene: &mut RasterScene,
        index: usize,
        end: usize,
        stop: ItemCounts,
    ) {
        match &mut self.pending {
            Some((records, pending_stop)) if records.end == index => {
                records.end = end;
                *pending_stop = stop;
            }
            _ => {
                self.flush(scene);
                self.pending = Some((index..end, stop));
            }
        }
    }

    /// Copies the queued subtrees into `scene`.
    fn flush(&mut self, scene: &mut RasterScene) {
        if let Some((records, stop)) = self.pending.take() {
            scene.copy_subtrees(self.old, records, stop);
        }
    }
}

/// Copies the subtrees `reuse` queued into `scene`, before it takes items of
/// its own.
pub(super) fn flush_copies(reuse: &mut Option<Reuse<'_>>, scene: &mut RasterScene) {
    if let Some(reuse) = reuse.as_mut() {
        reuse.flush(scene);
    }
}
