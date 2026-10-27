//! An update of the scene a picture holds, in place: the changed layers push
//! their subtrees again, their items take the old ones' places, the rest of
//! the scene stays as it was, and the areas to draw again come from the
//! replaced items alone. A subtree that pushes as many items as before
//! replaces the old one where it lies; one that pushes more or fewer is
//! spliced in, and what follows it moves. A layer whose child layers no
//! longer match its records pushes its whole subtree again. The walk to the
//! changed layers reads one record a layer and pushes nothing for the layers
//! above them. Each subtree takes its place as soon as it is pushed, so the
//! scratch scene holds one subtree at a time.

use cranpose_render_common::{
    graph::{LayerNode, ProjectiveTransform, RenderGraph, RenderNode, for_each_in_draw_order},
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowLight,
};

use super::{ContentStyle, RasterTraversalContext, populate_draws_from_graph, scene_light};
use crate::{
    damage::{RunReach, add_changed_runs, merge_areas},
    draw::{ClipBounds, TextRaster},
    scene::{RasterScene, SubtreeExtent},
};

// What a walk that pushes changed layers again shares.
struct Repush<'a> {
    scene: &'a mut RasterScene,
    reach: &'a mut [Option<ClipBounds>],
    // Whether `reach` still holds where each item of `scene` reaches: a
    // splice moves the items after it.
    held: bool,
    scratch: &'a mut RasterScene,
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
pub(crate) enum UpdatedReach {
    // It does: the update put the reach of each item it changed in its
    // item's place.
    Held,
    // It does not: a spliced subtree moved the items after it.
    Lost,
}

// A scene to bring up to its graph in place, where each of its items
// reaches, and the storage the update borrows: a scene for the layer it
// pushes again.
pub(crate) struct InPlaceUpdate<'a> {
    pub scene: &'a mut RasterScene,
    pub reach: &'a mut [Option<ClipBounds>],
    pub scratch: &'a mut RasterScene,
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
            scratch,
        } = self;
        areas.clear();
        let light = scene_light(graph, scale);
        if scene.start != Some((scale, light)) {
            return None;
        }
        scratch.clear();
        let held = reach.len() == scene.order.len();
        let mut repush = Repush {
            scene,
            reach,
            held,
            scratch,
            window,
            areas,
            diagnostics,
            light,
            scale,
            text,
        };
        let walked = repush.visit(&graph.root, 0, None);
        let held = repush.held;
        merge_areas(areas);
        walked.ok()?;
        Some(if held {
            UpdatedReach::Held
        } else {
            UpdatedReach::Lost
        })
    }
}

impl Repush<'_> {
    /// Walks `layer`, whose old record is `record` and whose parent passed
    /// down what the scene holds at `parent`, the root's start for `None`.
    fn visit(
        &mut self,
        layer: &LayerNode,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Stop> {
        let old = self.scene.subtrees.get(record).ok_or(Stop::OtherLayer)?;
        if old.identity != (layer.node_id, layer.wraps) {
            return Err(Stop::OtherLayer);
        }
        let placed_as_before = old.transform == layer.transform_to_parent;
        if old.change == layer.change && placed_as_before {
            return Ok(());
        }
        // A layer that changed itself, moved, or drew its subtree without
        // records of its children, as a surface or nothing, pushes it all.
        if old.own_change != layer.own_change || !placed_as_before || old.end == record + 1 {
            return self.push_again(layer, record, parent);
        }
        let Some(passed) = old.passed_down else {
            return self.push_again(layer, record, parent);
        };
        let mut next = record + 1;
        let mut walked = Ok(());
        for_each_in_draw_order(&layer.children, |child| {
            if walked.is_ok()
                && let RenderNode::Layer(child) = child
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
            Ok(()) | Err(Stop::OtherLayer) => self.push_again(layer, record, parent),
        }
    }

    /// Pushes `layer`'s subtree into the scratch scene as the full walk
    /// would, from what its parent passed down in the scene, adds the areas
    /// it changes and puts it in place of its old subtree, `record`.
    fn push_again(
        &mut self,
        layer: &LayerNode,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Stop> {
        let old_extent = self.scene.subtree_extent(record).ok_or(Stop::Rebuild)?;
        let passed = parent.and_then(|at| self.scene.passed_down.get(at));
        let transform = passed.map_or_else(
            || ProjectiveTransform::uniform_scale(self.scale),
            |passed| passed.transform,
        );
        let style = passed.map_or(ContentStyle::PLAIN, |passed| ContentStyle {
            alpha: passed.alpha,
            color_filter: passed.color_filter.clone(),
        });
        self.scratch.start = self.scene.start;
        populate_draws_from_graph(
            layer,
            self.scratch,
            &RasterTraversalContext {
                parent_transform: &transform,
                parent_content_style: &style,
                parent_visual_clip: passed.and_then(|passed| passed.clip),
                inherited_translated_snap_anchor: passed.and_then(|passed| passed.snap_anchor),
                inherited_translated_content_context: passed
                    .is_some_and(|passed| passed.translated),
                diagnostics: self.diagnostics,
                light: self.light,
                same_as_before: false,
                text: self.text,
                surface_root: false,
                // A scene draws its surfaces out of the graph's order, so the
                // walk to a changed layer stops at a scene's first child in
                // 3D: the layers it pushes again lie outside every space.
                in_space: false,
            },
            &mut None,
        );
        self.put_in_place(record, old_extent)
    }

    // Adds the areas the subtree the scratch scene holds changes and puts
    // it in place of the old subtree, `record`, of `old_extent`. Out of
    // line, so the walk that pushes stays small where the boards keep it in
    // fast RAM.
    #[inline(never)]
    fn put_in_place(&mut self, record: usize, old_extent: SubtreeExtent) -> Result<(), Stop> {
        let new_extent = self.scratch.subtree_extent(0).ok_or(Stop::Rebuild)?;
        let replaces = old_extent == new_extent;
        let (scene, scratch) = (&mut *self.scene, &mut *self.scratch);
        let (old, new) = (&scene.subtrees[record], &scratch.subtrees[0]);
        let order = old.start.order..old.stop.order;
        let reach = match self.reach.get_mut(order.clone()) {
            Some(run) if self.held && replaces => RunReach::Replaced(run),
            Some(run) if self.held => RunReach::Kept(run),
            _ => RunReach::Unknown,
        };
        add_changed_runs(
            (scene, &scene.order[order]),
            (scratch, &scratch.order[new.start.order..new.stop.order]),
            reach,
            self.window,
            self.areas,
        );
        if replaces {
            scene.replace_subtree(record, scratch, 0);
        } else {
            scene.splice_subtree(record, scratch, 0);
            self.held = false;
        }
        scratch.clear();
        Ok(())
    }
}
