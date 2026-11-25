//! Shared bi-planar YUV (NV12 and P010) GPU conversion pass for wgpu backends.
//!
//! Platform importers (Apple Metal IOSurface, Windows D3D11/DXGI, Linux DMABUF) hand their
//! zero-copy luma and chroma `wgpu::Texture` planes plus `VideoFrame` metadata to
//! [`BiPlanarConverter`], which runs one render pass into Cranpose's [`ExternalFrames`] ring.

use cranpose_services::{
    ColorPrimaries, ColorRange, ColorTransfer, CropRect, VideoColorInfo, VideoRotation,
};
use cranpose_ui::ImageBitmap;

use crate::{ExternalFrames, FrameWriter, GpuContext, video_transform::VideoSampleTransform};

// Five vec4<f32> uniforms (80 bytes).
const UNIFORM_SIZE: usize = 80;

// Sample bit depth of a bi-planar YUV buffer (8-bit NV12 or 10-bit P010 in upper 10 bits of u16).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YuvBitDepth {
    Bit8,
    Bit10,
}

// Pipeline specialization key: SDR BT.601/709 compiles out all HDR and gamut math.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PipelineKey {
    transfer: ColorTransfer,
    gamut_2020: bool,
}

impl PipelineKey {
    const fn of(color: VideoColorInfo) -> Self {
        Self {
            transfer: color.transfer,
            gamut_2020: matches!(color.primaries, ColorPrimaries::Bt2020),
        }
    }
}

// Converts bi-planar YUV GPU textures into RGBA8 `ImageBitmap`s in a reusable ring.
pub(crate) struct BiPlanarConverter {
    gpu: GpuContext,
    frames: ExternalFrames,
    writer: FrameWriter,
    size: (u32, u32),
    module: wgpu::ShaderModule,
    layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    // Cached specialized pipeline for the active transfer/gamut pair.
    pipeline: Option<(PipelineKey, wgpu::RenderPipeline)>,
}

// Geometry and color metadata of one decoded bi-planar picture.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BiPlanarFrameMeta {
    pub(crate) buffer_size: (u32, u32),
    pub(crate) crop: CropRect,
    pub(crate) color: VideoColorInfo,
    pub(crate) rotation: VideoRotation,
    pub(crate) bit_depth: YuvBitDepth,
}

impl BiPlanarConverter {
    pub(crate) fn new(gpu: &GpuContext) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video yuv to rgb"),
            source: wgpu::ShaderSource::Wgsl(
                cranpose_macros::include_wgsl!("video_convert.wgsl").into(),
            ),
        });
        let texture = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video yuv to rgb"),
            entries: &[
                texture(0),
                texture(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(UNIFORM_SIZE as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video yuv to rgb"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("video yuv to rgb"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video yuv to rgb"),
            size: UNIFORM_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let frames = ExternalFrames::new(
            gpu,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            true,
        );
        Self {
            gpu: gpu.clone(),
            writer: frames.writer(),
            frames,
            size: (0, 0),
            module,
            layout,
            pipeline_layout,
            sampler,
            uniforms,
            pipeline: None,
        }
    }

    pub(crate) fn convert(
        &mut self,
        luma: &wgpu::TextureView,
        chroma: &wgpu::TextureView,
        meta: BiPlanarFrameMeta,
    ) -> Option<ImageBitmap> {
        let target_size = VideoSampleTransform::target_size(meta.crop, meta.rotation);
        if target_size != self.size {
            self.frames.set_size(target_size.0, target_size.1);
            self.size = target_size;
        }
        let transform = VideoSampleTransform::new(meta.crop, meta.buffer_size, meta.rotation);
        let target = self.writer.begin()?;
        let uniform = ConversionUniform::new(meta.color, meta.bit_depth, transform);
        self.ensure_pipeline(PipelineKey::of(meta.color));
        let device = &self.gpu.device;
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video yuv to rgb"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(luma),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(chroma),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.uniforms.as_entire_binding(),
                },
            ],
        });
        self.gpu
            .queue
            .write_buffer(&self.uniforms, 0, &uniform.to_bytes());
        let pipeline = &self.pipeline.as_ref().expect("pipeline built").1;
        let mut commands = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("video yuv to rgb"),
        });
        {
            let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video yuv to rgb"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target.view(),
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([commands.finish()]);
        self.writer.publish(target.number());
        self.frames.next_frame()
    }

    fn ensure_pipeline(&mut self, key: PipelineKey) {
        let needs_build = self.pipeline.as_ref().is_none_or(|(k, _)| *k != key);
        if needs_build {
            let transfer_id = match key.transfer {
                ColorTransfer::Sdr => 0.0,
                ColorTransfer::Pq => 1.0,
                ColorTransfer::Hlg => 2.0,
            };
            let gamut_id = if key.gamut_2020 { 1.0 } else { 0.0 };
            let constants = [("TRANSFER", transfer_id), ("GAMUT_2020", gamut_id)];
            let pipeline =
                self.gpu
                    .device
                    .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                        label: Some("video yuv to rgb"),
                        layout: Some(&self.pipeline_layout),
                        vertex: wgpu::VertexState {
                            module: &self.module,
                            entry_point: Some("cover"),
                            compilation_options: Default::default(),
                            buffers: &[],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module: &self.module,
                            entry_point: Some("convert"),
                            compilation_options: wgpu::PipelineCompilationOptions {
                                constants: &constants,
                                ..Default::default()
                            },
                            targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
                        }),
                        primitive: Default::default(),
                        depth_stencil: None,
                        multisample: Default::default(),
                        multiview_mask: None,
                        cache: None,
                    });
            self.pipeline = Some((key, pipeline));
        }
    }
}

struct ConversionUniform {
    range: [f32; 4],
    weights: [f32; 4],
    origin_du: [f32; 4],
    dv_inv_target: [f32; 4],
    inside: [f32; 4],
}

impl ConversionUniform {
    fn new(color: VideoColorInfo, bit_depth: YuvBitDepth, transform: VideoSampleTransform) -> Self {
        let (kr, kb) = match color.primaries {
            ColorPrimaries::Bt601 => (0.299, 0.114),
            ColorPrimaries::Bt709 => (0.2126, 0.0722),
            ColorPrimaries::Bt2020 => (0.2627, 0.0593),
        };
        let kg = 1.0 - kr - kb;
        let (red, blue) = (2.0 * (1.0 - kr), 2.0 * (1.0 - kb));
        let range = match (color.range, bit_depth) {
            (ColorRange::Full, YuvBitDepth::Bit8) => [0.0, 1.0, 128.0 / 255.0, 1.0],
            (ColorRange::Limited, YuvBitDepth::Bit8) => {
                [16.0 / 255.0, 255.0 / 219.0, 128.0 / 255.0, 255.0 / 224.0]
            }
            // 10-bit P010 packs 10-bit samples into the upper 10 bits of a 16-bit normalized value
            // (`sample << 6`). Full range spans 0..=(1023 << 6) = 65472 / 65535; limited range
            // spans luma (64..=940) << 6 and chroma (64..=960) << 6 with center 512 << 6.
            (ColorRange::Full, YuvBitDepth::Bit10) => {
                [0.0, 65535.0 / 65472.0, 32768.0 / 65535.0, 65535.0 / 65472.0]
            }
            (ColorRange::Limited, YuvBitDepth::Bit10) => [
                4096.0 / 65535.0,
                65535.0 / 56064.0,
                32768.0 / 65535.0,
                65535.0 / 57344.0,
            ],
        };
        Self {
            range,
            weights: [red, -kb * blue / kg, -kr * red / kg, blue],
            origin_du: transform.origin_du,
            dv_inv_target: transform.dv_inv_target,
            inside: transform.inside,
        }
    }

    fn to_bytes(&self) -> [u8; UNIFORM_SIZE] {
        let mut bytes = [0; UNIFORM_SIZE];
        let (words, _) = bytes.as_chunks_mut::<4>();
        let values = self
            .range
            .into_iter()
            .chain(self.weights)
            .chain(self.origin_du)
            .chain(self.dv_inv_target)
            .chain(self.inside);
        for (word, value) in words.iter_mut().zip(values) {
            *word = value.to_ne_bytes();
        }
        bytes
    }
}
