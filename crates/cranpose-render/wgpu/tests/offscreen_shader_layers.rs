// A layer drawn by its runtime shader alone, which the app draws
// offscreen, keeps its output from its first frame. A frame fills only
// some of those outputs; the rest show their placeholder, and the frames
// after fill them until every layer shows its own pixels.

use cranpose_render_common::{
    Renderer,
    graph::{CachePolicy, LayerNode, ProjectiveTransform, RenderNode},
};
use cranpose_ui_graphics::{
    Color, CompositingStrategy, GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect,
    RuntimeShader, ShaderPlaceholder,
};

use crate::{shared_test_support, support};

const SIZE: u32 = 96;
const CELL: u32 = 24;

// A shader painting its layer in a shade of blue its uniform picks, red
// while it compiles.
pub(crate) fn shader() -> RuntimeShader {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // offscreen shader layers {}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(0.0, u[0].x, 1.0, 1.0);
         }}",
        std::process::id()
    ));
    shader.set_placeholder(Some(ShaderPlaceholder {
        color: Color(1.0, 0.0, 0.0, 1.0),
        shape: None,
    }));
    shader.set_position_independent(true);
    shader
}

// A page of sixteen cells, each a layer drawn by `shader` alone with a
// shade of its own, drawn with `strategy`. The first cell's shade is
// `first`.
pub(crate) fn page(shader: &RuntimeShader, strategy: CompositingStrategy, first: f32) -> LayerNode {
    grid(
        shader,
        strategy,
        first,
        Grid {
            cell: CELL,
            columns: 4,
            rows: 4,
        },
    )
}

// A page of `columns` by `rows` square cells of `cell` pixels.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub(crate) cell: u32,
    pub(crate) columns: u32,
    pub(crate) rows: u32,
}

impl Grid {
    pub(crate) fn size(self) -> (u32, u32) {
        (self.cell * self.columns, self.cell * self.rows)
    }
}

// The cells of `grid`, each a layer drawn by `shader` alone with a shade of
// its own, drawn with `strategy`. The first cell's shade is `first`.
pub(crate) fn grid(
    shader: &RuntimeShader,
    strategy: CompositingStrategy,
    first: f32,
    grid: Grid,
) -> LayerNode {
    let count = grid.columns * grid.rows;
    let side = grid.cell as f32;
    let cells = (0..count).map(|index| {
        let mut cell = shader.clone();
        let shade = if index == 0 {
            first
        } else {
            index as f32 / count as f32
        };
        cell.set_float4(0, shade, 0.0, 0.0, 0.0);
        let layer = GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(cell)),
            compositing_strategy: strategy,
            ..GraphicsLayer::default()
        };
        let (column, row) = ((index % grid.columns) as f32, (index / grid.columns) as f32);
        let mut node = shared_test_support::layer_node(
            Rect::from_size(cranpose_ui_graphics::Size::new(side, side)),
            ProjectiveTransform::translation(column * side, row * side),
            layer,
            Vec::new(),
        );
        // as the scene builder marks a layer the app draws offscreen
        node.isolation.explicit_offscreen = strategy == CompositingStrategy::Offscreen;
        node.cache_policy = if node.isolation.explicit_offscreen {
            CachePolicy::Offscreen
        } else {
            CachePolicy::Auto
        };
        RenderNode::Layer(Box::new(node))
    });
    let (width, height) = grid.size();
    support::page_graph(width, height, cells.collect())
}

#[test]
fn offscreen_shader_layers_past_a_frames_fills_reach_their_own_pixels() {
    let shader = shader();
    let mut renderer = support::headless_renderer().expect("GPU required");
    let reference = {
        let mut reference = support::LockedRenderer::beside_locked().expect("reference renderer");
        support::capture_graph_settled(
            &mut reference,
            &page(&shader, CompositingStrategy::Auto, 0.0),
            SIZE,
            SIZE,
        )
    };
    let kept = page(&shader, CompositingStrategy::Offscreen, 0.0);
    support::capture_graph_settled(
        &mut renderer,
        &page(&shader, CompositingStrategy::Auto, 0.0),
        SIZE,
        SIZE,
    );
    support::capture_graph(&mut renderer, &kept, SIZE, SIZE);
    let first = renderer.last_frame_stats().expect("frame statistics");
    assert!(
        first.placeholder_draws > 0,
        "a frame fills only some of sixteen new outputs: {first:?}"
    );
    assert!(
        renderer.needs_frame_warmup(),
        "a frame that left outputs to fill asks for the next"
    );
    let mut last = None;
    for _ in 0..32 {
        let frame = support::capture_graph(&mut renderer, &kept, SIZE, SIZE);
        last = Some(frame);
        if !renderer.needs_frame_warmup() {
            break;
        }
    }
    assert!(
        !renderer.needs_frame_warmup(),
        "the frames stop once every output is filled"
    );
    support::assert_same_bytes(
        "every cell once filled",
        SIZE,
        &last.expect("a frame").pixels,
        &reference.pixels,
    );
    support::capture_graph(&mut renderer, &kept, SIZE, SIZE);
    let still = renderer.last_frame_stats().expect("frame statistics");
    drop(renderer);
    assert_eq!(
        still.layer_cache_hits, 16,
        "each cell draws from its kept output: {still:?}"
    );
    assert_eq!(still.placeholder_draws, 0);
}
