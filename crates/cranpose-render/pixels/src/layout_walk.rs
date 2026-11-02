//! The walk that pushes a window's items straight from its layout tree,
//! with no render graph: it lowers each node alone, turns its draws into
//! items at once and lets the node go, and copies from the scene before the
//! subtrees whose nodes did not change since.

use cranpose_core::{MemoryApplier, NodeId, NodeMarks};
use cranpose_render_common::{
    NodeUpdate, SceneUpdates,
    graph::RenderNode,
    scene_builder::{
        LowerContext, LoweredNode, NodePlace, RecordingStore, kept_child_placement,
        lower_node_alone, recycle_lowered, release_lowered,
    },
};

use crate::{
    pipeline::{RasterTraversalContext, Reuse, populate_draws_from_graph},
    scene::{RasterScene, Subtree, SubtreeItems},
};

// The mark of a node above a dirty node.
const ABOVE_DIRTY: u8 = 1;
// A dirty node's update sits in the bits above the first.
const UPDATE_SHIFT: u8 = 1;

// The marks a scene update leaves on the layout nodes: each dirty node's
// update, and on each node above one, that it is.
#[derive(Default)]
pub(crate) struct DirtyMarks {
    marks: NodeMarks,
}

impl DirtyMarks {
    // Marks the nodes of `updates` with their updates, merged for a node
    // named more than once, and the nodes above them.
    pub(crate) fn mark(&mut self, applier: &MemoryApplier, updates: SceneUpdates<'_>) {
        self.marks.reset(applier);
        for &(node, update) in updates {
            let mark = self.marks.get(applier, node);
            let merged = decode(mark).map_or(update, |before| before.merge(update));
            self.marks.set(
                applier,
                node,
                (mark & ABOVE_DIRTY) | (encode(merged) << UPDATE_SHIFT),
            );
            self.marks.add_with_ancestors(applier, node, 0, ABOVE_DIRTY);
        }
    }

    // Unmarks every node: a build with no marks lowers every node.
    pub(crate) fn clear(&mut self, applier: &MemoryApplier) {
        self.marks.reset(applier);
    }

    // Whether neither `node` nor a node beneath it changed.
    pub(crate) fn is_clean(&self, applier: &MemoryApplier, node: NodeId) -> bool {
        self.marks.get(applier, node) == 0
    }

    // Whether `node` itself changed.
    pub(crate) fn changed(&self, applier: &MemoryApplier, node: NodeId) -> bool {
        decode(self.marks.get(applier, node)).is_some()
    }
}

fn encode(update: NodeUpdate) -> u8 {
    match update {
        NodeUpdate::Content => 1,
        NodeUpdate::Draws => 2,
        NodeUpdate::Layer => 3,
        NodeUpdate::Moved => 4,
        NodeUpdate::MovedLayer => 5,
    }
}

fn decode(mark: u8) -> Option<NodeUpdate> {
    match mark >> UPDATE_SHIFT {
        1 => Some(NodeUpdate::Content),
        2 => Some(NodeUpdate::Draws),
        3 => Some(NodeUpdate::Layer),
        4 => Some(NodeUpdate::Moved),
        5 => Some(NodeUpdate::MovedLayer),
        _ => None,
    }
}

// Whether the record `identity` is of the subtree of the layout node
// `node`: its layer or inline node, or the wrapper of its outer draws.
pub(crate) fn names(identity: (Option<NodeId>, Option<NodeId>), node: NodeId) -> bool {
    identity == (Some(node), None) || identity == (None, Some(node))
}

// The first record from `from` on, before `end`, of a child subtree: past
// the records of the draw groups of a node's own draws, which have no node.
pub(crate) fn child_record_from(records: &[SubtreeItems], mut from: usize, end: usize) -> usize {
    while from < end && records[from].identity == (None, None) {
        from = records[from].end;
    }
    from
}

// The first record of a child subtree of the record `own`, whose subtree's
// records end at `end`.
pub(crate) fn first_child_record(records: &[SubtreeItems], own: usize, end: usize) -> usize {
    child_record_from(records, own + 1, end)
}

// Where the old records of a node's child subtrees lie in the scene a
// rewrite pushes the node over: the next one a child takes, and their end.
#[derive(Clone, Copy)]
pub(crate) struct OldChildren {
    cursor: usize,
    end: usize,
}

impl OldChildren {
    // The old children of the node `node`, whose old subtree's record is
    // `record`, read before the rewrite takes any of its places.
    pub(crate) fn of(records: &[SubtreeItems], record: usize, node: NodeId) -> Self {
        let own = Self::own_record(records, record, node);
        let end = records[own].end;
        Self {
            cursor: first_child_record(records, own, end),
            end,
        }
    }

    // The record of `node`'s own layer: `record`, or inside the wrapper of
    // its outer draws that `record` is, its layer's.
    pub(crate) fn own_record(records: &[SubtreeItems], record: usize, node: NodeId) -> usize {
        if records[record].identity != (None, Some(node)) {
            return record;
        }
        let end = records[record].end;
        let mut child = record + 1;
        while child < end {
            if records[child].identity == (Some(node), None) {
                return child;
            }
            child = records[child].end;
        }
        record
    }

    // The old record of `child` at `cursor` of the children `(cursor, end)`,
    // or one subtree on, after a child before it went away.
    pub(crate) fn find(
        records: &[SubtreeItems],
        (cursor, end): (usize, usize),
        child: NodeId,
    ) -> Option<usize> {
        let at = records[..end].get(cursor)?;
        if names(at.identity, child) {
            return Some(cursor);
        }
        records[..end]
            .get(at.end)
            .is_some_and(|next| names(next.identity, child))
            .then_some(at.end)
    }
}

// A walk of one window's layout tree, with the marks of the update it
// pushes.
pub(crate) struct LayoutWalk<'a> {
    applier: &'a MemoryApplier,
    marks: &'a DirtyMarks,
}

impl<'a> LayoutWalk<'a> {
    pub(crate) fn new(applier: &'a MemoryApplier, marks: &'a DirtyMarks) -> Self {
        Self { applier, marks }
    }

    pub(crate) fn applier(&self) -> &'a MemoryApplier {
        self.applier
    }

    // Whether neither `node` nor a node beneath it changed.
    pub(crate) fn is_clean(&self, node: NodeId) -> bool {
        self.marks.is_clean(self.applier, node)
    }

    // Whether `node` itself changed.
    pub(crate) fn changed(&self, node: NodeId) -> bool {
        self.marks.changed(self.applier, node)
    }

    // Lowers `node` at `place` alone under `lower_context` and pushes its
    // subtree under `context`, over its old subtree, whose children's old
    // records are `old`, while a rewrite pushes it again.
    pub(crate) fn push_node(
        &self,
        (node, place): (NodeId, NodePlace),
        lower_context: LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        old: OldChildren,
    ) {
        lower_node_alone(self.applier, (node, place), lower_context, |lowered| {
            let LoweredNode {
                node: lowered,
                children,
                child_context,
            } = lowered;
            if let Some(subtree) = Subtree::of_child(&lowered) {
                let mut slot = ChildSlot {
                    walk: self,
                    owner: node,
                    children,
                    context: child_context,
                    old: Some(old),
                };
                let slot = (!children.is_empty()).then_some(&mut slot);
                populate_draws_from_graph(subtree, scene, context, &mut None, slot);
            }
            recycle_lowered(lowered);
        });
    }

    // Lowers the window's root `root` alone and hands its layer and the slot
    // of its children to `push`. False when the root is not placed.
    pub(crate) fn lower_root(
        &self,
        root: NodeId,
        push: impl FnOnce(&RenderNode, Option<&mut ChildSlot<'_>>),
    ) -> bool {
        let context = LowerContext::root(RecordingStore::Shared);
        let lowered = lower_node_alone(self.applier, (root, NodePlace::Root), context, |lowered| {
            let LoweredNode {
                node,
                children,
                child_context,
            } = lowered;
            let mut slot = ChildSlot {
                walk: self,
                owner: root,
                children,
                context: child_context,
                old: None,
            };
            push(&node, (!children.is_empty()).then_some(&mut slot));
            recycle_lowered(node);
        });
        release_lowered();
        lowered.is_some()
    }

    // Lowers `child` alone under `lower_context` and pushes its subtree
    // under `context`, over its old subtree, whose children's old records
    // are `old`, while a rewrite pushes the subtree it is in again.
    fn push_child(
        &self,
        (child, lower_context): (NodeId, LowerContext),
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        reuse: &mut Option<Reuse<'_>>,
        old: Option<OldChildren>,
    ) {
        lower_node_alone(
            self.applier,
            (child, NodePlace::Child),
            lower_context,
            |lowered| {
                let LoweredNode {
                    node,
                    children,
                    child_context,
                } = lowered;
                if let Some(subtree) = Subtree::of_child(&node) {
                    let mut slot = ChildSlot {
                        walk: self,
                        owner: child,
                        children,
                        context: child_context,
                        old,
                    };
                    let slot = (!children.is_empty()).then_some(&mut slot);
                    populate_draws_from_graph(subtree, scene, context, reuse, slot);
                }
                recycle_lowered(node);
            },
        );
    }
}

// The layout children of a lowered node, which its own layer pushes between
// its draws before them and after them.
pub(crate) struct ChildSlot<'w> {
    walk: &'w LayoutWalk<'w>,
    owner: NodeId,
    children: &'w [NodeId],
    context: LowerContext,
    // While a rewrite pushes the owner over its old subtree, where the
    // children's old records lie.
    old: Option<OldChildren>,
}

impl ChildSlot<'_> {
    // The node whose children these are.
    pub(crate) fn owner(&self) -> NodeId {
        self.owner
    }

    // Pushes the children's subtrees under `context`, copying from `reuse`'s
    // scene those whose nodes did not change and that lie where they did.
    pub(crate) fn push_children(
        &mut self,
        scene: &mut RasterScene,
        context: &RasterTraversalContext<'_>,
        reuse: &mut Option<Reuse<'_>>,
    ) {
        let walk = self.walk;
        if let Some(old) = self.old {
            return self.push_over_old(old, scene, context);
        }
        for &child in self.children {
            if let Some(kept) = reuse.as_mut()
                && walk.is_clean(child)
                && let Some(placement) = kept_child_placement(walk.applier, child, self.context)
                && kept.keep(scene, placement, context)
            {
                continue;
            }
            walk.push_child((child, self.context), (scene, context), reuse, None);
        }
    }

    // Pushes the children's subtrees under `context` over their old ones,
    // whose records lie at `old`, keeping in the rewrite those whose nodes
    // did not change and that lie where they did.
    fn push_over_old(
        &self,
        mut old: OldChildren,
        scene: &mut RasterScene,
        context: &RasterTraversalContext<'_>,
    ) {
        let walk = self.walk;
        for &child in self.children {
            let found = OldChildren::find(&scene.subtrees, (old.cursor, old.end), child);
            let Some(at) = found else {
                walk.push_child((child, self.context), (scene, context), &mut None, None);
                continue;
            };
            let end = scene.subtrees[at].end;
            let record = &scene.subtrees[at];
            if context.same_as_before
                && walk.is_clean(child)
                && kept_child_placement(walk.applier, child, self.context)
                    == Some((record.identity, record.transform))
                && let Some(mut rewrite) = scene.rewrite.take()
            {
                rewrite.keep(scene, at);
                scene.rewrite = Some(rewrite);
            } else {
                let children = OldChildren::of(&scene.subtrees, at, child);
                walk.push_child(
                    (child, self.context),
                    (scene, context),
                    &mut None,
                    Some(children),
                );
            }
            old.cursor = end;
        }
    }
}
