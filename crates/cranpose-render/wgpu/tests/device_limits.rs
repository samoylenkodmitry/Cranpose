//! Regression coverage for devices with small uniform-buffer binding limits.
//!
//! Android requests `wgpu::Limits::downlevel_defaults()`-shaped device limits,
//! whose 16 KiB `max_uniform_buffer_binding_size` is below the desktop-sized
//! shape batch uniform (768 shapes x 80 bytes = 60 KiB). The renderer must
//! size its batch buffers and shader arrays from the actual device limits
//! instead of compile-time desktop constants, or the very first
//! "Shape Bind Group" raises a validation error and the app dies on launch.

mod support;

#[path = "../src/test_support.rs"]
mod shared_test_support;

use std::sync::Arc;

use cranpose_render_common::{
    Renderer,
    graph::{
        DrawPrimitiveNode, PrimitiveEntry, PrimitiveNode, PrimitivePhase, ProjectiveTransform,
        RenderGraph, RenderNode,
    },
};
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::AppContext;
use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, GraphicsLayer, Rect};

fn downlevel_uniform_renderer() -> Result<(WgpuRenderer, Arc<wgpu::Device>), String> {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .map_err(|err| format!("adapter request failed: {err:?}"))?;
    // The exact limits shape Android requests: downlevel defaults with texture
    // resolution raised to the adapter's, leaving the 16 KiB uniform binding cap.
    let required_limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
    assert_eq!(
        required_limits.max_uniform_buffer_binding_size, 16384,
        "test premise: downlevel uniform binding cap is 16 KiB"
    );
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Downlevel Uniform Limit Test Device"),
        required_features: wgpu::Features::empty(),
        required_limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .map_err(|err| format!("device request failed: {err:?}"))?;

    let device = Arc::new(device);
    let mut renderer = WgpuRenderer::new(&[support::TEST_FONT]);
    renderer.init_gpu(
        Arc::clone(&device),
        Arc::new(queue),
        wgpu::TextureFormat::Bgra8UnormSrgb,
        adapter.get_info().backend,
        adapter.get_downlevel_capabilities().flags,
    );
    Ok((renderer, device))
}

fn solid_rect(rect: Rect, color: Color) -> RenderNode {
    RenderNode::Primitive(PrimitiveEntry {
        phase: PrimitivePhase::BeforeChildren,
        node: PrimitiveNode::Draw(DrawPrimitiveNode {
            primitive: DrawPrimitive::Rect {
                rect,
                brush: Brush::solid(color),
                stroke: None,
            },
            clip: None,
        }),
    })
}

fn many_shapes_graph(shape_count: usize, width: f32, height: f32) -> RenderGraph {
    let columns = 32usize;
    let cell = width / columns as f32;
    let children = (0..shape_count)
        .map(|index| {
            let col = index % columns;
            let row = index / columns;
            solid_rect(
                Rect {
                    x: col as f32 * cell,
                    y: (row as f32 * 3.0) % height,
                    width: cell.max(1.0) - 0.5,
                    height: 2.5,
                },
                Color(
                    0.2 + (index % 5) as f32 * 0.15,
                    0.3,
                    0.9 - (index % 7) as f32 * 0.1,
                    1.0,
                ),
            )
        })
        .collect();
    RenderGraph::new(shared_test_support::layer_node(
        Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        ProjectiveTransform::identity(),
        GraphicsLayer::default(),
        children,
    ))
}

#[test]
fn renderer_survives_downlevel_uniform_binding_limit() {
    let _lock = support::gpu_test_lock();
    let (mut renderer, device) = match downlevel_uniform_renderer() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping downlevel uniform limit test, headless WGPU init failed: {err}");
            return;
        }
    };

    let error_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    // Force batch buffer/bind group creation plus a real multi-batch render:
    // 300 shapes exceed the 16 KiB uniform budget (204 shapes) and must split.
    let app_context = AppContext::new();
    renderer.attach_app_context_services(&app_context);
    renderer.scene_mut().graph = Some(many_shapes_graph(300, 256.0, 256.0));
    let frame = app_context.enter(|| renderer.capture_frame(256, 256));
    let validation = pollster::block_on(error_scope.pop());

    assert!(
        validation.is_none(),
        "rendering on a 16 KiB uniform-binding device must not raise validation errors: {validation:?}"
    );
    let frame = frame.expect("capture should succeed on a downlevel-uniform device");
    let index = ((frame.width + 17) * 4) as usize;
    assert!(
        frame.pixels[index..index + 3] == [128, 76, 178],
        "an interior shape pixel must preserve the deterministic linear output bytes: {:?}",
        &frame.pixels[index..index + 3]
    );
}
