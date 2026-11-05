//! The walk that pushes a window's items straight from its layout tree,
//! with no render graph: it lowers each node alone, turns its draws into
//! items at once and lets the node go, and copies from the scene before the
//! subtrees whose nodes did not change since.

use std::cell::RefCell;

use cranpose_core::{MemoryApplier, NodeId, NodeMarks};
use cranpose_render_common::{
    NodeUpdate, SceneUpdates,
    graph::RenderNode,
    scene_builder::{
        LowerContext, LoweredNode, NodePlace, RecordingStore, SpaceFragment, has_children,
        kept_child_placement, lower_moved_subtree, lower_node_alone, lower_space_fragment,
        placed_child_position, recycle_lowered, release_lowered, visit_lowered_children,
    },
};
use cranpose_ui_graphics::Point;

use crate::{
    moved_subtrees::MovedSubtrees,
    pipeline::{RasterTraversalContext, Reuse, populate_draws_from_graph},
    raster_scene::{RasterScene, Subtree, SubtreeItems},
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
    // named more than once, and the nodes above them. Returns whether an
    // update may move a layer with no layout pass.
    pub(crate) fn mark(&mut self, applier: &MemoryApplier, updates: SceneUpdates<'_>) -> bool {
        self.marks.reset(applier);
        let mut moves = false;
        for &(node, update) in updates {
            moves |= !matches!(update, NodeUpdate::Content | NodeUpdate::Draws);
            let merge = |mark| {
                let merged = decode(mark).map_or(update, |before| before.merge(update));
                (mark & ABOVE_DIRTY) | (encode(merged) << UPDATE_SHIFT)
            };
            self.marks
                .update_with_ancestors(applier, node, merge, ABOVE_DIRTY);
        }
        moves
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

    // The update `node` itself takes, when it changed.
    pub(crate) fn update_of(&self, applier: &MemoryApplier, node: NodeId) -> Option<NodeUpdate> {
        decode(self.marks.get(applier, node))
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

// A subtree whose layer holds a 3D space, which the scene keeps whole, and
// whether the scene published where its surfaces show since the walk
// lowered it.
pub(crate) struct KeptSpace {
    pub space: SpaceFragment,
    pub published: bool,
}

// A walk of one window's layout tree, with the marks of the update it
// pushes, the subtrees whose layers hold a 3D space, which the scene keeps
// whole, and the subtrees of the children that moved in the update before.
pub(crate) struct LayoutWalk<'a> {
    applier: &'a MemoryApplier,
    marks: &'a DirtyMarks,
    spaces: &'a RefCell<Vec<KeptSpace>>,
    moved: &'a RefCell<MovedSubtrees>,
}

impl<'a> LayoutWalk<'a> {
    pub(crate) fn new(
        applier: &'a MemoryApplier,
        marks: &'a DirtyMarks,
        (spaces, moved): (&'a RefCell<Vec<KeptSpace>>, &'a RefCell<MovedSubtrees>),
    ) -> Self {
        Self {
            applier,
            marks,
            spaces,
            moved,
        }
    }

    // Lets go what the walk lowered and did not keep, as it ends.
    pub(crate) fn release(&self) {
        self.moved.borrow_mut().end_update();
        release_lowered();
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

    // Whether `node` at `place` lowers as a leaf: a child without children.
    pub(crate) fn lowers_as_leaf(&self, (node, place): (NodeId, NodePlace)) -> bool {
        place == NodePlace::Child && !has_children(self.applier, node)
    }

    // Whether the scene keeps `node`'s subtree whole, as its layer holds a
    // 3D space.
    pub(crate) fn holds_space(&self, node: NodeId) -> bool {
        self.spaces
            .borrow()
            .iter()
            .any(|kept| kept.space.node() == node)
    }

    // Lowers the window's root `root` alone, or whole when its layer holds a
    // 3D space, and hands its layer and the slot of its children to `push`.
    // False when the root is not placed.
    pub(crate) fn lower_root(
        &self,
        root: NodeId,
        push: impl FnOnce(&RenderNode, Option<&mut ChildSlot<'_>>),
    ) -> bool {
        let context = LowerContext::root(RecordingStore::Redrawn);
        let mut push = Some(push);
        let lowered =
            lower_node_alone(self.applier, (root, NodePlace::Root), &context, |lowered| {
                let LoweredNode {
                    node,
                    children,
                    child_context,
                } = lowered;
                let space = holds_space(&node);
                if !space && let Some(push) = push.take() {
                    let mut slot = ChildSlot {
                        walk: self,
                        owner: root,
                        children,
                        context: child_context,
                        old: None,
                    };
                    push(&node, (!children.is_empty()).then_some(&mut slot));
                }
                recycle_lowered(node);
                space
            });
        if lowered == Some(true)
            && let Some(push) = push.take()
            && let Some(space) =
                lower_space_fragment(self.applier, (root, NodePlace::Root), &context)
        {
            push(space.root(), None);
            self.keep_space(space);
        }
        self.release();
        lowered.is_some()
    }

    // Lowers `node` at `place` alone under `lower_context` and pushes its
    // subtree under `context`: over its old subtree, whose children's old
    // records are `old`, while a rewrite pushes the subtree it is in again.
    // A node whose layer holds a 3D space lowers whole, and the scene keeps
    // its subtree.
    pub(crate) fn push(
        &self,
        (node, place): (NodeId, NodePlace),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        reuse: &mut Option<Reuse<'_>>,
        old: Option<OldChildren>,
    ) {
        let lowered = lower_node_alone(self.applier, (node, place), lower_context, |lowered| {
            let LoweredNode {
                node: lowered,
                children,
                child_context,
            } = lowered;
            let space = holds_space(&lowered);
            if !space && let Some(subtree) = Subtree::of_child(&lowered) {
                let mut slot = ChildSlot {
                    walk: self,
                    owner: node,
                    children,
                    context: child_context,
                    old,
                };
                let slot = (!children.is_empty()).then_some(&mut slot);
                populate_draws_from_graph(subtree, scene, context, reuse, slot);
            }
            recycle_lowered(lowered);
            space
        });
        if lowered == Some(true)
            && let Some(space) = lower_space_fragment(self.applier, (node, place), lower_context)
        {
            if let Some(subtree) = Subtree::of_child(space.root()) {
                populate_draws_from_graph(subtree, scene, context, reuse, None);
            }
            self.keep_space(space);
        }
    }

    // Lowers `node` at `place` alone under `lower_context`, for
    // `push_lowered` to push its subtree after the walk lowered every node it
    // pushes again: lowering and recording, and pushing items and comparing
    // them, then each run for all the nodes, whose code a frame would
    // otherwise evict from the cache for every node. Puts its layer into
    // `layer` and, when it has children, what they lower with into `inner`.
    // Leaves both empty for a node gone, and for one whose layer holds a 3D
    // space, which `push` lowers.
    pub(crate) fn lower(
        &self,
        (node, place): (NodeId, NodePlace),
        lower_context: &LowerContext,
        (layer, inner): (&mut Option<RenderNode>, &mut Option<LowerContext>),
    ) {
        lower_node_alone(self.applier, (node, place), lower_context, |lowered| {
            if holds_space(&lowered.node) || Subtree::of_child(&lowered.node).is_none() {
                recycle_lowered(lowered.node);
                return;
            }
            if !lowered.children.is_empty() {
                *inner = Some(*lowered.child_context);
            }
            *layer = Some(lowered.node);
        });
    }

    // Pushes under `context` the subtree of `node`, whose `layer` `lower`
    // gave with what its children lower with, `inner`, for a node with
    // children: over its old subtree, whose children's old records are
    // `old`, while a rewrite pushes the subtree it is in again.
    pub(crate) fn push_lowered(
        &self,
        node: NodeId,
        (layer, inner): (RenderNode, Option<&LowerContext>),
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        old: Option<OldChildren>,
    ) {
        if let Some(subtree) = Subtree::of_child(&layer) {
            match inner {
                Some(inner) => {
                    visit_lowered_children(self.applier, node, |children| {
                        let mut slot = ChildSlot {
                            walk: self,
                            owner: node,
                            children,
                            context: inner,
                            old,
                        };
                        populate_draws_from_graph(
                            subtree,
                            scene,
                            context,
                            &mut None,
                            Some(&mut slot),
                        );
                    });
                }
                None => populate_draws_from_graph(subtree, scene, context, &mut None, None),
            }
        }
        recycle_lowered(layer);
    }

    // Pushes under `context` the subtree of `child`, which did not change,
    // though it moved or lies under other state than before, over its old
    // subtree at the record `at`: from the subtree lowered whole when it
    // moved in the updates before, placed where it now lies, else lowered
    // whole now, which the scene keeps while the child moves. `position` is
    // where the child lies in its parent's content, when the walk read it.
    // False for a subtree that holds a 3D space, which the walk lowers as
    // any other.
    pub(crate) fn push_moved(
        &self,
        (child, position): (NodeId, Option<Point>),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        at: usize,
    ) -> bool {
        if self.keeps_space(&scene.subtrees, at) {
            return false;
        }
        // A child no longer placed pushes nothing.
        let Some(position) = position.or_else(|| placed_child_position(self.applier, child)) else {
            return true;
        };
        let mut kept = self.moved.borrow_mut().take(child);
        if let Some(stale) =
            kept.take_if(|subtree| !subtree.follow(self.applier, position, lower_context))
        {
            stale.recycle();
        }
        let Some(subtree) =
            kept.or_else(|| lower_moved_subtree(self.applier, child, lower_context))
        else {
            return true;
        };
        if let Some(node) = Subtree::of_child(subtree.root()) {
            populate_draws_from_graph(node, scene, context, &mut None, None);
        }
        self.moved.borrow_mut().keep(subtree);
        true
    }

    // Whether the old subtree at the record `at` holds a subtree the scene
    // keeps whole for its 3D space.
    fn keeps_space(&self, records: &[SubtreeItems], at: usize) -> bool {
        let spaces = self.spaces.borrow();
        !spaces.is_empty()
            && records[at..records[at].end].iter().any(|record| {
                spaces
                    .iter()
                    .any(|kept| names(record.identity, kept.space.node()))
            })
    }

    // Keeps `space` in place of the subtree the scene kept for its node.
    fn keep_space(&self, space: SpaceFragment) {
        let mut spaces = self.spaces.borrow_mut();
        let kept = KeptSpace {
            space,
            published: false,
        };
        match spaces
            .iter_mut()
            .find(|old| old.space.node() == kept.space.node())
        {
            Some(old) => *old = kept,
            None => spaces.push(kept),
        }
    }
}

// Whether `node`, a node lowered alone, holds a 3D space in its own layer.
fn holds_space(node: &RenderNode) -> bool {
    let RenderNode::Layer(layer) = node else {
        return false;
    };
    if layer.graphics_layer.space.is_some() {
        return true;
    }
    // The wrapper of a node's outer draws holds the node's layer.
    layer.wraps.is_some_and(|wrapped| {
        layer.children.iter().any(|child| {
            matches!(child, RenderNode::Layer(inner)
                if inner.node_id == Some(wrapped) && inner.graphics_layer.space.is_some())
        })
    })
}

// The layout children of a lowered node, which its own layer pushes between
// its draws before them and after them.
pub(crate) struct ChildSlot<'w> {
    walk: &'w LayoutWalk<'w>,
    owner: NodeId,
    children: &'w [NodeId],
    context: &'w LowerContext,
    // While a rewrite pushes the owner over its old subtree, where the
    // children's old records lie.
    old: Option<OldChildren>,
}

impl ChildSlot<'_> {
    // The node whose children these are.
    pub(crate) fn owner(&self) -> NodeId {
        self.owner
    }

    // Pushes the children anew, into the scene of the owner's own surface,
    // which holds none of their old records.
    pub(crate) fn for_surface(&mut self) -> &mut Self {
        self.old = None;
        self
    }

    // Whether the owner placed its children where it did before: so it did
    // when only state its draws read changed.
    fn placed_as_before(&self) -> bool {
        self.walk.marks.update_of(self.walk.applier, self.owner) == Some(NodeUpdate::Draws)
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
            walk.push(
                (child, NodePlace::Child),
                self.context,
                (scene, context),
                reuse,
                None,
            );
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
        let placed_as_before = self.placed_as_before();
        for &child in self.children {
            let found = OldChildren::find(&scene.subtrees, (old.cursor, old.end), child);
            let Some(at) = found else {
                walk.push(
                    (child, NodePlace::Child),
                    self.context,
                    (scene, context),
                    &mut None,
                    None,
                );
                continue;
            };
            let end = scene.subtrees[at].end;
            let record = &scene.subtrees[at];
            let clean = walk.is_clean(child);
            // A child whose owner placed it as before lies where it did.
            let placement = (clean && context.same_as_before && !placed_as_before)
                .then(|| kept_child_placement(walk.applier, child, self.context))
                .flatten();
            let lies_as_before = context.same_as_before
                && (placed_as_before
                    || placement.is_some_and(|placement| {
                        (placement.identity, placement.transform)
                            == (record.identity, record.transform)
                    }));
            if clean
                && lies_as_before
                && let Some(mut rewrite) = scene.rewrite.take()
            {
                rewrite.keep(scene, at);
                scene.rewrite = Some(rewrite);
            } else if !(clean
                && walk.push_moved(
                    (child, placement.map(|placement| placement.position)),
                    self.context,
                    (scene, context),
                    at,
                ))
            {
                let children = OldChildren::of(&scene.subtrees, at, child);
                walk.push(
                    (child, NodePlace::Child),
                    self.context,
                    (scene, context),
                    &mut None,
                    Some(children),
                );
            }
            old.cursor = end;
        }
    }
}
