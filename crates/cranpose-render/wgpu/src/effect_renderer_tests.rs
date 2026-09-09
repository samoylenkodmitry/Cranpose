use super::*;
use crate::frame_graph::{WgpuFrameGraph, WgpuFrameGraphExecutor, read_uploaded_bytes};

const MODES: [TileMode; 4] = [
    TileMode::Clamp,
    TileMode::Repeated,
    TileMode::Mirror,
    TileMode::Decal,
];

fn capture_blur_batch(
    renderer: &mut EffectRenderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &OffscreenTarget,
    format: wgpu::TextureFormat,
    downsample: Option<u32>,
    phase: f32,
) -> Vec<u8> {
    let target = OffscreenTarget::new(device, format, 64, 32);
    let row_bytes = 64 * format.block_copy_size(None).expect("uncompressed target");
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row_bytes * 32),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let draws: Vec<_> = MODES
        .iter()
        .enumerate()
        .map(|(index, mode)| {
            let dest = (index as u32 * 16, 0, 16, 32);
            let mut uniforms = EffectRenderer::blur_uniforms(
                true,
                (32, 32),
                (5, 7, 13, 17),
                dest,
                (8.0, 8.0),
                *mode,
            );
            uniforms.source_region[0] += phase;
            uniforms.dest_region[0] += phase;
            BlurDraw {
                source,
                uniforms,
                downsample,
                scissor: Some(dest),
            }
        })
        .collect();
    let mut graph = WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(None, &[], &[], |recorder| {
        renderer.encode_blur_pass(
            recorder,
            device,
            "Tile Mode Test",
            UploadAllocatorId::BlurHorizontal,
            &target.view,
            (64, 32),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &draws,
        );
        recorder.encoder.copy_texture_to_buffer(
            target.texture().as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: None,
                },
            },
            target.texture().size(),
        );
        Ok(())
    });
    let execution = WgpuFrameGraphExecutor::new()
        .execute_recorded_graph(device, queue, graph)
        .expect("blur batch");
    read_uploaded_bytes(device, &readback, execution.submission)
}

#[test]
fn mixed_blur_tile_modes_preserve_dynamic_shader_pixels() {
    let (_lock, device, queue) = crate::frame_graph::upload_test_device();
    let source = OffscreenTarget::new(&device, wgpu::TextureFormat::Rgba8Unorm, 32, 32);
    let pixels: Vec<_> = (0..32)
        .flat_map(|y| {
            (0..32).flat_map(move |x| [(x * 7) as u8, (y * 5) as u8, ((x + y) * 3) as u8, 255])
        })
        .collect();
    WgpuFrameGraphExecutor::new().upload_texture(
        &queue,
        source.texture().as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(128),
            rows_per_image: None,
        },
        source.texture().size(),
    );
    let dynamic = format!(
        "{}{}",
        shaders::FULLSCREEN_QUAD_VS,
        include_str!("../tests/fixtures/blur_tile_reference.wgsl"),
    );
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba16Float,
    ] {
        let mut renderers = [false, true].map(|reference| {
            let mut renderer =
                EffectRenderer::new(&device, None, format, device.adapter_info().backend);
            if reference {
                renderer.blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("Dynamic Blur Tile Reference"),
                    source: wgpu::ShaderSource::Wgsl(dynamic.clone().into()),
                });
            }
            renderer
        });
        for downsample in [None, Some(2), Some(4)] {
            for phase in [0.0, 0.375] {
                let captures: Vec<_> = renderers
                    .iter_mut()
                    .map(|renderer| {
                        capture_blur_batch(
                            renderer, &device, &queue, &source, format, downsample, phase,
                        )
                    })
                    .collect();
                assert!(captures[0].iter().any(|byte| *byte != 0));
                let differing = captures[0]
                    .iter()
                    .zip(&captures[1])
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(
                    differing, 0,
                    "format={format:?} downsample={downsample:?} phase={phase}"
                );
            }
        }
    }
}
