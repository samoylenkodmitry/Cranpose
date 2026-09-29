use std::{
    borrow::Cow,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::SharedShader;

static SOURCE_BUILDS: AtomicUsize = AtomicUsize::new(0);

const SHADER: &str = "
override TINT: f32 = 0.0;

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(f32(index), 0.0, 0.0, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(TINT, 0.0, 0.0, 1.0);
}
";

fn counted_source() -> Cow<'static, str> {
    SOURCE_BUILDS.fetch_add(1, Ordering::Relaxed);
    Cow::Borrowed(SHADER)
}

fn pipeline(device: &wgpu::Device, shader: &SharedShader, tint: f64) -> wgpu::RenderPipeline {
    let constants = [("TINT", tint)];
    let module = shader.module();
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shared shader test"),
        layout: Some(shader.layout()),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants: &constants,
                ..wgpu::PipelineCompilationOptions::default()
            },
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[test]
fn every_variant_and_clone_builds_from_one_parse() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let shader = SharedShader::new(
        &device,
        device.adapter_info().backend,
        "shared shader test",
        counted_source,
        &[],
    );
    assert_eq!(
        SOURCE_BUILDS.load(Ordering::Relaxed),
        0,
        "nothing is parsed before a pipeline needs it"
    );

    let clone = shader.clone();
    let _pipelines = [
        pipeline(&device, &shader, 0.25),
        pipeline(&device, &clone, 0.75),
        pipeline(&device, &shader, 1.0),
    ];

    assert_eq!(SOURCE_BUILDS.load(Ordering::Relaxed), 1);
    assert!(shader.module() == clone.module());
}
