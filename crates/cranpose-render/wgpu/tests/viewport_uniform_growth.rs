mod support;

use cranpose_render_common::{
    Renderer,
    graph::{DrawRunNode, PrimitivePhase, ProjectiveTransform, RenderGraph, RenderNode},
};
use cranpose_ui_graphics::{
    Brush, Color, CompositingStrategy, DrawScope, DrawScopeDefault, GraphicsLayer, Size,
};

const COLUMNS: u32 = 16;
const ROWS: u32 = 20;
const TILE: u32 = 16;

fn tiles(rows: u32, ink: [u8; 3]) -> RenderGraph {
    let children = (0..COLUMNS * rows)
        .map(|index| {
            let mut draw = DrawScopeDefault::new(Size::new(TILE as f32, TILE as f32));
            draw.draw_rect(Brush::solid(Color::from_rgb_u8(ink[0], ink[1], ink[2])));
            let mut layer = support::layer_node(
                Some(index as usize + 1),
                TILE as f32,
                TILE as f32,
                vec![RenderNode::DrawRun(DrawRunNode::new(
                    PrimitivePhase::BeforeChildren,
                    draw.into_primitives(),
                ))],
            );
            layer.graphics_layer = GraphicsLayer {
                compositing_strategy: CompositingStrategy::Offscreen,
                ..Default::default()
            };
            layer.transform_to_parent = ProjectiveTransform::translation(
                (index % COLUMNS * TILE) as f32,
                (index / COLUMNS * TILE) as f32,
            );
            RenderNode::Layer(Box::new(layer))
        })
        .collect();
    let mut graph = RenderGraph::new(support::layer_node(
        Some(0),
        (COLUMNS * TILE) as f32,
        (rows * TILE) as f32,
        children,
    ));
    graph.root.recompute_raster_cache_hashes();
    graph
}

#[test]
fn every_viewport_survives_first_frame_growth_and_buffer_reuse() {
    let mut renderer = support::headless_renderer().expect("GPU renderer");
    for (rows, ink) in [
        (ROWS, [32, 160, 224]),
        (1, [224, 32, 96]),
        (ROWS * 2, [64, 224, 96]),
    ] {
        renderer.scene_mut().graph = Some(tiles(rows, ink));
        let frame = renderer
            .capture_frame(COLUMNS * TILE, rows * TILE)
            .expect("first frame after changing the grid");
        for index in 0..COLUMNS * rows {
            let x = index % COLUMNS * TILE + TILE / 2;
            let y = index / COLUMNS * TILE + TILE / 2;
            let offset = ((y * frame.width + x) * 4) as usize;
            assert_eq!(
                &frame.pixels[offset..offset + 4],
                &[ink[0], ink[1], ink[2], 255],
                "tile {index} in {rows} rows must render on its first frame, including earlier passes after storage grows"
            );
        }
    }
}
