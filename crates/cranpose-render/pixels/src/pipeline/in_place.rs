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
    graph::{ProjectiveTransform, RenderGraph, for_each_in_draw_order},
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowLight,
    scene_builder::{
        LowerContext, NodePlace, RecordingStore, release_lowered, root_layer_bounds,
        visit_kept_node,
    },
};

use super::{ContentStyle, RasterTraversalContext, populate_draws_from_graph, scene_light};
use crate::{
    damage::merge_areas,
    draw::{ClipBounds, TextRaster},
    layout_walk::{LayoutWalk, OldChildren, child_record_from, first_child_record, names},
    rewrite::Rewrite,
    scene::{RasterScene, Subtree},
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

// What the walk above a subtree it pushes again passed down: what it did
// before, as the nodes it came through kept it, or unknown, along a graph.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Above {
    Kept,
    Unknown,
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
    /// Brings the scene, the one `graph` gave last at `scale`, up to `graph`
    /// and puts into `areas` the parts of `window` it changed. `None` when
    /// the scale, the light or the root layer changed, with the scene as it
    /// was, or when a subtree's records lie apart, with the areas the layers
    /// it updated before changed: the caller builds the scene again.
    pub(crate) fn run(
        self,
        graph: &RenderGraph,
        diagnostics: &RenderDiagnostics,
        scale: f32,
        text: &TextRaster,
        window: ClipBounds,
        areas: &mut Vec<ClipBounds>,
    ) -> Option<UpdatedReach> {
        let InPlaceUpdate {
            scene,
            reach,
            rewrite,
        } = self;
        areas.clear();
        let light = scene_light(graph, scale);
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
        let walked = repush.visit(Subtree::Layer(&graph.root), 0, None);
        let held = repush.held;
        merge_areas(areas);
        walked.ok()?;
        Some(held)
    }
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
        if walk.changed(node) {
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
        self.rewrite_subtree((record, parent), Above::Kept, |scene, context| {
            walk.push_node((node, place), lower_context, (scene, context), old);
        })
    }

    // Pushes a subtree with `push` over its old subtree, whose record is
    // `record`, from what its parent passed down in the scene, the root's
    // start for `None`, under a walk above that passed down what it did
    // before as `above` says, and adds the areas it changes.
    fn rewrite_subtree(
        &mut self,
        (record, parent): (usize, Option<usize>),
        above: Above,
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
            same_as_before: above == Above::Kept,
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
        let (reach, held, areas) = rewrite.results();
        (*self.reach, self.held) = (reach, held);
        self.areas.append(areas);
        *self.rewrite = Some(rewrite);
        Ok(())
    }

    /// Walks `subtree`, whose old record is `record` and whose parent passed
    /// down what the scene holds at `parent`, the root's start for `None`.
    fn visit(
        &mut self,
        subtree: Subtree<'_>,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Stop> {
        let old = self.scene.subtrees.get(record).ok_or(Stop::OtherLayer)?;
        if old.identity != subtree.identity() {
            return Err(Stop::OtherLayer);
        }
        let placed_as_before = old.transform == subtree.transform_to_parent();
        if old.change == subtree.change() && placed_as_before {
            return Ok(());
        }
        // A node drawn inline has no records beneath its own.
        let Subtree::Layer(layer) = subtree else {
            return self.push_again(subtree, record, parent);
        };
        // A layer that changed itself, moved, or drew its subtree without
        // records of its children, as a surface or nothing, pushes it all.
        if old.own_change != layer.own_change || !placed_as_before || old.end == record + 1 {
            return self.push_again(subtree, record, parent);
        }
        let Some(passed) = old.passed_down else {
            return self.push_again(subtree, record, parent);
        };
        let mut next = record + 1;
        let mut walked = Ok(());
        for_each_in_draw_order(&layer.children, |child| {
            if walked.is_ok()
                && let Some(child) = Subtree::of_child(child)
            {
                walked = self.visit(child, next, Some(passed));
                next = self
                    .scene
                    .subtrees
                    .get(next)
                    .map_or(usize::MAX, |at| at.end);
            }
        });
        match walked {
            Err(Stop::Rebuild) => Err(Stop::Rebuild),
            // A child pushed again with more or fewer records moved where
            // the layer ends.
            Ok(()) if next == self.scene.subtrees[record].end => Ok(()),
            // The child layers no longer match the records: the layer pushes
            // its whole subtree again in place of what they pushed.
            Ok(()) | Err(Stop::OtherLayer) => self.push_again(subtree, record, parent),
        }
    }

    /// Pushes `subtree` again over its old subtree, whose record is
    /// `record`, as the full walk would, from what its parent passed down in
    /// the scene, and adds the areas it changes.
    fn push_again(
        &mut self,
        subtree: Subtree<'_>,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Stop> {
        self.rewrite_subtree((record, parent), Above::Unknown, |scene, context| {
            populate_draws_from_graph(subtree, scene, context, &mut None, None);
        })
    }
}
