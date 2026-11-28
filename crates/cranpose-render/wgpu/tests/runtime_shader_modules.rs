// A runtime shader's module holds the naga IR of its source: a megabyte or
// more for a large shader. On a backend whose pipelines let their modules
// go once made (Vulkan and Metal), a screen of many runtime shaders keeps
// no module of theirs once its pipelines are built: a page of twelve such
// shaders leaves as many live shader modules as a page of two.

use cranpose_render_common::graph::{LayerNode, ProjectiveTransform, RenderNode};
use cranpose_ui_graphics::{
    GraphicsLayer, RUNTIME_SHADER_PRELUDE_WGSL, Rect, RenderEffect, RuntimeShader, Size,
    request_shader_warm_ups,
};

use crate::{shared_test_support, support};

const SIZE: u32 = 64;
const CELL: f32 = 16.0;

// A shader of a source no other test draws, `index` its own. It does not
// declare its independence of fragment positions. A draw then asks its
// module.
fn shader(index: usize) -> RuntimeShader {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // runtime shader modules {} {index}
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return vec4<f32>(u[0].x, 0.0, 1.0, 1.0);
         }}",
        std::process::id()
    ));
    shader.set_float4(0, index as f32 / 16.0, 0.0, 0.0, 0.0);
    shader
}

// A page of a cell for each of `shaders`.
fn page(shaders: std::ops::Range<usize>) -> LayerNode {
    let cells = shaders.map(|index| {
        let layer = GraphicsLayer {
            render_effect: Some(RenderEffect::runtime_shader(shader(index))),
            ..GraphicsLayer::default()
        };
        let (column, row) = ((index % 4) as f32, (index / 4 % 4) as f32);
        RenderNode::Layer(Box::new(shared_test_support::layer_node(
            Rect::from_size(Size::new(CELL, CELL)),
            ProjectiveTransform::translation(column * CELL, row * CELL),
            layer,
            Vec::new(),
        )))
    });
    support::page_graph(SIZE, SIZE, cells.collect())
}

// The live shader modules of `renderer`'s device once `page` is drawn and
// its frames are done with their builds.
fn modules_after(renderer: &mut support::LockedRenderer, page: &LayerNode) -> isize {
    support::capture_graph_settled(renderer, page, SIZE, SIZE);
    support::wait_for_background_compiler_idle();
    // the frames after the builds let their modules go
    support::capture_graph(renderer, page, SIZE, SIZE);
    support::capture_graph(renderer, page, SIZE, SIZE);
    renderer
        .try_device()
        .expect("a device")
        .get_internal_counters()
        .hal
        .shader_modules
        .read()
}

#[test]
fn built_runtime_shaders_keep_no_shader_module() {
    let _lock = support::gpu_test_lock();
    let renderer = support::LockedRenderer::beside_locked().expect("GPU required");
    assert_modules_do_not_grow(renderer);
}

// As an app's renderer builds them: on the compiler's threads, while the
// frames that asked go on.
#[test]
fn runtime_shaders_built_in_the_background_keep_no_shader_module() {
    let _lock = support::gpu_test_lock();
    let renderer =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    assert_modules_do_not_grow(renderer);
}

// As an app's start builds the shaders it asked to warm up: queued once, on
// the compiler's threads, while frames that draw none of them go on.
#[test]
fn warmed_up_runtime_shaders_keep_no_shader_module() {
    let _lock = support::gpu_test_lock();
    let mut renderer =
        support::LockedRenderer::compiling_in_background_beside_locked().expect("GPU required");
    if !keeps_no_modules(&renderer) {
        return;
    }
    let drawn = page(0..2);
    let two = modules_after(&mut renderer, &drawn);
    request_shader_warm_ups((20..32).map(shader));
    let warmed = modules_after(&mut renderer, &drawn);
    drop(renderer);
    assert_eq!(
        warmed, two,
        "twelve warmed-up runtime shaders keep no shader module once built"
    );
}

// A pipeline a later frame needs from a source whose module went, as a
// page change or a theme asks for a body's other look: the source is
// parsed again and the new pipeline draws its own pixels.
#[test]
fn a_later_pipeline_of_a_released_source_draws() {
    let _lock = support::gpu_test_lock();
    let mut renderer = support::LockedRenderer::beside_locked().expect("GPU required");
    let blue = modules_after(&mut renderer, &look(false));
    let frame = support::capture_graph_settled(&mut renderer, &look(true), SIZE, SIZE);
    let red = modules_after(&mut renderer, &look(true));
    drop(renderer);
    assert!(
        frame.pixels.as_chunks::<4>().0.contains(&[255, 0, 0, 255]),
        "the later pipeline draws red"
    );
    assert_eq!(red, blue, "its module goes too");
}

// A page of one cell whose shader draws red where its override asks and
// blue otherwise, each a pipeline of its own from one source.
fn look(red: bool) -> LayerNode {
    let mut shader = RuntimeShader::new(&format!(
        "{RUNTIME_SHADER_PRELUDE_WGSL}
         // runtime shader modules look {}
         override RED: bool = false;
         @fragment fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
             return select(vec4<f32>(0.0, 0.0, 1.0, 1.0), vec4<f32>(1.0, 0.0, 0.0, 1.0), RED);
         }}",
        std::process::id()
    ));
    shader.set_override("RED", if red { 1.0 } else { 0.0 });
    shader.set_specialization_exact(true);
    let layer = GraphicsLayer {
        render_effect: Some(RenderEffect::runtime_shader(shader)),
        ..GraphicsLayer::default()
    };
    let cell = shared_test_support::layer_node(
        Rect::from_size(Size::new(CELL, CELL)),
        ProjectiveTransform::identity(),
        layer,
        Vec::new(),
    );
    support::page_graph(SIZE, SIZE, vec![RenderNode::Layer(Box::new(cell))])
}

// Whether `renderer`'s pipelines let their shader modules go: on Vulkan and
// Metal.
fn keeps_no_modules(renderer: &support::LockedRenderer) -> bool {
    let backend = renderer
        .try_device()
        .expect("a device")
        .adapter_info()
        .backend;
    matches!(backend, wgpu::Backend::Vulkan | wgpu::Backend::Metal)
}

fn assert_modules_do_not_grow(mut renderer: support::LockedRenderer) {
    if !keeps_no_modules(&renderer) {
        return;
    }
    let two = modules_after(&mut renderer, &page(0..2));
    assert!(two > 0, "the device counts its shader modules");
    let twelve = modules_after(&mut renderer, &page(2..14));
    drop(renderer);
    assert_eq!(
        twelve, two,
        "twelve more runtime shaders keep no shader module once built"
    );
}
