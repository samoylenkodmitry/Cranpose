// A check while the layout walk replaces the render graph: with
// CRANPOSE_DIRECT_CHECK set, every scene update also builds the scene from
// the layout tree, and each picture the graph gave must hold the same items.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{SceneUpdates, graph_scene::RenderDiagnostics};

use crate::{
    draw::{ClipBounds, RenderItem, TextRaster},
    layout_walk::{DirtyMarks, LayoutWalk},
    pipeline::{InPlaceUpdate, UpdatedReach, fill_raster_scene_from_layout},
    rewrite::Rewrite,
    scene::RasterScene,
};

#[derive(Default)]
pub(crate) struct DirectCheck {
    enabled: bool,
    marks: DirtyMarks,
    raster: Option<RasterScene>,
    reach: Vec<Option<ClipBounds>>,
    rewrite: Option<Box<Rewrite>>,
    scale: f32,
    updates: u64,
    in_place: u64,
}

impl DirectCheck {
    pub(crate) fn new() -> Self {
        Self {
            enabled: std::env::var_os("CRANPOSE_DIRECT_CHECK").is_some(),
            scale: 1.0,
            ..Self::default()
        }
    }

    pub(crate) fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.raster = None;
    }

    pub(crate) fn build(
        &mut self,
        applier: &MemoryApplier,
        root: NodeId,
        updates: Option<SceneUpdates<'_>>,
        (diagnostics, text): (&RenderDiagnostics, &TextRaster),
    ) {
        if !self.enabled {
            return;
        }
        self.updates += 1;
        let window = ClipBounds {
            min_x: 0,
            min_y: 0,
            max_x: i32::MAX,
            max_y: i32::MAX,
        };
        let previous = match updates {
            Some(updates) if !updates.is_empty() => {
                self.marks.mark(applier, updates);
                if let Some(raster) = self.raster.as_mut() {
                    let walk = LayoutWalk::new(applier, &self.marks);
                    let mut areas = Vec::new();
                    let updated = InPlaceUpdate {
                        scene: raster,
                        reach: &mut self.reach,
                        rewrite: &mut self.rewrite,
                    }
                    .run_layout(
                        (&walk, root),
                        diagnostics,
                        (self.scale, text),
                        window,
                        &mut areas,
                    );
                    if let Some(reach) = updated {
                        self.in_place += 1;
                        if reach == UpdatedReach::Lost {
                            raster.reach_into(window, &mut self.reach);
                        }
                        return;
                    }
                }
                self.raster.take()
            }
            _ => {
                self.marks.clear(applier);
                None
            }
        };
        let mut raster = RasterScene::new();
        let walk = LayoutWalk::new(applier, &self.marks);
        let built = fill_raster_scene_from_layout(
            &mut raster,
            (&walk, root),
            (self.scale, previous.as_ref()),
            diagnostics,
            text,
        );
        if built {
            raster.reach_into(window, &mut self.reach);
        }
        self.raster = built.then_some(raster);
    }

    pub(crate) fn compare(&self, graph: &RasterScene, scale: f32) {
        if !self.enabled || scale != self.scale {
            return;
        }
        let Some(direct) = &self.raster else {
            panic!("direct check: update {} built no scene", self.updates);
        };
        if std::env::var_os("CRANPOSE_DIRECT_CHECK_LOG").is_some() {
            eprintln!(
                "[direct-check] update {} in place {} items {}",
                self.updates,
                self.in_place,
                direct.order.len()
            );
        }
        if let Some(at) = first_difference(graph, direct) {
            let shown = |scene: &RasterScene| {
                scene.order[at.saturating_sub(2)..(at + 3).min(scene.order.len())]
                    .iter()
                    .map(|&item| describe(scene, item))
                    .collect::<Vec<_>>()
                    .join("\n    ")
            };
            panic!(
                "direct check: update {} differs at item {at} of {} / {}\n  graph:\n    {}\n  walk:\n    {}",
                self.updates,
                graph.order.len(),
                direct.order.len(),
                shown(graph),
                shown(direct),
            );
        }
    }
}

fn first_difference(graph: &RasterScene, direct: &RasterScene) -> Option<usize> {
    let shared = graph.order.len().min(direct.order.len());
    (0..shared)
        .find(|&at| describe(graph, graph.order[at]) != describe(direct, direct.order[at]))
        .or_else(|| (graph.order.len() != direct.order.len()).then_some(shared))
}

fn describe(scene: &RasterScene, item: RenderItem) -> String {
    match item {
        RenderItem::Shape(index) => format!("{:?}", scene.shapes[index]),
        RenderItem::Text(index) => format!("{:?}", scene.texts[index]),
        RenderItem::Shadow(index) => format!("{:?}", scene.shadows[index]),
        RenderItem::Image(index) => {
            let image = &scene.images[index];
            format!(
                "Image {:?} {:?} {} {:?} {:?} {:?} {:?} {:?} {:?}",
                image.rect,
                image.snap_anchor,
                image.alpha,
                image.color_filter,
                image.clip,
                image.blend_mode,
                image.src_rect,
                image.transform,
                image.image.id(),
            )
        }
    }
}
