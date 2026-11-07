use cranpose_core::NodeId;
use cranpose_render_common::{
    NodeUpdate,
    scene_builder::{LowerContext, NodePlace, OwnLayer, kept_child_placement},
};
use cranpose_ui_graphics::Point;

use super::{HeadChange, HeadChildren, LayoutWalk, OldChildren, decode};
use crate::{
    pipeline::{RasterTraversalContext, Reuse},
    raster_scene::RasterScene,
};

// The layout children of a lowered node, which its own layer pushes between
// its draws before them and after them.
pub(crate) struct ChildSlot<'w> {
    pub(super) walk: &'w LayoutWalk<'w>,
    pub(super) owner: NodeId,
    pub(super) children: &'w [NodeId],
    pub(super) context: &'w LowerContext,
    pub(super) pushes: ChildPushes,
}

// Where an update finds a child that did not change itself in its parent's
// content.
#[derive(Clone, Copy)]
pub(crate) enum ChildPosition {
    // Where it lay: its parent placed its children as before.
    AsBefore,
    // Where its parent placed it anew, when the walk read it.
    Anew(Option<Point>),
}

// How a slot pushes its children.
#[derive(Clone, Copy)]
pub(super) enum ChildPushes {
    // Over their old records, at `OldChildren`, while a rewrite pushes the
    // owner over its old subtree.
    OverOld(OldChildren),
    // Over their old records, at `OldChildren`, from the heads they keep,
    // for an owner that did not change: its children are those its old
    // record holds.
    Records(OldChildren),
    // Over their old records, at `OldChildren`, from the heads they keep,
    // each child's whole subtree in its old order: neither the owner nor a
    // node beneath it changed.
    Whole(OldChildren),
    // Anew, into a scene that holds none of their old records.
    Anew,
    // Anew, into the scene of the owner's own surface: a child that did not
    // change pushes the subtree the walk keeps for it, lowered once.
    IntoSurface,
}

impl ChildPushes {
    pub(super) fn of(old: Option<OldChildren>) -> Self {
        old.map_or(Self::Anew, Self::OverOld)
    }
}

impl ChildSlot<'_> {
    // The node whose children these are.
    pub(crate) fn owner(&self) -> NodeId {
        self.owner
    }

    // Pushes the children anew, into the scene of the owner's own surface,
    // which holds none of their old records.
    pub(crate) fn for_surface(&mut self) -> &mut Self {
        self.pushes = ChildPushes::IntoSurface;
        self
    }

    // Whether the owner placed its children where it did before: so it did
    // when only state its draws read changed.
    fn placed_as_before(&self) -> bool {
        self.walk.marks.update_of(self.walk.applier, self.owner) == Some(NodeUpdate::Draws)
    }

    // Pushes the children's subtrees under `context`, copying from `reuse`'s
    // scene those whose nodes did not change and that lie where they did.
    // Out of line: a layer calls it once, from the code that pushes each of
    // its own draws.
    #[inline(never)]
    pub(crate) fn push_children(
        &mut self,
        scene: &mut RasterScene,
        context: &RasterTraversalContext<'_>,
        reuse: &mut Option<Reuse<'_>>,
    ) {
        let walk = self.walk;
        let into_surface = match self.pushes {
            ChildPushes::OverOld(old) => return self.push_over_old(old, scene, context),
            ChildPushes::Records(old) => return self.push_records(old, scene, context),
            ChildPushes::Whole(old) => return self.push_whole(old, scene, context),
            ChildPushes::Anew => false,
            ChildPushes::IntoSurface => true,
        };
        for &child in self.children {
            if let Some(kept) = reuse.as_mut()
                && walk.is_clean(child)
                && let Some(placement) = kept_child_placement(walk.applier, child, self.context)
                && kept.keep(scene, &placement, context)
            {
                continue;
            }
            if into_surface && walk.push_clean_into_surface(child, self.context, (scene, context)) {
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
        old: OldChildren,
        scene: &mut RasterScene,
        context: &RasterTraversalContext<'_>,
    ) {
        let walk = self.walk;
        let placed_as_before = self.placed_as_before();
        let mut cursor = (old, scene.records_moved());
        for &child in self.children {
            let (old, _) = cursor;
            let Some(found) = OldChildren::find(&scene.subtrees, (old.cursor, old.end), child)
            else {
                walk.push(
                    (child, NodePlace::Child),
                    self.context,
                    (scene, context),
                    &mut None,
                    None,
                );
                // A gap that closed while the child pushed moved the old
                // records the cursor holds.
                let (old, moved) = &mut cursor;
                let now = scene.records_moved();
                old.follow((*moved, now));
                *moved = now;
                continue;
            };
            self.push_found(
                (child, found),
                &mut cursor,
                (scene, context),
                |scene, at| {
                    let record = &scene.subtrees[at];
                    let mark = walk.marks.marks.get(walk.applier, child);
                    let clean = mark == 0;
                    let update = decode(mark);
                    let kept = update.is_none();
                    // A child whose owner placed it as before lies where it did.
                    let placement = (kept && context.same_as_before && !placed_as_before)
                        .then(|| kept_child_placement(walk.applier, child, self.context))
                        .flatten();
                    let lies_as_before = context.same_as_before
                        && (placed_as_before
                            || placement.is_some_and(|placement| {
                                (placement.identity, placement.transform)
                                    == (record.identity, record.transform)
                            }));
                    let position = if placed_as_before {
                        ChildPosition::AsBefore
                    } else {
                        ChildPosition::Anew(placement.map(|placement| placement.position))
                    };
                    (clean && lies_as_before && scene.keep_old(at))
                        || (kept
                            && lies_as_before
                            && walk.push_through((child, at), self.context, (scene, context)))
                        || (matches!(update, None | Some(NodeUpdate::Moved | NodeUpdate::Layer))
                            && walk.push_from_head(
                                (child, at, position),
                                (HeadChange::of(mark), self.context),
                                (scene, context),
                            ))
                },
            );
        }
    }

    // Pushes the children's subtrees under `context` over their old ones,
    // whose records lie at `old`, from the heads the records keep: the
    // owner did not change, so its children are those its record holds and
    // lie where they did in it. Keeps in the rewrite those whose nodes did
    // not change, under what the owner passed down before.
    fn push_records(
        &self,
        old: OldChildren,
        scene: &mut RasterScene,
        context: &RasterTraversalContext<'_>,
    ) {
        let walk = self.walk;
        let mut cursor = (old, scene.records_moved());
        while cursor.0.cursor < cursor.0.end {
            let found = cursor.0.cursor;
            let Some(child) = scene.subtrees[found].node() else {
                break;
            };
            self.push_found(
                (child, found),
                &mut cursor,
                (scene, context),
                |scene, at| {
                    let mark = walk.marks.marks.get(walk.applier, child);
                    (context.same_as_before && mark == 0 && scene.keep_old(at))
                        || walk.push_from_head(
                            (child, at, ChildPosition::AsBefore),
                            (HeadChange::of(mark), self.context),
                            (scene, context),
                        )
                },
            );
        }
    }

    // Pushes the children's subtrees under `context` over their old ones,
    // whose records lie at `old`, from the heads the records keep: neither
    // the owner nor a node beneath it changed, so its children are those
    // its record holds, each lies where it did, and each pushes its whole
    // subtree in its old order, over its old places. A child whose head
    // does not follow it lowers again.
    fn push_whole(
        &self,
        old: OldChildren,
        scene: &mut RasterScene,
        context: &RasterTraversalContext<'_>,
    ) {
        let walk = self.walk;
        let whole = (None, OwnLayer::AsLowered, HeadChildren::Whole);
        let (mut old, mut moved) = (old, scene.records_moved());
        let mut at = old.cursor;
        while at < old.end {
            let record = &scene.subtrees[at];
            let Some(child) = record.node() else {
                break;
            };
            old.cursor = record.end;
            if !walk.push_head((child, at), whole, self.context, (scene, context)) {
                walk.push(
                    (child, NodePlace::Child),
                    self.context,
                    (scene, context),
                    &mut None,
                    None,
                );
            }
            // A gap that closed while the child pushed moved the old
            // records after it.
            let now = scene.records_moved();
            old.follow((moved, now));
            moved = now;
            at = old.cursor;
        }
    }

    // Pushes `child`, whose old record the walk found at `found` among the
    // old children `cursor` holds, with how far the splices had moved the
    // old records when it read them: the children pushed anew since the
    // pushes last fit their old places, and a child that went away, give
    // way to the child's old subtree, then `push` pushes it from what the
    // scene keeps of it, given where its old record now lies, or it lowers
    // again. Moves the cursor past it.
    fn push_found(
        &self,
        (child, found): (NodeId, usize),
        cursor: &mut (OldChildren, isize),
        (scene, context): (&mut RasterScene, &RasterTraversalContext<'_>),
        push: impl FnOnce(&mut RasterScene, usize) -> bool,
    ) {
        let at = scene.rejoin(found);
        let end = scene.subtrees[at].end;
        if !push(scene, at) {
            self.walk
                .push_over((child, at), self.context, (scene, context));
        }
        let now = scene.records_moved();
        let (old, moved) = cursor;
        old.pass((found, at, end), (*moved, now));
        *moved = now;
    }
}
