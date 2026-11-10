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

use cranpose_render_common::{
    graph::ProjectiveTransform, graph_scene::RenderDiagnostics, scene_store::SceneStore,
};

use super::{ContentStyle, LightSource, RasterTraversalContext, populate_draws_from_graph};
use crate::{
    damage::merge_areas,
    draw::{ClipBounds, TextRaster},
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
    light: LightSource,
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
    pub(crate) fn run(
        self,
        store: &SceneStore,
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
        let light = LightSource::of(store, scale);
        if scene.start != Some(light) {
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
        let root = store.root_layer()?;
        let walked = repush.visit(Subtree::Layer(root), 0, None);
        let held = repush.held;
        merge_areas(areas);
        walked.ok()?;
        Some(held)
    }
}

impl Repush<'_> {
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
        // A subtree that holds no change pushes again only when it moved.
        let changes = subtree.changes();
        if old.changes.same_subtree(changes) {
            if old.changes.same_place(changes) {
                return Ok(());
            }
            return self.push_again(subtree, record, parent);
        }
        // A node drawn inline has no records beneath its own.
        let Subtree::Layer(layer) = subtree else {
            return self.push_again(subtree, record, parent);
        };
        // A layer that changed itself, drew its subtree without records of
        // its children, as a surface or nothing, or moved pushes it all.
        if !old.changes.same_own(changes)
            || old.end == record + 1
            || !old.changes.same_place(changes)
        {
            return self.push_again(subtree, record, parent);
        }
        let Some(passed) = old.passed_down else {
            return self.push_again(subtree, record, parent);
        };
        let mut next = record + 1;
        let mut walked = Ok(());
        for child in layer.children() {
            walked = self.visit(Subtree::of_record(child), next, Some(passed));
            if walked.is_err() {
                break;
            }
            next = self
                .scene
                .subtrees
                .get(next)
                .map_or(usize::MAX, |at| at.end);
        }
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

    fn push_again(
        &mut self,
        subtree: Subtree<'_>,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Stop> {
        let passed = parent.and_then(|at| self.scene.passed_down.get(at));
        let transform = passed.map_or_else(
            || ProjectiveTransform::uniform_scale(self.scale),
            |passed| passed.transform,
        );
        let style = passed.map_or(ContentStyle::PLAIN, |passed| ContentStyle {
            alpha: passed.alpha,
            color_filter: passed.color_filter.clone(),
        });
        let context = RasterTraversalContext {
            parent_transform: &transform,
            parent_content_style: &style,
            parent_visual_clip: passed.and_then(|passed| passed.clip),
            inherited_translated_snap_anchor: passed.and_then(|passed| passed.snap_anchor),
            inherited_translated_content_context: passed.is_some_and(|passed| passed.translated),
            diagnostics: self.diagnostics,
            light: self.light,
            same_as_before: false,
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
        populate_draws_from_graph(subtree, self.scene, &context, &mut None);
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
}
