mod support;

#[path = "../src/record_columns.rs"]
mod record_columns;

use cranpose_ui_graphics::{
    Brush, Color, CommandRecording, DrawScope, DrawScopeDefault, Point, Rect, Size, Stroke,
    StrokeCap, framework_shaders::SHAPE_WGSL, strip_index_pattern,
};
use wgpu::util::DeviceExt;

const SIDE: u32 = 160;

fn arcs() -> CommandRecording {
    let mut scope = DrawScopeDefault::new(Size::new(SIDE as f32, SIDE as f32));
    scope.draw_rect_at(
        Rect {
            x: 30.25,
            y: 25.125,
            width: 13.0,
            height: 17.0,
        },
        Brush::solid(Color(0.7, 0.1, 0.4, 0.8)),
    );
    for cap in [StrokeCap::Butt, StrokeCap::Round, StrokeCap::Square] {
        for ring in 0..8 {
            for angle in 0..24 {
                scope.draw_arc(
                    Brush::solid(Color(0.23, 0.61, 0.89, 0.57)),
                    Point::new(65.37, 63.81),
                    24.0 + ring as f32 * 3.125,
                    angle as f32 * 0.27 + 0.017,
                    0.079 + ring as f32 * 0.003,
                    Stroke::new(1.37).with_cap(cap),
                );
            }
        }
    }
    let recording = scope.finish();
    assert!(
        recording
            .shapes()
            .iter()
            .skip(1)
            .all(|record| record.is_banded())
    );
    recording
}

fn pipeline(device: &wgpu::Device, segments: u32) -> wgpu::RenderPipeline {
    let mut replacements = 0;
    let source = SHAPE_WGSL
        .lines()
        .map(|line| {
            if line.trim().starts_with("let segments =") {
                replacements += 1;
                format!("let segments = {segments}u;\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect::<String>();
    assert_eq!(replacements, 1);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let constants = [
        ("SHAPE_SOLID", 1.0),
        ("SHAPE_CLIPPED", 1.0),
        ("SHAPE_KIND_FIXED", -1.0),
    ];
    let options = || wgpu::PipelineCompilationOptions {
        constants: &constants,
        ..Default::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_record_solid"),
            compilation_options: options(),
            buffers: &record_columns::record_vertex_layouts(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_solid"),
            compilation_options: options(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[test]
fn arc_pixels_are_independent_of_strip_tessellation() {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("headless adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).expect("headless device");
    let recording = arcs();
    let buffer = |bytes: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
    };
    let bodies = buffer(
        bytemuck::cast_slice(recording.shapes().bodies()),
        wgpu::BufferUsages::VERTEX,
    );
    let curves = buffer(
        bytemuck::cast_slice(recording.shapes().curves()),
        wgpu::BufferUsages::VERTEX,
    );
    let mut values = [0.0f32; 36];
    values[..4].copy_from_slice(&[SIDE as f32, SIDE as f32, 4.125, 7.75]);
    values[4..8].copy_from_slice(&[0.37, -0.19, 1.25, f32::from_bits(2)]);
    values[8..12].copy_from_slice(&[17.25, 12.375, 125.0, 121.0]);
    values[14] = 1.0;
    let uniforms = buffer(bytemuck::cast_slice(&values), wgpu::BufferUsages::UNIFORM);
    let empty_tables = buffer(&vec![0; 12_288], wgpu::BufferUsages::UNIFORM);
    let (texture, view) = support::render_target(
        &device,
        SIDE,
        SIDE,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let mut reference = None;
    for segments in [1, 2, 4, 8] {
        let pipeline = pipeline(&device, segments);
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let tables = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(1),
            entries: &[1, 2, 3].map(|binding| wgpu::BindGroupEntry {
                binding,
                resource: empty_tables.as_entire_binding(),
            }),
        });
        let indices: Vec<u32> = strip_index_pattern(segments).collect();
        let index = buffer(bytemuck::cast_slice(&indices), wgpu::BufferUsages::INDEX);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.set_bind_group(1, &tables, &[]);
            pass.set_vertex_buffer(0, bodies.slice(..));
            pass.set_vertex_buffer(1, curves.slice(..));
            pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(
                0..indices.len() as u32,
                0,
                0..recording.shapes().len() as u32,
            );
        }
        queue.submit([encoder.finish()]);
        let pixels = support::read_texture_rgba8(&device, &queue, &texture);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|rgba| rgba[3] != 0)
                .count()
                > 2_000
        );
        if let Some(expected) = &reference {
            assert!(
                pixels == *expected,
                "{segments} segments changed the same analytic arcs"
            );
        } else {
            reference = Some(pixels);
        }
    }
}
