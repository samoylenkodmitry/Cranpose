//! An update of the scene a picture holds, in place: when only some layers
//! changed and each pushes as many items as it did before, their items take
//! the old ones' places, the rest of the scene stays as it was, and the
//! areas to draw again come from the replaced items alone. The walk to the
//! changed layers reads one record a layer and pushes nothing for the layers
//! above them.

use cranpose_render_common::{
    graph::{LayerNode, ProjectiveTransform, RenderGraph, RenderNode, for_each_in_draw_order},
    graph_scene::RenderDiagnostics,
    layer_shadow::ShadowLight,
};

use super::{ContentStyle, RasterTraversalContext, populate_draws_from_graph, scene_light};
use crate::{
    damage::{add_changed_pairs, merge_areas},
    draw::{ClipBounds, TextRaster},
    scene::RasterScene,
};

/// What a walk that pushes changed layers again shares.
struct Repush<'a> {
    scene: &'a RasterScene,
    scratch: &'a mut RasterScene,
    /// For each layer pushed again, its record in `scene` and in `scratch`.
    pushed: &'a mut Vec<(usize, usize)>,
    diagnostics: &'a RenderDiagnostics,
    light: ShadowLight,
    scale: f32,
    text: &'a TextRaster,
}

/// A scene to bring up to its graph in place, and the storage the update
/// borrows: a scene for the layers it pushes again, and its list of them.
pub(crate) struct InPlaceUpdate<'a> {
    pub scene: &'a mut RasterScene,
    pub scratch: &'a mut RasterScene,
    pub pushed: &'a mut Vec<(usize, usize)>,
}

impl InPlaceUpdate<'_> {
    /// Brings the scene, the one `graph` gave last at `scale`, up to `graph`
    /// and puts into `areas` the parts of `window` it changed. Returns false,
    /// with the scene as it was, when a layer pushes a different number of
    /// items or the layers' shape changed: the caller builds the scene again.
    pub(crate) fn run(
        self,
        graph: &RenderGraph,
        diagnostics: &RenderDiagnostics,
        scale: f32,
        text: &TextRaster,
        window: ClipBounds,
        areas: &mut Vec<ClipBounds>,
    ) -> bool {
        let InPlaceUpdate {
            scene,
            scratch,
            pushed,
        } = self;
        update(
            scene,
            scratch,
            pushed,
            graph,
            (diagnostics, text),
            scale,
            window,
            areas,
        )
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "the parts of `InPlaceUpdate::run`, taken apart to borrow each alone"
)]
fn update(
    scene: &mut RasterScene,
    scratch: &mut RasterScene,
    pushed: &mut Vec<(usize, usize)>,
    graph: &RenderGraph,
    (diagnostics, text): (&RenderDiagnostics, &TextRaster),
    scale: f32,
    window: ClipBounds,
    areas: &mut Vec<ClipBounds>,
) -> bool {
    let light = scene_light(graph, scale);
    if scene.start != Some((scale, light)) {
        return false;
    }
    scratch.clear();
    scratch.start = scene.start;
    pushed.clear();
    let mut repush = Repush {
        scene,
        scratch,
        pushed,
        diagnostics,
        light,
        scale,
        text,
    };
    if repush.visit(&graph.root, 0, None).is_none() {
        return false;
    }
    let fits = pushed.iter().all(|&(old, new)| {
        scene
            .subtree_extent(old)
            .is_some_and(|extent| scratch.subtree_extent(new) == Some(extent))
    });
    if !fits {
        return false;
    }
    areas.clear();
    for &(old, new) in pushed.iter() {
        let (old_record, new_record) = (scene.subtrees[old], scratch.subtrees[new]);
        add_changed_pairs(
            scene,
            &scene.order[old_record.start.order..old_record.stop.order],
            scratch,
            &scratch.order[new_record.start.order..new_record.stop.order],
            window,
            areas,
        );
    }
    for &(old, new) in pushed.iter() {
        scene.replace_subtree(old, scratch, new);
    }
    merge_areas(areas);
    true
}

impl Repush<'_> {
    /// Walks `layer`, whose old record is `record` and whose parent passed
    /// down what the old scene holds at `parent`, the root's start for
    /// `None`. `None` when the graph's layers no longer match the records.
    fn visit(&mut self, layer: &LayerNode, record: usize, parent: Option<usize>) -> Option<()> {
        let old = *self.scene.subtrees.get(record)?;
        if old.identity != (layer.node_id, layer.wraps) {
            return None;
        }
        let placed_as_before = old.transform == layer.transform_to_parent;
        if old.change == layer.change && placed_as_before {
            return Some(());
        }
        // A layer that changed itself, moved, or drew its subtree without
        // records of its children, as a surface or nothing, pushes it all.
        if old.own_change != layer.own_change || !placed_as_before || old.end == record + 1 {
            self.push_again(layer, record, parent);
            return Some(());
        }
        let passed = old.passed_down?;
        let mut next = record + 1;
        let mut found = Some(());
        for_each_in_draw_order(&layer.children, |child| {
            if let (Some(()), RenderNode::Layer(child)) = (found, child) {
                found = self.visit(child, next, Some(passed));
                next = self
                    .scene
                    .subtrees
                    .get(next)
                    .map_or(usize::MAX, |at| at.end);
            }
        });
        found.filter(|()| next == old.end)
    }

    /// Pushes `layer`'s subtree into the scratch scene as the full walk
    /// would, from what its parent passed down in the old scene.
    fn push_again(&mut self, layer: &LayerNode, record: usize, parent: Option<usize>) {
        let passed = parent.and_then(|at| self.scene.passed_down.get(at));
        let transform = passed.map_or_else(
            || ProjectiveTransform::uniform_scale(self.scale),
            |passed| passed.transform,
        );
        let style = passed.map_or(ContentStyle::PLAIN, |passed| ContentStyle {
            alpha: passed.alpha,
            color_filter: passed.color_filter.clone(),
        });
        let from = self.scratch.subtrees.len();
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
            },
            &mut None,
        );
        self.pushed.push((record, from));
    }
}
