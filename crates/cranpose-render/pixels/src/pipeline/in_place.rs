//! An update of the scene a picture holds, in place: the changed layers push
//! their subtrees again, their items take the old ones' places, the rest of
//! the scene stays as it was, and the areas to draw again come from the
//! replaced items alone. A subtree that pushes the items it pushed before,
//! in kind and order, writes each over the old one as it pushes it; from an
//! item that differs on, it pushes into a spill scene that is spliced in,
//! and what follows it moves. A layer whose child layers no longer match
//! its records pushes its whole subtree again. The walk to the changed
//! layers reads one record a layer and pushes nothing for the layers above
//! them.

use cranpose_core::NodeId;
use cranpose_render_common::{
    NodeUpdate, SceneUpdates,
    graph::RenderNode,
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowLight,
    scene_builder::{
        LowerContext, NodePlace, RecordingStore, release_lowered, root_layer_bounds,
        visit_kept_node,
    },
};

use super::{ContentStyle, RasterTraversalContext};
use crate::{
    damage::merge_areas,
    draw::{ClipBounds, TextRaster},
    layout_walk::{ChildPosition, LayoutWalk, OldChildren, child_record_from, names},
    raster_scene::{PassedDown, RasterScene},
    rewrite::{Replaced, Rewrite},
};

// What a walk that pushes changed layers again shares.
struct Repush<'a> {
    scene: &'a mut RasterScene,
    reach: &'a mut Vec<Option<ClipBounds>>,
    // Whether `reach` still holds where each item of `scene` reaches: a
    // splice moves the items after it.
    held: UpdatedReach,
    rewrite: &'a mut Option<Box<Rewrite>>,
    window: ClipBounds,
    areas: &'a mut Vec<ClipBounds>,
    // Where the update's own areas start in `areas`.
    first_area: usize,
    diagnostics: &'a RenderDiagnostics,
    light: ShadowLight,
    scale: f32,
    text: &'a TextRaster,
    targets: &'a mut Vec<PushTarget>,
    // Whether a rewrite spliced a run of the scene's lists.
    replaced: Replaced,
}

// The nodes the last update in place pushed again, and the updates its walk
// found them for: empty when no walk's nodes hold. An update with the same
// updates finds the same nodes at the same records, with what they lower
// with, while no rewrite spliced the scene since and no subtree holds a 3D
// space: it pushes them again with no walk. An animation changes the same
// nodes each frame.
#[derive(Default)]
pub(crate) struct WalkedTargets {
    updates: Vec<(NodeId, NodeUpdate)>,
    targets: Vec<PushTarget>,
}

impl WalkedTargets {
    pub(crate) fn clear(&mut self) {
        self.updates.clear();
        self.targets.clear();
    }
}

// A node whose subtree an update pushes again: where it lies in the layout
// tree and in the scene, what it lowers with, whether it lowers as a leaf,
// and once the update lowered it, its layer and what its children lower
// with when it has children.
pub(crate) struct PushTarget {
    node: NodeId,
    place: NodePlace,
    context: LowerContext,
    record: usize,
    parent: Option<usize>,
    leaf: bool,
    layer: Option<RenderNode>,
    inner: Option<LowerContext>,
}

// Why a walk to a layer stopped.
enum Stop {
    // The record holds another layer: the parent pushes its subtree again.
    OtherLayer,
    // A subtree's entries of what its layers passed down lie apart: the
    // scene is built again.
    Rebuild,
}

// Whether the scene's reach list still holds where each item reaches.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum UpdatedReach {
    // It does: the update put the reach of each item it changed in its
    // item's place.
    Held,
    // It does not: a spliced subtree moved the items after it.
    #[default]
    Lost,
}

// A scene to bring up to its layout tree in place by `updates`, where each
// of its items reaches, and the storage the update borrows: the rewrite that
// pushes a changed subtree again.
pub(crate) struct InPlaceUpdate<'a> {
    pub scene: &'a mut RasterScene,
    pub reach: &'a mut Vec<Option<ClipBounds>>,
    pub rewrite: &'a mut Option<Box<Rewrite>>,
    pub updates: SceneUpdates<'a>,
}

impl InPlaceUpdate<'_> {
    // Brings the scene, the one `walk`'s window gave last at `scale`, up to
    // the window's layout tree from its root `root`, and adds to `areas`
    // the parts of `window` it changed, merged with those `areas` held.
    // `None` when the scene must be built again.
    pub(crate) fn run_layout(
        self,
        (walk, root): (&LayoutWalk<'_>, NodeId),
        diagnostics: &RenderDiagnostics,
        (scale, text): (f32, &TextRaster),
        window: ClipBounds,
        areas: &mut Vec<ClipBounds>,
    ) -> Option<UpdatedReach> {
        let InPlaceUpdate {
            scene,
            reach,
            rewrite,
            updates,
        } = self;
        let bounds = root_layer_bounds(walk.applier(), root)?;
        let light =
            ShadowLight::for_window(bounds.width * scale, bounds.height * scale, scale, 1.0);
        if scene.start != Some((scale, light)) {
            return None;
        }
        let held = if reach.len() == scene.order.len() {
            UpdatedReach::Held
        } else {
            UpdatedReach::Lost
        };
        let mut walked = std::mem::take(&mut scene.walked);
        let found_before = walked.updates == updates && walk.holds_no_space();
        let mut repush = Repush {
            scene,
            reach,
            held,
            rewrite,
            window,
            first_area: areas.len(),
            areas,
            diagnostics,
            light,
            scale,
            text,
            targets: &mut walked.targets,
            replaced: Replaced::Nothing,
        };
        let found = if found_before && repush.targets_hold(walk) {
            Ok(())
        } else {
            repush.targets.clear();
            let context = LowerContext::root(RecordingStore::Redrawn);
            repush.visit_node(walk, (root, NodePlace::Root), &context, (0, None))
        };
        let pushed = found.and_then(|()| repush.push_targets(walk));
        // The rewrites run last first. Each puts its areas in reverse after
        // those before. One more reverse puts the update's areas in the
        // scene's order, in which a host sends them.
        repush.areas[repush.first_area..].reverse();
        let (held, replaced) = (repush.held, repush.replaced);
        if pushed.is_ok() && replaced == Replaced::Nothing && walk.holds_no_space() {
            if !found_before {
                walked.updates.clear();
                walked.updates.extend_from_slice(updates);
            }
        } else {
            walked.clear();
        }
        scene.walked = walked;
        release_lowered();
        merge_areas(areas);
        pushed.ok()?;
        Some(held)
    }
}

impl Repush<'_> {
    // Walks the layout node `node` at `place`, lowered under `context`,
    // whose old record is `record` and whose parent passed down what the
    // scene holds at `parent`, the root's start for `None`: to the nodes
    // the update changed, which push their subtrees again.
    fn visit_node(
        &mut self,
        walk: &LayoutWalk<'_>,
        (node, place): (NodeId, NodePlace),
        context: &LowerContext,
        (record, parent): (usize, Option<usize>),
    ) -> Result<(), Stop> {
        let old = self.scene.subtrees.get(record).ok_or(Stop::OtherLayer)?;
        if !names(old.identity, node) {
            return Err(Stop::OtherLayer);
        }
        // A subtree the scene keeps whole for its 3D space lowers whole.
        if walk.changed(node) || walk.holds_space(node) {
            self.defer(walk, (node, place), context, (record, parent));
            return Ok(());
        }
        // A node that wraps its outer draws holds its children in its own
        // layer inside the wrapper.
        let own = OldChildren::own_record(&self.scene.subtrees, record, node);
        let Some(passed) = self.scene.subtrees[own].passed_down else {
            self.defer(walk, (node, place), context, (record, parent));
            return Ok(());
        };
        let deferred = self.targets.len();
        let walked = visit_kept_node(walk.applier(), (node, place), context, |children, inner| {
            let end = self.scene.subtrees[own].end;
            let mut cursor = child_record_from(&self.scene.subtrees, own + 1, end);
            for &child in children {
                let at = OldChildren::find(&self.scene.subtrees, (cursor, end), child)
                    .ok_or(Stop::OtherLayer)?;
                if !walk.is_clean(child) {
                    self.visit_node(walk, (child, NodePlace::Child), inner, (at, Some(passed)))?;
                }
                cursor = self.scene.subtrees[at].end;
            }
            // Past the last child only the node's own draws may follow.
            if child_record_from(&self.scene.subtrees, cursor, end) == end {
                Ok(())
            } else {
                Err(Stop::OtherLayer)
            }
        })
        .unwrap_or(Err(Stop::OtherLayer));
        if let Err(Stop::OtherLayer) = walked {
            // The node's subtree is pushed again whole, its children's with
            // it.
            self.targets.truncate(deferred);
            self.defer(walk, (node, place), context, (record, parent));
            return Ok(());
        }
        walked
    }

    // Whether the nodes the walk found for the same updates before still
    // lie at their records, readied to lower again.
    fn targets_hold(&mut self, walk: &LayoutWalk<'_>) -> bool {
        self.targets.iter_mut().all(|target| {
            target.leaf = walk.lowers_as_leaf((target.node, target.place));
            target.inner = None;
            self.scene
                .subtrees
                .get(target.record)
                .is_some_and(|old| names(old.identity, target.node))
        })
    }

    // Notes that the subtree of `node`, whose old record is `record`, is
    // pushed again, as `visit_node` says.
    fn defer(
        &mut self,
        walk: &LayoutWalk<'_>,
        (node, place): (NodeId, NodePlace),
        context: &LowerContext,
        (record, parent): (usize, Option<usize>),
    ) {
        self.targets.push(PushTarget {
            node,
            place,
            context: *context,
            record,
            parent,
            leaf: walk.lowers_as_leaf((node, place)),
            layer: None,
            inner: None,
        });
    }

    // Lowers every node the visit found changed, then pushes their subtrees
    // again, the last first: a rewrite moves only the records and entries
    // after its subtree's, so the targets before it lie where the visit
    // found them.
    fn push_targets(&mut self, walk: &LayoutWalk<'_>) -> Result<(), Stop> {
        let mut targets = std::mem::take(self.targets);
        // The nodes with children lower before the leaves: the two kinds run
        // different lowering code, and each would evict the other's from a
        // small instruction cache between targets.
        for leaves in [false, true] {
            for target in targets.iter_mut().filter(|target| target.leaf == leaves) {
                // A child that only moved pushes again from the head its
                // record keeps.
                if walk.moves_with_head(
                    (target.node, target.place),
                    &self.scene.subtrees[target.record],
                ) {
                    continue;
                }
                let lowered = (&mut target.layer, &mut target.inner);
                walk.lower((target.node, target.place), &target.context, lowered);
            }
        }
        let pushed = targets
            .iter_mut()
            .rev()
            .try_for_each(|target| self.push_node_again(walk, target));
        *self.targets = targets;
        pushed
    }

    // Pushes the subtree of `target` over its old one, keeping the subtrees
    // of its children that did not change, from what its parent passed down
    // in the scene, and adds the areas it changes.
    fn push_node_again(
        &mut self,
        walk: &LayoutWalk<'_>,
        target: &mut PushTarget,
    ) -> Result<(), Stop> {
        let (node, place) = (target.node, target.place);
        let old = OldChildren::of(&self.scene.subtrees, target.record, node);
        self.rewrite_subtree(
            (target.record, target.parent),
            |scene, context| match target.layer.take() {
                Some(layer) => {
                    let lowered = (layer, target.inner.as_ref());
                    let lowered_with = (&target.context, Some(old));
                    walk.push_lowered((node, place), lowered, (scene, context), lowered_with);
                }
                None => {
                    let moved = (node, target.record, ChildPosition::AsBefore);
                    if !(place == NodePlace::Child
                        && walk.push_moved(moved, &target.context, (scene, context)))
                    {
                        walk.push(
                            (node, place),
                            &target.context,
                            (scene, context),
                            &mut None,
                            Some(old),
                        );
                    }
                }
            },
        )
    }

    // Pushes a subtree with `push` over its old subtree, whose record is
    // `record`, from what its parent passed down in the scene, the root's
    // start for `None`, under a walk above that passed down what it did
    // before, as the nodes the walk came through kept it, and adds the areas
    // it changes. Out of line: the classic ESP32's 64 KB list of hot code
    // takes a function whole. The update with this rewrite inline would be
    // one function of 9 KB.
    #[inline(never)]
    fn rewrite_subtree(
        &mut self,
        (record, parent): (usize, Option<usize>),
        push: impl FnOnce(&mut RasterScene, &RasterTraversalContext<'_>),
    ) -> Result<(), Stop> {
        let passed = parent
            .and_then(|at| self.scene.passed_down.get(at))
            .cloned()
            .unwrap_or_else(|| PassedDown::root(self.scale));
        let style = ContentStyle::of(&passed);
        // A scene draws its surfaces out of the graph's order, so the walk to
        // a changed layer stops at a scene's first child in 3D: the layers it
        // pushes again lie outside every space.
        let context = RasterTraversalContext::beneath(
            (&passed, &style),
            (self.diagnostics, self.text),
            self.light,
        );
        let mut rewrite = self.rewrite.take().unwrap_or_default();
        if !rewrite.start(self.scene, record, self.window, self.reach, self.held) {
            *self.rewrite = Some(rewrite);
            return Err(Stop::Rebuild);
        }
        self.scene.rewrite = Some(rewrite);
        push(self.scene, &context);
        let mut rewrite = self
            .scene
            .rewrite
            .take()
            .expect("the rewrite started above");
        if rewrite.finish(self.scene) == Replaced::Run {
            self.replaced = Replaced::Run;
        }
        self.scene.hits_changed |= rewrite.replaced_hits();
        (*self.reach, self.held) = rewrite.results(self.areas);
        *self.rewrite = Some(rewrite);
        Ok(())
    }
}
