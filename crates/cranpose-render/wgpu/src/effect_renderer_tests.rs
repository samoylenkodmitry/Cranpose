use super::*;
use crate::frame_graph::{
    PassContext, WgpuFrameGraph, WgpuFrameGraphExecutor, read_uploaded_bytes,
};

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
    capture_effect_batch(device, queue, format, |recorder, target| {
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
    })
}

fn capture_effect_batch(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
    record: impl FnOnce(&mut PassContext<'_>, &OffscreenTarget),
) -> Vec<u8> {
    let target = OffscreenTarget::new(device, format, 64, 32);
    let row_bytes = 64 * format.block_copy_size(None).expect("uncompressed target");
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row_bytes * 32),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut graph = WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(None, &[], &[], |recorder| {
        record(recorder, &target);
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
        .expect("effect batch");
    read_uploaded_bytes(device, &readback, execution.submission)
}

fn patterned_source(device: &wgpu::Device, queue: &wgpu::Queue) -> OffscreenTarget {
    let source = OffscreenTarget::new(device, wgpu::TextureFormat::Rgba8Unorm, 32, 32);
    let pixels: Vec<_> = (0..32)
        .flat_map(|y| {
            (0..32).flat_map(move |x| [(x * 7) as u8, (y * 5) as u8, ((x + y) * 3) as u8, 255])
        })
        .collect();
    WgpuFrameGraphExecutor::new().upload_texture(
        queue,
        source.texture().as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(128),
            rows_per_image: None,
        },
        source.texture().size(),
    );
    source
}

#[test]
fn mixed_blur_tile_modes_preserve_dynamic_shader_pixels() {
    let (_lock, device, queue) = crate::frame_graph::upload_test_device();
    let source = patterned_source(&device, &queue);
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

#[test]
fn blit_specialization_preserves_sampling_masks_and_blending() {
    let (_lock, device, queue) = crate::frame_graph::upload_test_device();
    let source = patterned_source(&device, &queue);
    let backend = device.adapter_info().backend;
    let modes = [BlendMode::Src, BlendMode::SrcOver, BlendMode::DstOut];
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba16Float,
    ] {
        let mut renderers = [false, true].map(|reference| {
            let renderer = EffectRenderer::new(&device, None, format, backend);
            if reference {
                for mode in modes {
                    let dynamic = renderer.blit_pipeline(&device, mode, false).clone();
                    let resources = match mode {
                        BlendMode::Src => &renderer.blit_pipeline_src,
                        BlendMode::DstOut => &renderer.blit_pipeline_dst_out,
                        _ => &renderer.blit_pipeline,
                    };
                    resources[1].get_or_init(backend, || dynamic);
                }
            }
            renderer
        });
        for sample_mode in [CompositeSampleMode::Nearest, CompositeSampleMode::Linear] {
            for rounded_mask in [
                None,
                Some(RoundedCompositeMask {
                    rect: [2.0, 3.0, 58.0, 25.0],
                    radii: [3.0, 5.0, 7.0, 9.0],
                }),
            ] {
                for blend_mode in modes {
                    for phase in [0.0, 0.375] {
                        for prepared in [false, true] {
                            let options = CompositePassOptions {
                                alpha: 0.625,
                                load_op: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.2,
                                    g: 0.3,
                                    b: 0.4,
                                    a: 0.5,
                                }),
                                scissor: Some((1, 2, 61, 28)),
                                rounded_mask,
                                blend_mode,
                                dest_viewport: Some((phase, phase, 64.0 - phase, 32.0 - phase)),
                                source_viewport: Some((3.0 + phase, 7.0 + phase, 13.0, 17.0)),
                                sample_mode,
                            };
                            let captures: Vec<_> = renderers
                                .iter_mut()
                                .map(|renderer| {
                                    capture_effect_batch(
                                        &device,
                                        &queue,
                                        format,
                                        |recorder, target| {
                                            if prepared {
                                                let draw = renderer.prepare_composite_draw(
                                                    recorder,
                                                    &device,
                                                    options.load_op,
                                                    &CompositeBatchItem {
                                                        source: &source,
                                                        alpha: options.alpha,
                                                        scissor: options.scissor,
                                                        rounded_mask,
                                                        blend_mode,
                                                        dest_viewport: options.dest_viewport,
                                                        source_viewport: options.source_viewport,
                                                        sample_mode,
                                                    },
                                                );
                                                let mut pass = recorder.begin_color_pass(
                                                    "Blit Test",
                                                    &target.view,
                                                    options.load_op,
                                                );
                                                renderer.draw_prepared_composite(
                                                    &mut pass,
                                                    (64, 32),
                                                    &draw,
                                                );
                                            } else {
                                                renderer.encode_composite_to_view_pass(
                                                    recorder,
                                                    &device,
                                                    &source,
                                                    &target.view,
                                                    options,
                                                );
                                            }
                                        },
                                    )
                                })
                                .collect();
                            let differing = captures[0]
                                .iter()
                                .zip(&captures[1])
                                .filter(|(a, b)| a != b)
                                .count();
                            assert_eq!(
                                differing, 0,
                                "format={format:?} sample={sample_mode:?} mask={rounded_mask:?} blend={blend_mode:?} phase={phase} prepared={prepared}"
                            );
                        }
                    }
                }
            }
        }
    }
}
