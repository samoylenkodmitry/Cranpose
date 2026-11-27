//! Zero-copy WebGPU and WebGL2 video importer for `VideoFrame` and `HTMLVideoElement`.
//!
//! Copies directly on the GPU through `wgpu::Queue::copy_external_image_to_texture` (mapped to
//! `GPUQueue.copyExternalImageToTexture` on WebGPU and `WebGL2RenderingContext.texSubImage2D` on
//! WebGL2) with zero Canvas2D CPU pixel readback. Uncropped, unrotated frames write straight into
//! Cranpose's [`ExternalFrames`] ring with no extra render pass; cropped or rotated frames reuse
//! one staging GPU texture and apply [`VideoSampleTransform`] in a single fullscreen-triangle pass.

use cranpose_services::{CropRect, VideoFrame, VideoRotation, WebVideoSource};
use cranpose_ui::ImageBitmap;

use crate::{
    ExternalFrames, FrameWriter, GpuContext, cover_pipeline::cover_pipeline,
    video_transform::VideoSampleTransform,
};

// Three vec4<f32> uniforms (48 bytes).
const TRANSFORM_UNIFORM_SIZE: usize = 48;

struct BlitPass {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    staging: Option<StagingTexture>,
}

struct StagingTexture {
    size: (u32, u32),
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

// Imports WebCodecs `VideoFrame` and `HTMLVideoElement` frames directly on the GPU.
pub(crate) struct WebVideoConverter {
    gpu: GpuContext,
    frames: ExternalFrames,
    writer: FrameWriter,
    size: (u32, u32),
    blit: Option<BlitPass>,
}

impl WebVideoConverter {
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        let frames = ExternalFrames::new(
            gpu,
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::RENDER_ATTACHMENT,
            true,
        );
        Some(Self {
            gpu: gpu.clone(),
            writer: frames.writer(),
            frames,
            size: (0, 0),
            blit: None,
        })
    }

    pub(crate) fn convert(&mut self, frame: VideoFrame) -> Option<ImageBitmap> {
        let src_size = frame.size();
        if src_size.0 == 0 || src_size.1 == 0 {
            return None;
        }
        let crop = frame.crop();
        let rotation = frame.rotation();
        let target_size = VideoSampleTransform::target_size(crop, rotation);
        if target_size != self.size {
            self.frames.set_size(target_size.0, target_size.1);
            self.size = target_size;
        }
        let target = self.writer.begin()?;
        let source = match frame.web_source() {
            // Use `Clone::clone` (cheap wasm-bindgen handle clone) rather than JS `VideoFrame.clone()`
            // so no extra unclosed WebCodecs handle is allocated.
            WebVideoSource::VideoFrame(vf) => {
                wgpu::ExternalImageSource::VideoFrame(Clone::clone(vf))
            }
            WebVideoSource::HtmlVideoElement(el) => {
                wgpu::ExternalImageSource::HTMLVideoElement(Clone::clone(el))
            }
        };
        let is_identity = rotation == VideoRotation::Degrees0
            && crop.x == 0
            && crop.y == 0
            && crop.width == src_size.0
            && crop.height == src_size.1;
        if is_identity {
            copy_external_to_texture(&self.gpu.queue, source, target.texture(), src_size);
            // Flush pending queue writes so WebGL2 executes `texSubImage2D` before `drop(frame)`
            // closes a WebCodecs `VideoFrame`.
            self.gpu.queue.submit([]);
        } else {
            self.copy_transformed(source, src_size, crop, rotation, target.view());
        }
        self.writer.publish(target.number());
        // Dropping `frame` closes a WebCodecs `VideoFrame` immediately after the GPU copy is queued.
        drop(frame);
        self.frames.next_frame()
    }

    fn copy_transformed(
        &mut self,
        source: wgpu::ExternalImageSource,
        src_size: (u32, u32),
        crop: CropRect,
        rotation: VideoRotation,
        target_view: &wgpu::TextureView,
    ) {
        let blit = self.blit.get_or_insert_with(|| BlitPass::new(&self.gpu));
        blit.ensure_staging(&self.gpu.device, src_size);
        let staging = blit.staging.as_ref().expect("staging texture initialized");
        copy_external_to_texture(&self.gpu.queue, source, &staging.texture, src_size);
        let transform = VideoSampleTransform::new(crop, src_size, rotation);
        self.gpu
            .queue
            .write_buffer(&blit.uniforms, 0, &transform_bytes(transform));
        let mut commands =
            self.gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("web video blit"),
                });
        {
            let mut pass = commands.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("web video blit"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&blit.pipeline);
            pass.set_bind_group(0, &staging.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.gpu.queue.submit([commands.finish()]);
    }
}

impl BlitPass {
    fn new(gpu: &GpuContext) -> Self {
        let device = &gpu.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("web video blit"),
            source: wgpu::ShaderSource::Wgsl(
                cranpose_macros::include_wgsl!("video_blit.wgsl").into(),
            ),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("web video blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(TRANSFORM_UNIFORM_SIZE as u64),
                    },
                    count: None,
                },
            ],
        });
        let pipeline = cover_pipeline(
            device,
            "web video blit",
            &layout,
            &module,
            "blit",
            wgpu::TextureFormat::Rgba8Unorm,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("web video blit"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("web video blit"),
            size: TRANSFORM_UNIFORM_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline,
            sampler,
            uniforms,
            staging: None,
        }
    }

    fn ensure_staging(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        let needs_new = self.staging.as_ref().is_none_or(|s| s.size != size);
        if needs_new {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("web video staging"),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::COPY_DST
                    | wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("web video staging"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: self.uniforms.as_entire_binding(),
                    },
                ],
            });
            self.staging = Some(StagingTexture {
                size,
                texture,
                bind_group,
            });
        }
    }
}

fn copy_external_to_texture(
    queue: &wgpu::Queue,
    source: wgpu::ExternalImageSource,
    texture: &wgpu::Texture,
    size: (u32, u32),
) {
    queue.copy_external_image_to_texture(
        &wgpu::CopyExternalImageSourceInfo {
            source,
            origin: wgpu::Origin2d::ZERO,
            flip_y: false,
        },
        wgpu::CopyExternalImageDestInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
            color_space: wgpu::PredefinedColorSpace::Srgb,
            premultiplied_alpha: false,
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
}

fn transform_bytes(transform: VideoSampleTransform) -> [u8; TRANSFORM_UNIFORM_SIZE] {
    let mut bytes = [0; TRANSFORM_UNIFORM_SIZE];
    let (words, _) = bytes.as_chunks_mut::<4>();
    let values = transform
        .origin_du
        .into_iter()
        .chain(transform.dv_inv_target)
        .chain(transform.inside);
    for (word, value) in words.iter_mut().zip(values) {
        *word = value.to_ne_bytes();
    }
    bytes
}
