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
    graph::{ProjectiveTransform, RenderNode},
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowLight,
    scene_builder::{LowerContext, NodePlace, RecordingStore, root_layer_bounds, visit_kept_node},
};

use super::{ContentStyle, RasterTraversalContext};
use crate::{
    damage::merge_areas,
    draw::{ClipBounds, TextRaster},
    layout_walk::{LayoutWalk, OldChildren, child_record_from, first_child_record, names},
    raster_scene::RasterScene,
    rewrite::Rewrite,
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

// A scene to bring up to its graph in place, where each of its items
// reaches, and the storage the update borrows: the rewrite that pushes a
// changed subtree again.
pub(crate) struct InPlaceUpdate<'a> {
    pub scene: &'a mut RasterScene,
    pub reach: &'a mut Vec<Option<ClipBounds>>,
    pub rewrite: &'a mut Option<Box<Rewrite>>,
    pub targets: &'a mut Vec<PushTarget>,
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
            targets,
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
            targets,
        };
        let context = LowerContext::root(RecordingStore::Redrawn);
        let walked = repush
            .visit_node(walk, (root, NodePlace::Root), &context, (0, None))
            .and_then(|()| repush.push_targets(walk));
        // The rewrites ran last first, and each put its areas in reverse
        // after those before: in reverse once more, the update's areas
        // follow the scene's order, in which a host sends them.
        repush.areas[repush.first_area..].reverse();
        repush.targets.clear();
        walk.release();
        let held = repush.held;
        merge_areas(areas);
        walked.ok()?;
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
            let mut cursor =
                first_child_record(&self.scene.subtrees, own, self.scene.subtrees[own].end);
            let end = self.scene.subtrees[own].end;
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
        match walked {
            Err(Stop::Rebuild) => Err(Stop::Rebuild),
            Ok(()) => Ok(()),
            Err(Stop::OtherLayer) => {
                // The node's subtree is pushed again whole, its children's
                // with it.
                self.targets.truncate(deferred);
                self.defer(walk, (node, place), context, (record, parent));
                Ok(())
            }
        }
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
                    walk.push_lowered(node, lowered, (scene, context), Some(old));
                }
                None => walk.push(
                    (node, place),
                    &target.context,
                    (scene, context),
                    &mut None,
                    Some(old),
                ),
            },
        )
    }

    // Pushes a subtree with `push` over its old subtree, whose record is
    // `record`, from what its parent passed down in the scene, the root's
    // start for `None`, under a walk above that passed down what it did
    // before, as the nodes the walk came through kept it, and adds the areas
    // it changes. Out of line: inlined into the update, it made one function
    // of 9 KB that the classic ESP32's 64 KB list of hot code took whole.
    #[inline(never)]
    fn rewrite_subtree(
        &mut self,
        (record, parent): (usize, Option<usize>),
        push: impl FnOnce(&mut RasterScene, &RasterTraversalContext<'_>),
    ) -> Result<(), Stop> {
        let passed = parent
            .and_then(|at| self.scene.passed_down.get(at))
            .cloned();
        let transform = passed.as_ref().map_or_else(
            || ProjectiveTransform::uniform_scale(self.scale),
            |passed| passed.transform,
        );
        let style = passed
            .as_ref()
            .map_or(ContentStyle::PLAIN, |passed| ContentStyle {
                alpha: passed.alpha,
                color_filter: passed.color_filter.clone(),
            });
        let context = RasterTraversalContext {
            parent_transform: &transform,
            parent_content_style: &style,
            parent_visual_clip: passed.as_ref().and_then(|passed| passed.clip),
            inherited_translated_snap_anchor: passed.as_ref().and_then(|passed| passed.snap_anchor),
            inherited_translated_content_context: passed
                .as_ref()
                .is_some_and(|passed| passed.translated),
            diagnostics: self.diagnostics,
            light: self.light,
            same_as_before: true,
            text: self.text,
            surface_root: false,
            // A scene draws its surfaces out of the graph's order, so the
            // walk to a changed layer stops at a scene's first child in
            // 3D: the layers it pushes again lie outside every space.
            in_space: false,
        };
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
        rewrite.finish(self.scene, record);
        self.scene.hits_changed |= rewrite.replaced_hits();
        (*self.reach, self.held) = rewrite.results(self.areas);
        *self.rewrite = Some(rewrite);
        Ok(())
    }
}
