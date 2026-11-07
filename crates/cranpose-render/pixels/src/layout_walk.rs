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
        KeptHead, KeptNodeRef, LowerContext, LoweredNode, NodePlace, OwnLayer, RecordingStore,
        SpaceFragment, has_children, lower_moved_subtree, lower_node_alone, lower_space_fragment,
        placed_child_position, recycle_lowered, release_lowered, visit_kept_node,
        visit_lowered_children,
    },
};
use cranpose_ui_graphics::Point;

use crate::{
    moved_subtrees::MovedSubtrees,
    pipeline::{ContentStyle, RasterTraversalContext, Reuse, populate_draws_from_graph},
    raster_scene::{RasterScene, Subtree, SubtreeItems},
};

mod child_slot;
use child_slot::ChildPushes;
pub(crate) use child_slot::{ChildPosition, ChildSlot};

// The mark of a node above a dirty node.
const ABOVE_DIRTY: u8 = 1;
// A dirty node's update sits in the bits above the first.
const UPDATE_SHIFT: u8 = 1;

// The marks a scene update leaves on the layout nodes: each dirty node's
// update, and on each node above one, that it is.
#[derive(Default)]
pub(crate) struct DirtyMarks {
    marks: NodeMarks,
    // Whether the marks name what an update changed. After `clear` every
    // node reads as clean, though any may have changed.
    from_update: bool,
}

impl DirtyMarks {
    // Marks the nodes of `updates` with their updates, merged for a node
    // named more than once, and the nodes above them. Lets go the subtrees
    // `moved` keeps of each node an update names and of the nodes above
    // them. Returns whether an update may move a layer with no layout pass.
    pub(crate) fn mark(
        &mut self,
        applier: &MemoryApplier,
        updates: SceneUpdates<'_>,
        moved: &mut MovedSubtrees,
    ) -> bool {
        self.marks.reset(applier);
        self.from_update = true;
        let mut moves = false;
        for &(node, update) in updates {
            moves |= !matches!(update, NodeUpdate::Content | NodeUpdate::Draws);
            moved.let_go(node);
            let merge = |mark| {
                let merged = decode(mark).map_or(update, |before| before.merge(update));
                (mark & ABOVE_DIRTY) | (encode(merged) << UPDATE_SHIFT)
            };
            let above = |node| moved.let_go(node);
            self.marks
                .update_with_ancestors(applier, node, merge, (ABOVE_DIRTY, above));
        }
        moves
    }

    // Unmarks every node: a build with no marks lowers every node.
    pub(crate) fn clear(&mut self, applier: &MemoryApplier) {
        self.marks.reset(applier);
        self.from_update = false;
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
    // Indexed by the bits above the first, as `encode` writes them.
    const UPDATES: [Option<NodeUpdate>; 6] = [
        None,
        Some(NodeUpdate::Content),
        Some(NodeUpdate::Draws),
        Some(NodeUpdate::Layer),
        Some(NodeUpdate::Moved),
        Some(NodeUpdate::MovedLayer),
    ];
    UPDATES
        .get(usize::from(mark >> UPDATE_SHIFT))
        .copied()
        .flatten()
}

// Whether the record `identity` is of the subtree of the layout node
// `node`: its layer or inline node, or the wrapper of its outer draws.
pub(crate) fn names(identity: (Option<NodeId>, Option<NodeId>), node: NodeId) -> bool {
    identity == (Some(node), None) || identity == (None, Some(node))
}

// The 3D subtree of `spaces` whose node the record `identity` names.
pub(crate) fn kept_space(
    spaces: &[KeptSpace],
    identity: (Option<NodeId>, Option<NodeId>),
) -> Option<&KeptSpace> {
    spaces
        .iter()
        .find(|kept| names(identity, kept.space.node()))
}

// The first record from `from` on, before `end`, of a child subtree: past
// the records of the draw groups of a node's own draws, which have no node.
pub(crate) fn child_record_from(records: &[SubtreeItems], mut from: usize, end: usize) -> usize {
    while from < end && records[from].identity == (None, None) {
        from = records[from].end;
    }
    from
}

// The record of the first child subtree of `node`, whose old subtree's
// record is `at`, when that record is the node's own layer, the draw groups
// before its first child note nothing passed down, and nothing of the
// node's own follows its last child: the shape a walk keeps around the
// children it pushes again.
fn first_child_of_kept(records: &[SubtreeItems], at: usize, node: NodeId) -> Option<usize> {
    let record = &records[at];
    if record.identity != (Some(node), None) {
        return None;
    }
    let first = child_record_from(records, at + 1, record.end);
    if records[at + 1..first]
        .iter()
        .any(|group| group.passed_down.is_some())
    {
        return None;
    }
    let (mut last, mut next) = (None, first);
    while next < record.end {
        if records[next].identity == (None, None) {
            return None;
        }
        last = Some(next);
        next = records[next].end;
    }
    (records[last?].stop == record.stop).then_some(first)
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
            cursor: child_record_from(records, own + 1, end),
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

    // Moves the cursor past the child whose old record the walk found at
    // `found`, which a rejoin moved to `at`, and whose old end it read as
    // `end` there. The splices since then moved the old records after them
    // `now - moved` places on, the rejoin's included.
    fn pass(&mut self, (found, at, end): (usize, usize, usize), (moved, now): (isize, isize)) {
        let rejoined = at as isize - found as isize;
        self.cursor = end.saturating_add_signed(now - moved - rejoined);
        self.end = self.end.saturating_add_signed(now - moved);
    }

    // Whether a child subtree's record lies at the cursor: past the last
    // child lie only the node's own draws.
    pub(crate) fn holds_children(self, records: &[SubtreeItems]) -> bool {
        self.cursor < self.end && records[self.cursor].node().is_some()
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
    // Whether the scene kept no 3D subtree when the walk started: no old
    // record holds one.
    no_space: bool,
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
            no_space: spaces.borrow().is_empty(),
            spaces,
            moved,
        }
    }

    pub(crate) fn applier(&self) -> &'a MemoryApplier {
        self.applier
    }

    // Whether neither `node` nor a node beneath it changed.
    pub(crate) fn is_clean(&self, node: NodeId) -> bool {
        self.marks.is_clean(self.applier, node)
    }

    // The update `node` itself takes, when it changed.
    pub(crate) fn update_of(&self, node: NodeId) -> Option<NodeUpdate> {
        self.marks.update_of(self.applier, node)
    }

    // Whether `node` itself changed.
    pub(crate) fn changed(&self, node: NodeId) -> bool {
        self.marks.changed(self.applier, node)
    }

    // Whether `node` at `place`, a child whose old record is `record`, draws
    // as it drew, as it only moved or only its layer changed, and the record
    // keeps its head: it pushes again from the head.
    pub(crate) fn pushes_from_head(
        &self,
        (node, place): (NodeId, NodePlace),
        record: &SubtreeItems,
    ) -> bool {
        place == NodePlace::Child
            && matches!(
                self.marks.update_of(self.applier, node),
                Some(NodeUpdate::Moved | NodeUpdate::Layer)
            )
            && record.head.is_some()
    }

    // Whether `node` at `place` lowers as a leaf: a child without children.
    pub(crate) fn lowers_as_leaf(&self, (node, place): (NodeId, NodePlace)) -> bool {
        place == NodePlace::Child && !has_children(self.applier, node)
    }

    // Whether the scene keeps `node`'s subtree whole, as its layer holds a
    // 3D space.
    // Whether no subtree the scene keeps whole holds a 3D space.
    pub(crate) fn holds_no_space(&self) -> bool {
        self.spaces.borrow().is_empty()
    }

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
                        pushes: ChildPushes::Anew,
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
        release_lowered();
        lowered.is_some()
    }

    // Lowers `node` at `place` alone under `lower_context` and pushes its
    // subtree under `context`: over its old subtree, whose children's old
    // records are `old`, while a rewrite pushes the subtree it is in again.
    // A child with children that an update pushes keeps its layer for the
    // next updates, which push it again while the child's own draws stay. A
    // node whose layer holds a 3D space lowers whole, and the scene keeps
    // its subtree. Out of line: the update and the children of a layer each
    // call it. A copy in each caller would take room in the instruction cache.
    #[inline(never)]
    pub(crate) fn push(
        &self,
        (node, place): (NodeId, NodePlace),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        reuse: &mut Option<Reuse<'_>>,
        old: Option<OldChildren>,
    ) {
        // A child an update pushes keeps its node, lowered alone, in its
        // record for the next updates, as `keeps_head` says.
        let keeps_head = self.keeps_head(place);
        let index = scene.next_record();
        let lowered = lower_node_alone(self.applier, (node, place), lower_context, |lowered| {
            let LoweredNode {
                node: lowered,
                children,
                child_context,
            } = lowered;
            let space = holds_space(&lowered);
            let subtree = Subtree::of_child(&lowered).filter(|_| !space);
            let pushed = subtree.is_some();
            if let Some(subtree) = subtree {
                let mut slot = ChildSlot {
                    walk: self,
                    owner: node,
                    children,
                    context: child_context,
                    pushes: ChildPushes::of(old),
                };
                let slot = (!children.is_empty()).then_some(&mut slot);
                populate_draws_from_graph(subtree, scene, context, reuse, slot);
            }
            match KeptHead::new(lowered, lower_context) {
                Ok(head) if keeps_head && pushed => scene.keep_head(index, head),
                Ok(head) => head.recycle(),
                Err(draw) => recycle_lowered(draw),
            }
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
    // `layer` and whether it has children into `children`. Leaves the layer
    // empty for a node gone, and for one whose layer holds a 3D space, which
    // `push` lowers. Out of line, as `Repush::rewrite_subtree` is.
    #[inline(never)]
    pub(crate) fn lower(
        &self,
        (node, place): (NodeId, NodePlace),
        lower_context: &LowerContext,
        (layer, children): (&mut Option<KeptHead>, &mut bool),
    ) {
        lower_node_alone(self.applier, (node, place), lower_context, |lowered| {
            if holds_space(&lowered.node) {
                return recycle_lowered(lowered.node);
            }
            match KeptHead::new(lowered.node, lower_context) {
                Ok(head) => {
                    *children = !lowered.children.is_empty();
                    *layer = Some(head);
                }
                Err(draw) => recycle_lowered(draw),
            }
        });
    }

    // Pushes under `context` the subtree of `node`, whose `layer` `lower`
    // gave with what its children lower with, `inner`, for a node with
    // children: over its old subtree, whose children's old records are
    // `old`, while a rewrite pushes the subtree it is in again.
    pub(crate) fn push_lowered(
        &self,
        (node, place): (NodeId, NodePlace),
        (layer, inner): (RenderNode, Option<&LowerContext>),
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        (lower_context, old): (&LowerContext, Option<OldChildren>),
    ) {
        let index = scene.next_record();
        let Some(subtree) = Subtree::of_child(&layer) else {
            return recycle_lowered(layer);
        };
        match inner {
            Some(inner) => {
                self.push_lowered_children((node, subtree), inner, (scene, context), old);
            }
            None => populate_draws_from_graph(subtree, scene, context, &mut None, None),
        }
        match KeptHead::new(layer, lower_context) {
            Ok(head) if self.keeps_head(place) => scene.keep_head(index, head),
            Ok(head) => head.recycle(),
            Err(draw) => recycle_lowered(draw),
        }
    }

    // Whether a node at `place`, which an update lowers, keeps its head for
    // the next updates: a child, which a later update may push again from
    // the head, moved or with a new layer, or lower again into the head's
    // boxes and recordings, as a retained graph lowers into its node's.
    fn keeps_head(&self, place: NodePlace) -> bool {
        place == NodePlace::Child && self.marks.from_update
    }

    // Pushes `subtree`, the layer of `node`, with the children `inner` lowers
    // with, for `push_lowered`. Out of line: most nodes an update pushes
    // again are leaves, and the visit of the children inline would grow the
    // rewrite that runs for each of them.
    #[inline(never)]
    fn push_lowered_children(
        &self,
        (node, subtree): (NodeId, Subtree<'_>),
        inner: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        old: Option<OldChildren>,
    ) {
        visit_lowered_children(self.applier, node, |children| {
            let mut slot = ChildSlot {
                walk: self,
                owner: node,
                children,
                context: inner,
                pushes: ChildPushes::of(old),
            };
            populate_draws_from_graph(subtree, scene, context, &mut None, Some(&mut slot));
        });
    }

    // Pushes again, over its old subtree at the record `at`, the subtree of
    // `child`, a node that did not change, lies where it did and holds nodes
    // that did: its record, its own items before its children and its
    // passed-down entry stay as they were, and its children push over their
    // old subtrees under `lower_context`, what the child lowers with. False,
    // with nothing pushed, when its old subtree does not take that shape or
    // the rewrite does not stand at it.
    pub(crate) fn push_through(
        &self,
        (child, at): (NodeId, usize),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
    ) -> bool {
        let Some(first) = first_child_of_kept(&scene.subtrees, at, child) else {
            return false;
        };
        let pushed = visit_kept_node(
            self.applier,
            (child, NodePlace::Child),
            lower_context,
            |children, inner| {
                let Some(entry) = scene
                    .rewrite
                    .as_deref_mut()
                    .and_then(|rewrite| rewrite.reopen(&scene.subtrees, at, first))
                else {
                    return false;
                };
                let passed = scene.passed_down[entry].clone();
                let style = ContentStyle::of(&passed);
                let mut slot = ChildSlot {
                    walk: self,
                    owner: child,
                    children,
                    context: inner,
                    pushes: ChildPushes::OverOld(OldChildren::of(&scene.subtrees, at, child)),
                };
                slot.push_children(scene, &context.kept_beneath((&passed, &style)), &mut None);
                scene.close_subtree(at);
                true
            },
        );
        pushed == Some(true)
    }

    // Pushes under `context` the subtree of `child`, which draws as it drew,
    // over its old subtree at the record `at`, from the head the record
    // keeps: its own items from the head, with the layer the child has now
    // when its update changed only that, at `position` in its parent's
    // content, or where its layout now places it when it moved, and its
    // children from their old records. False, with nothing pushed, when the
    // child changed more than its place or its layer, the record keeps no
    // head the child can take, the child is no longer placed, or the subtree
    // holds a 3D space, which the walk lowers as any other.
    pub(crate) fn push_from_head(
        &self,
        (child, at, position): (NodeId, usize, ChildPosition),
        (change, lower_context): (HeadChange, &LowerContext),
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
    ) -> bool {
        if !self.marks.from_update || self.keeps_space(&scene.subtrees, at) {
            return false;
        }
        let (update, children) = match change {
            HeadChange::Nothing => (None, HeadChildren::Whole),
            HeadChange::Marked(update) => (update, HeadChildren::Records),
        };
        let position = match (update, position) {
            (None | Some(NodeUpdate::Layer), ChildPosition::AsBefore) => None,
            (None | Some(NodeUpdate::Moved | NodeUpdate::Layer), ChildPosition::Anew(read)) => {
                let Some(position) = read.or_else(|| placed_child_position(self.applier, child))
                else {
                    return false;
                };
                Some(position)
            }
            (Some(NodeUpdate::Moved), ChildPosition::AsBefore) => {
                let Some(position) = placed_child_position(self.applier, child) else {
                    return false;
                };
                Some(position)
            }
            (Some(_), _) => return false,
        };
        let own_layer = match update {
            Some(NodeUpdate::Layer) => OwnLayer::Changed,
            _ => OwnLayer::AsLowered,
        };
        // A whole subtree whose nodes publish no window geometry pushes the
        // same with no place in the window.
        let lean;
        let lower_context = match children {
            HeadChildren::Whole if !holds_sinks(&scene.subtrees, at) => {
                lean = lower_context.without_window_origin();
                &lean
            }
            _ => lower_context,
        };
        self.push_head(
            (child, at),
            (position, own_layer, children),
            lower_context,
            (scene, context),
        )
    }

    // Pushes under `context` the subtree of `child` over its old subtree at
    // the record `at`, from the head the record keeps, placed at `position`
    // in its parent's content when the parent placed it anew, with the
    // layer the child has now when `own_layer` changed, and its children as
    // `children` says. False, with nothing pushed, when the record keeps no
    // head or the head does not follow the child.
    pub(super) fn push_head(
        &self,
        (child, at): (NodeId, usize),
        (position, own_layer, children): (Option<Point>, OwnLayer, HeadChildren),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
    ) -> bool {
        let Some(mut head) = scene.take_head(at) else {
            return false;
        };
        let index = scene.next_record();
        match head.follow(self.applier, child, lower_context, (position, own_layer)) {
            // A node drawn in its parent's layer has no children.
            Some(KeptNodeRef::Inline(inline)) => {
                populate_draws_from_graph(Subtree::Inline(inline), scene, context, &mut None, None);
            }
            Some(KeptNodeRef::Layer(layer, inner)) => {
                let old = OldChildren::of(&scene.subtrees, at, child);
                let pushes = match children {
                    HeadChildren::Records => ChildPushes::Records(old),
                    HeadChildren::Whole => {
                        scene.begin_whole();
                        ChildPushes::Whole(old)
                    }
                };
                let mut slot = ChildSlot {
                    walk: self,
                    owner: child,
                    children: &[],
                    context: &inner,
                    pushes,
                };
                let slot = old.holds_children(&scene.subtrees).then_some(&mut slot);
                populate_draws_from_graph(Subtree::Layer(layer), scene, context, &mut None, slot);
            }
            None => {
                head.recycle();
                return false;
            }
        }
        scene.keep_head(index, head);
        true
    }

    // Lowers `child` again under `lower_context` and pushes its subtree under
    // `context` over its old one, whose record is `at`.
    fn push_over(
        &self,
        (child, at): (NodeId, usize),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
    ) {
        // The old head goes first: the recordings it holds may take the
        // node's new draws.
        scene.drop_head(at);
        let children = OldChildren::of(&scene.subtrees, at, child);
        self.push(
            (child, NodePlace::Child),
            lower_context,
            (scene, context),
            &mut None,
            Some(children),
        );
    }

    // Pushes under `context`, into the scene of its parent's own surface, the
    // subtree of `child`, which did not change since the update before, from
    // its subtree lowered whole: the surface's scene holds none of its old
    // records. False when the child may have changed or the scene keeps a 3D
    // space, whose subtrees the walk lowers as any other.
    pub(crate) fn push_clean_into_surface(
        &self,
        child: NodeId,
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
    ) -> bool {
        if !self.marks.from_update || !self.is_clean(child) || !self.holds_no_space() {
            return false;
        }
        self.push_kept_subtree((child, None), lower_context, (scene, context));
        true
    }

    // Pushes the subtree of `child`, which did not change, from the subtree
    // the walk kept for it, placed where it now lies, else lowered whole now,
    // and keeps it for the next updates. `position` is where the child lies
    // in its parent's content, when the walk read it.
    fn push_kept_subtree(
        &self,
        (child, position): (NodeId, Option<Point>),
        lower_context: &LowerContext,
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
    ) {
        // A child no longer placed pushes nothing.
        let Some(position) = position.or_else(|| placed_child_position(self.applier, child)) else {
            return;
        };
        let (at, mut kept) = self.moved.borrow_mut().take(child).unzip();
        if let Some(stale) =
            kept.take_if(|subtree| !subtree.follow(self.applier, position, lower_context))
        {
            stale.recycle();
        }
        let Some(subtree) =
            kept.or_else(|| lower_moved_subtree(self.applier, child, lower_context))
        else {
            return;
        };
        if let Some(node) = Subtree::of_child(subtree.root()) {
            populate_draws_from_graph(node, scene, context, &mut None, None);
        }
        self.moved.borrow_mut().keep(at, subtree);
    }

    // Whether the old subtree at the record `at` holds a subtree the scene
    // keeps whole for its 3D space.
    fn keeps_space(&self, records: &[SubtreeItems], at: usize) -> bool {
        if self.no_space {
            return false;
        }
        let spaces = self.spaces.borrow();
        !spaces.is_empty()
            && records[at..records[at].end]
                .iter()
                .any(|record| kept_space(&spaces, record.identity).is_some())
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

// Whether a node of the old subtree at the record `at` publishes where it
// lies in its window.
fn holds_sinks(records: &[SubtreeItems], at: usize) -> bool {
    records[at..records[at].end]
        .iter()
        .any(|record| record.sinks)
}

// What changed of a node that an update pushes from its head.
#[derive(Clone, Copy)]
pub(crate) enum HeadChange {
    // Neither the node nor a node beneath it: its whole subtree pushes
    // again in its old order.
    Nothing,
    // The node or a node beneath it, with the node's own update, if any.
    Marked(Option<NodeUpdate>),
}

impl HeadChange {
    // What the node with `mark` changed.
    pub(crate) fn of(mark: u8) -> Self {
        match mark {
            0 => Self::Nothing,
            mark => Self::Marked(decode(mark)),
        }
    }
}

// How the children of a node pushed from its head push.
#[derive(Clone, Copy)]
pub(super) enum HeadChildren {
    // From the heads their records keep, each as its mark says.
    Records,
    // Whole, each over its old places in its old order.
    Whole,
}
