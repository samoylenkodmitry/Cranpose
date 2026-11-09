//! What a built scene actually paints, for tests that need to compare the
//! scene against the tree it was built from.

use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    graph_scene::Scene,
    scene_store::{ItemRef, LayerRef},
};
use cranpose_ui::{LayoutEngine, Size, TestComposition};
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive};

pub(crate) fn painted_text(layer: LayerRef<'_>, out: &mut Vec<String>) {
    out.extend(layer.painted_text());
}

pub(crate) fn initial_graph(
    composition: &mut TestComposition,
    root: NodeId,
    viewport: Size,
) -> Scene {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    applier
        .compute_layout(root, viewport)
        .expect("composition lays out");
    let mut scene = Scene::new();
    scene.rebuild_from_applier(&applier, root);
    assert!(scene.root_layer().is_some(), "initial scene");
    applier.clear_runtime_handle();
    scene
}

pub(crate) fn with_applier<R>(
    composition: &mut TestComposition,
    read: impl FnOnce(&mut MemoryApplier) -> R,
) -> R {
    let runtime = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(runtime);
    let result = read(&mut applier);
    applier.clear_runtime_handle();
    result
}

pub(crate) fn update_scene(
    composition: &mut TestComposition,
    root: NodeId,
    viewport: Size,
    scene: &mut Scene,
) -> Vec<NodeId> {
    while composition.process_invalid_scopes().expect("recompose") {}
    with_applier(composition, |applier| {
        applier.compute_layout(root, viewport).expect("relayout");
        let geometry = cranpose_ui::take_geometry_scene_nodes();
        let mut content = geometry.reshaped;
        content.extend(cranpose_ui::take_draw_repass_nodes());
        content.extend(applier.take_structural_change_parents_attached_to(root));
        content.sort_unstable();
        content.dedup();
        let moved: Vec<NodeId> = geometry.moved.iter().map(|moved| moved.node).collect();
        let mut layers = Vec::new();
        cranpose_ui::take_layer_property_repass_nodes_into(&mut layers);
        assert!(
            scene.update_from_applier(
                applier,
                root,
                &content
                    .iter()
                    .map(|&id| (id, cranpose_render_common::NodeUpdate::Content))
                    .chain(
                        layers
                            .iter()
                            .map(|&id| (id, cranpose_render_common::NodeUpdate::Layer))
                    )
                    .chain(
                        moved
                            .iter()
                            .map(|&id| (id, cranpose_render_common::NodeUpdate::Moved))
                    )
                    .collect::<Vec<_>>(),
            ),
            "the scoped update applies"
        );
        moved
    })
}

pub(crate) fn fresh_graph(composition: &mut TestComposition, root: NodeId) -> Scene {
    with_applier(composition, |applier| {
        let mut scene = Scene::new();
        scene.rebuild_from_applier(applier, root);
        assert!(scene.root_layer().is_some(), "fresh scene");
        scene
    })
}

// Every solid rectangle color `scene` paints, in draw order.
pub(crate) fn solid_rect_colors(scene: &Scene) -> Vec<Color> {
    let mut colors = Vec::new();
    painted_solid_rect_colors(scene.root_layer().expect("a scene"), &mut colors);
    colors
}

pub(crate) fn painted_solid_rect_colors(layer: LayerRef<'_>, colors: &mut Vec<Color>) {
    for item in layer.items() {
        let primitives: Vec<DrawPrimitive> = match item {
            ItemRef::Layer(child) | ItemRef::Leaf(child) => {
                painted_solid_rect_colors(child, colors);
                continue;
            }
            ItemRef::Run(run) => run.primitives().collect(),
            ItemRef::Shape(shape) => shape.primitives().collect(),
            ItemRef::Draw(draw) => vec![draw.primitive.clone()],
            ItemRef::Text(_) => continue,
        };
        colors.extend(
            primitives
                .into_iter()
                .filter_map(|primitive| match primitive {
                    DrawPrimitive::Rect {
                        brush: Brush::Solid(color),
                        ..
                    } => Some(color),
                    _ => None,
                }),
        );
    }
}
