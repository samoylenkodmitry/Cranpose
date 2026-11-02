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
    graph::ProjectiveTransform,
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
    diagnostics: &'a RenderDiagnostics,
    light: ShadowLight,
    scale: f32,
    text: &'a TextRaster,
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
}

impl InPlaceUpdate<'_> {
    // Brings the scene, the one `walk`'s window gave last at `scale`, up to
    // the window's layout tree from its root `root`, and puts into `areas`
    // the parts of `window` it changed. `None` as for [`Self::run`].
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
        } = self;
        areas.clear();
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
            areas,
            diagnostics,
            light,
            scale,
            text,
        };
        let context = LowerContext::root(RecordingStore::Shared);
        let walked = repush.visit_node(walk, (root, NodePlace::Root), context, (0, None));
        release_lowered();
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
        context: LowerContext,
        (record, parent): (usize, Option<usize>),
    ) -> Result<(), Stop> {
        let old = self.scene.subtrees.get(record).ok_or(Stop::OtherLayer)?;
        if !names(old.identity, node) {
            return Err(Stop::OtherLayer);
        }
        // A subtree the scene keeps whole for its 3D space lowers whole.
        if walk.changed(node) || walk.holds_space(node) {
            return self.push_node_again(walk, (node, place), context, (record, parent));
        }
        // A node that wraps its outer draws holds its children in its own
        // layer inside the wrapper.
        let own = OldChildren::own_record(&self.scene.subtrees, record, node);
        let Some(passed) = self.scene.subtrees[own].passed_down else {
            return self.push_node_again(walk, (node, place), context, (record, parent));
        };
        let walked = visit_kept_node(walk.applier(), (node, place), context, |children, inner| {
            let mut cursor =
                first_child_record(&self.scene.subtrees, own, self.scene.subtrees[own].end);
            for &child in children {
                // A child pushed again with more or fewer records moved
                // where the node's subtree ends.
                let end = self.scene.subtrees[own].end;
                let at = OldChildren::find(&self.scene.subtrees, (cursor, end), child)
                    .ok_or(Stop::OtherLayer)?;
                if !walk.is_clean(child) {
                    self.visit_node(walk, (child, NodePlace::Child), inner, (at, Some(passed)))?;
                }
                cursor = self.scene.subtrees[at].end;
            }
            // Past the last child only the node's own draws may follow.
            let end = self.scene.subtrees[own].end;
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
                self.push_node_again(walk, (node, place), context, (record, parent))
            }
        }
    }

    // Lowers `node` again and pushes its subtree over its old one, whose
    // record is `record`, keeping the subtrees of its children that did not
    // change, from what its parent passed down in the scene, and adds the
    // areas it changes.
    fn push_node_again(
        &mut self,
        walk: &LayoutWalk<'_>,
        (node, place): (NodeId, NodePlace),
        lower_context: LowerContext,
        (record, parent): (usize, Option<usize>),
    ) -> Result<(), Stop> {
        let old = OldChildren::of(&self.scene.subtrees, record, node);
        self.rewrite_subtree((record, parent), |scene, context| {
            walk.push(
                (node, place),
                lower_context,
                (scene, context),
                &mut None,
                Some(old),
            );
        })
    }

    // Pushes a subtree with `push` over its old subtree, whose record is
    // `record`, from what its parent passed down in the scene, the root's
    // start for `None`, under a walk above that passed down what it did
    // before, as the nodes the walk came through kept it, and adds the areas
    // it changes.
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
        (*self.reach, self.held) = rewrite.results(self.areas);
        *self.rewrite = Some(rewrite);
        Ok(())
    }
}
