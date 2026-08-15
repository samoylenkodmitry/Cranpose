use crate::scene::{
    CompositorScene, DrawOp, DrawOpKind, DrawShape, ImageDraw, SceneBrush, SnapAnchor, TextDraw,
};
use crate::surface_plan::TranslationRenderContext;
use cranpose_core::collections::map::HashMap;
use cranpose_core::NodeId;
use cranpose_ui_graphics::{Brush, Point, Rect};
use std::cell::RefCell;

const MEMO_ITEM_LIMIT: usize = 65536;
const MEMO_SPAN_ITEM_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CollectMemoKey {
    layer_offset: [u32; 2],
    inherited_clip: Option<[u32; 4]>,
    snap_anchor: Option<[u32; 3]>,
    context: TranslationRenderContext,
    child_context: TranslationRenderContext,
}

pub(crate) fn memo_key(
    layer_offset: Point,
    inherited_clip: Option<Rect>,
    snap_anchor: Option<SnapAnchor>,
    context: TranslationRenderContext,
    child_context: TranslationRenderContext,
) -> CollectMemoKey {
    CollectMemoKey {
        layer_offset: [layer_offset.x.to_bits(), layer_offset.y.to_bits()],
        inherited_clip: inherited_clip.map(|clip| {
            [
                clip.x.to_bits(),
                clip.y.to_bits(),
                clip.width.to_bits(),
                clip.height.to_bits(),
            ]
        }),
        snap_anchor: snap_anchor.map(|anchor| {
            [
                anchor.origin.x.to_bits(),
                anchor.origin.y.to_bits(),
                anchor.device_pixel_step.to_bits(),
            ]
        }),
        context,
        child_context,
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CollectMemoBase {
    shapes: usize,
    brushes: usize,
    images: usize,
    texts: usize,
    draw_ops: usize,
    shadow_draws: usize,
    effect_layers: usize,
    backdrop_layers: usize,
    retained_draws: usize,
    child_layers: usize,
    next_z: usize,
}

pub(crate) fn base_of(scene: &CompositorScene, child_layers: usize) -> CollectMemoBase {
    CollectMemoBase {
        shapes: scene.shapes.len(),
        brushes: scene.brushes.len(),
        images: scene.images.len(),
        texts: scene.texts.len(),
        draw_ops: scene.draw_ops.len(),
        shadow_draws: scene.shadow_draws.len(),
        effect_layers: scene.effect_layers.len(),
        backdrop_layers: scene.backdrop_layers.len(),
        retained_draws: scene.retained_draws.len(),
        child_layers,
        next_z: scene.next_z,
    }
}

struct CollectMemoEntry {
    key: CollectMemoKey,
    shapes: Vec<DrawShape>,
    brushes: Vec<Brush>,
    images: Vec<ImageDraw>,
    texts: Vec<TextDraw>,
    draw_ops: Vec<DrawOp>,
    z_len: usize,
}

impl CollectMemoEntry {
    fn item_count(&self) -> usize {
        self.shapes.len() + self.images.len() + self.texts.len() + self.draw_ops.len()
    }
}

enum CollectMemoSlot {
    Seen(CollectMemoKey),
    Held(CollectMemoEntry),
}

impl CollectMemoSlot {
    fn item_count(&self) -> usize {
        match self {
            CollectMemoSlot::Seen(_) => 0,
            CollectMemoSlot::Held(entry) => entry.item_count(),
        }
    }
}

#[derive(Default)]
struct CollectMemo {
    armed: bool,
    root_scale: u32,
    items: usize,
    reuses: usize,
    entries: HashMap<NodeId, CollectMemoSlot>,
}

impl CollectMemo {
    fn clear(&mut self) {
        self.entries.clear();
        self.items = 0;
    }
}

thread_local! {
    static COLLECT_MEMO: RefCell<CollectMemo> = RefCell::new(CollectMemo::default());
}

pub(crate) fn begin_frame(root_scale: f32) {
    COLLECT_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if memo.root_scale != root_scale.to_bits() {
            memo.root_scale = root_scale.to_bits();
            memo.clear();
        }
        memo.armed = true;
    });
}

pub(crate) fn end_frame() {
    COLLECT_MEMO.with(|memo| memo.borrow_mut().armed = false);
}

pub(crate) fn drop_nodes(node_ids: &[NodeId]) {
    COLLECT_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        for node_id in node_ids {
            if let Some(entry) = memo.entries.remove(node_id) {
                memo.items = memo.items.saturating_sub(entry.item_count());
            }
        }
    });
}

pub(crate) fn drop_all() {
    COLLECT_MEMO.with(|memo| memo.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn entry_count() -> usize {
    COLLECT_MEMO.with(|memo| memo.borrow().entries.len())
}

#[cfg(test)]
pub(crate) fn take_reuse_count() -> usize {
    COLLECT_MEMO.with(|memo| std::mem::take(&mut memo.borrow_mut().reuses))
}

pub(crate) fn try_reuse(
    node_id: Option<NodeId>,
    key: &CollectMemoKey,
    scene: &mut CompositorScene,
) -> bool {
    let Some(node_id) = node_id else {
        return false;
    };
    COLLECT_MEMO.with(|memo| {
        let mut borrowed = memo.borrow_mut();
        let memo = &mut *borrowed;
        if !memo.armed {
            return false;
        }
        let Some(CollectMemoSlot::Held(entry)) = memo.entries.get(&node_id) else {
            return false;
        };
        if entry.key != *key {
            return false;
        }
        write_entry_into(entry, scene);
        memo.reuses += 1;
        true
    })
}

fn write_entry_into(entry: &CollectMemoEntry, scene: &mut CompositorScene) {
    let shape_base = scene.shapes.len();
    let brush_base = scene.brushes.len() as u32;
    let image_base = scene.images.len();
    let text_base = scene.texts.len();
    let z_base = scene.next_z;

    scene.brushes.extend(entry.brushes.iter().cloned());
    scene.shapes.extend(entry.shapes.iter().map(|shape| {
        let mut shape = *shape;
        shape.z_index += z_base;
        if let SceneBrush::Gradient(index) = shape.brush {
            shape.brush = SceneBrush::Gradient(index + brush_base);
        }
        shape
    }));
    scene.images.extend(entry.images.iter().map(|image| {
        let mut image = image.clone();
        image.z_index += z_base;
        image
    }));
    scene.texts.extend(entry.texts.iter().map(|text| {
        let mut text = text.clone();
        text.z_index += z_base;
        text
    }));
    scene
        .draw_ops
        .extend(entry.draw_ops.iter().map(|op| DrawOp {
            z_index: op.z_index + z_base,
            kind: match op.kind {
                DrawOpKind::Shape(index) => DrawOpKind::Shape(index + shape_base),
                DrawOpKind::Image(index) => DrawOpKind::Image(index + image_base),
                DrawOpKind::Text(index) => DrawOpKind::Text(index + text_base),
                DrawOpKind::Shadow(index) => DrawOpKind::Shadow(index),
                DrawOpKind::Retained(index) => DrawOpKind::Retained(index),
            },
        }));
    scene.next_z = z_base + entry.z_len;
}

pub(crate) fn capture(
    node_id: Option<NodeId>,
    key: CollectMemoKey,
    scene: &CompositorScene,
    base: CollectMemoBase,
    child_layers: usize,
) {
    let Some(node_id) = node_id else {
        return;
    };
    if scene.shadow_draws.len() != base.shadow_draws
        || scene.effect_layers.len() != base.effect_layers
        || scene.backdrop_layers.len() != base.backdrop_layers
        || scene.retained_draws.len() != base.retained_draws
        || child_layers != base.child_layers
    {
        return;
    }
    let span_items = (scene.shapes.len() - base.shapes)
        + (scene.images.len() - base.images)
        + (scene.texts.len() - base.texts)
        + (scene.draw_ops.len() - base.draw_ops);
    if span_items == 0 || span_items > MEMO_SPAN_ITEM_LIMIT {
        return;
    }

    COLLECT_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        if !memo.armed {
            return;
        }
        let seen_the_same_span_before = match memo.entries.remove(&node_id) {
            Some(previous) => {
                memo.items = memo.items.saturating_sub(previous.item_count());
                match previous {
                    CollectMemoSlot::Seen(seen_key) => seen_key == key,
                    CollectMemoSlot::Held(entry) => entry.key == key,
                }
            }
            None => false,
        };
        if !seen_the_same_span_before {
            memo.entries.insert(node_id, CollectMemoSlot::Seen(key));
            return;
        }
        if memo.items + span_items > MEMO_ITEM_LIMIT {
            memo.entries.insert(node_id, CollectMemoSlot::Seen(key));
            return;
        }
        let brush_base = base.brushes as u32;
        let entry = CollectMemoEntry {
            key,
            shapes: scene.shapes[base.shapes..]
                .iter()
                .map(|shape| {
                    let mut shape = *shape;
                    shape.z_index -= base.next_z;
                    if let SceneBrush::Gradient(index) = shape.brush {
                        shape.brush = SceneBrush::Gradient(index - brush_base);
                    }
                    shape
                })
                .collect(),
            brushes: scene.brushes[base.brushes..].to_vec(),
            images: scene.images[base.images..]
                .iter()
                .map(|image| {
                    let mut image = image.clone();
                    image.z_index -= base.next_z;
                    image
                })
                .collect(),
            texts: scene.texts[base.texts..]
                .iter()
                .map(|text| {
                    let mut text = text.clone();
                    text.z_index -= base.next_z;
                    text
                })
                .collect(),
            draw_ops: scene.draw_ops[base.draw_ops..]
                .iter()
                .map(|op| DrawOp {
                    z_index: op.z_index - base.next_z,
                    kind: match op.kind {
                        DrawOpKind::Shape(index) => DrawOpKind::Shape(index - base.shapes),
                        DrawOpKind::Image(index) => DrawOpKind::Image(index - base.images),
                        DrawOpKind::Text(index) => DrawOpKind::Text(index - base.texts),
                        DrawOpKind::Shadow(index) => DrawOpKind::Shadow(index),
                        DrawOpKind::Retained(index) => DrawOpKind::Retained(index),
                    },
                })
                .collect(),
            z_len: scene.next_z - base.next_z,
        };
        memo.items += entry.item_count();
        memo.entries.insert(node_id, CollectMemoSlot::Held(entry));
    });
}

#[cfg(test)]
mod tests {
    use super::{begin_frame, drop_all, drop_nodes, end_frame, entry_count, take_reuse_count};
    use crate::normalized_scene::collect_layer_contents;
    use crate::scene::CompositorScene;
    use cranpose_core::collections::map::HashMap;
    use cranpose_core::NodeId;
    use cranpose_render_common::graph::RenderGraph;
    use cranpose_render_common::scene_builder::{
        build_graph_from_applier, update_graph_from_applier_report_into,
    };
    use cranpose_ui::text::TextStyle;
    use cranpose_ui::{Column, ColumnSpec, LayoutEngine, Modifier, ScrollState, Size, Text};
    use cranpose_ui_graphics::GraphicsLayer;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn shape_summary(scene: &CompositorScene) -> Vec<(u32, u32, u32, u32, usize, bool)> {
        scene
            .shapes
            .iter()
            .map(|shape| {
                (
                    shape.rect.x.to_bits(),
                    shape.rect.y.to_bits(),
                    shape.rect.width.to_bits(),
                    shape.rect.height.to_bits(),
                    shape.z_index,
                    matches!(shape.brush, crate::scene::SceneBrush::Gradient(_)),
                )
            })
            .collect()
    }

    fn text_summary(scene: &CompositorScene) -> Vec<(NodeId, u32, u32, usize)> {
        scene
            .texts
            .iter()
            .map(|text| {
                (
                    text.node_id,
                    text.rect.x.to_bits(),
                    text.rect.y.to_bits(),
                    text.z_index,
                )
            })
            .collect()
    }

    fn collect_scene(graph: &RenderGraph) -> CompositorScene {
        let mut rects: HashMap<usize, cranpose_ui_graphics::Rect> = HashMap::new();
        let mut requirements: HashMap<usize, crate::surface_plan::LayerSurfaceRequirements> =
            HashMap::new();
        collect_layer_contents(&graph.root, None, None, &mut rects, &mut requirements).scene
    }

    #[test]
    fn a_patched_scene_collects_what_a_full_collect_collects() {
        let scroll_holder: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
        let label_holder: Rc<RefCell<Option<cranpose_core::MutableState<String>>>> =
            Rc::new(RefCell::new(None));
        let scroll_id_holder: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let scroll_holder_for_comp = scroll_holder.clone();
        let label_holder_for_comp = label_holder.clone();
        let scroll_id_holder_for_comp = scroll_id_holder.clone();

        let mut composition = cranpose_ui::run_test_composition(move || {
            let scroll_id_holder_for_content = scroll_id_holder_for_comp.clone();
            let scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
            *scroll_holder_for_comp.borrow_mut() = Some(scroll_state.clone());
            let label = cranpose_core::useState(|| "top".to_string());
            *label_holder_for_comp.borrow_mut() = Some(label);
            Column(
                Modifier::empty().size_points(240.0, 400.0),
                ColumnSpec::default(),
                move || {
                    Column(
                        Modifier::empty().size_points(240.0, 40.0),
                        ColumnSpec::default(),
                        || {
                            cranpose_ui::Box(
                                Modifier::empty().size_points(240.0, 20.0),
                                cranpose_ui::BoxSpec::default(),
                                || {
                                    Text("chrome a", Modifier::empty(), TextStyle::default());
                                },
                            );
                            cranpose_ui::Box(
                                Modifier::empty().size_points(240.0, 20.0),
                                cranpose_ui::BoxSpec::default(),
                                || {
                                    Text("chrome b", Modifier::empty(), TextStyle::default());
                                },
                            );
                        },
                    );
                    Text(label, Modifier::empty(), TextStyle::default());
                    let scroll_id = Column(
                        Modifier::empty()
                            .size_points(240.0, 320.0)
                            .vertical_scroll(scroll_state.clone(), false),
                        ColumnSpec::default(),
                        || {
                            for index in 0..10usize {
                                cranpose_ui::Box(
                                    Modifier::empty().size_points(240.0, 60.0).graphics_layer(
                                        || GraphicsLayer {
                                            alpha: 1.0,
                                            ..GraphicsLayer::default()
                                        },
                                    ),
                                    cranpose_ui::BoxSpec::default(),
                                    move || {
                                        Text(
                                            format!("row {index}"),
                                            Modifier::empty(),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            }
                        },
                    );
                    *scroll_id_holder_for_content.borrow_mut() = Some(scroll_id);
                },
            );
        });

        let root = composition.root().expect("composition root");
        let viewport = Size {
            width: 240.0,
            height: 400.0,
        };
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("initial layout");
        applier.clear_runtime_handle();
        drop(applier);

        let scroll_state_first = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state");
        assert!(
            scroll_state_first.dispatch_raw_delta(30.0) > 0.0,
            "the first scroll must be consumed"
        );
        composition
            .process_invalid_scopes()
            .expect("recomposition after the first scroll");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("scrolled layout");
        let mut graph = build_graph_from_applier(&mut applier, root, 1.0).expect("initial graph");
        graph.root.recompute_raster_cache_hashes();
        applier.clear_runtime_handle();
        drop(applier);

        drop_all();
        begin_frame(1.0);
        let _first = collect_scene(&graph);
        end_frame();
        assert!(entry_count() > 0, "the first collect must note the spans");
        begin_frame(1.0);
        let _second = collect_scene(&graph);
        end_frame();
        let _ = take_reuse_count();

        let scroll_state = scroll_holder
            .borrow()
            .as_ref()
            .cloned()
            .expect("scroll state");
        assert!(
            scroll_state.dispatch_raw_delta(70.0) > 0.0,
            "test scroll must be consumed"
        );
        let _label = label_holder
            .borrow()
            .as_ref()
            .copied()
            .expect("label state");
        composition
            .process_invalid_scopes()
            .expect("recomposition after the change");
        let dirty_nodes = vec![scroll_id_holder.borrow().expect("scroll column id")];

        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        applier
            .compute_layout(root, viewport)
            .expect("updated layout");
        let mut changed_nodes: Vec<NodeId> = Vec::new();
        let report = update_graph_from_applier_report_into(
            &mut applier,
            &mut graph,
            &dirty_nodes,
            1.0,
            &mut changed_nodes,
        );
        applier.clear_runtime_handle();
        assert!(report.applied, "the patch should apply in place");

        drop_nodes(&changed_nodes);
        begin_frame(1.0);
        let memo_road = collect_scene(&graph);
        end_frame();
        let reuses = take_reuse_count();
        assert!(
            reuses > 0,
            "the patched collect must reuse the spans it holds, dirty {dirty_nodes:?}"
        );

        drop_all();
        begin_frame(1.0);
        let full_road = collect_scene(&graph);
        end_frame();
        assert_eq!(take_reuse_count(), 0, "the full collect must reuse nothing");

        assert_eq!(
            memo_road.draw_ops, full_road.draw_ops,
            "draw ops must match the full collect"
        );
        assert_eq!(memo_road.next_z, full_road.next_z, "z count must match");
        assert_eq!(
            shape_summary(&memo_road),
            shape_summary(&full_road),
            "shapes must match the full collect"
        );
        assert_eq!(
            text_summary(&memo_road),
            text_summary(&full_road),
            "texts must match the full collect"
        );
        assert_eq!(
            memo_road.brushes.len(),
            full_road.brushes.len(),
            "brush table must match"
        );
        assert_eq!(
            memo_road.images.len(),
            full_road.images.len(),
            "image count must match"
        );
        assert_eq!(
            memo_road.shadow_draws.len(),
            full_road.shadow_draws.len(),
            "shadow count must match"
        );
    }
}
