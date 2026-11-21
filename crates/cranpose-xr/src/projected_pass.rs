//! The pictures that the projection layer draws in place of layers of their own: the panels and
//! handles of a frame that has more layers than the runtime takes.
//!
//! The pass draws each picture into both eyes' images of the projection layer, over the 3D content,
//! in the order in which the compositor would draw their layers. Where the images carry depth, it
//! tests and writes that depth, so the layers over the projection layer test against the pictures
//! as they test against the volumes. A panel samples its part of the atlas, and a handle the
//! picture of its sphere. The pass exists from the first frame that draws a picture, so a frame
//! within the runtime's limit costs nothing.

use cranpose::GpuContext;

use crate::{Pose, eye_camera::EyeCamera, handle::HandleKind};

// The floats of one picture in the instance buffer: five vectors of four.
const FLOATS: usize = 20;
const STRIDE: u64 = (FLOATS * size_of::<f32>()) as u64;
// The vertices of a picture: two triangles for each of the shader's 32 strips
const VERTICES: u32 = 6 * 32;

// What a picture that the projection layer draws shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Picture {
    // A panel's part of the atlas: left, top, width and height in pixels
    Atlas([u32; 4]),
    // The sphere of a handle
    Handle(HandleKind),
}

// A picture that the projection layer draws in place of a layer of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProjectedLayer {
    pub(crate) picture: Picture,
    // The pose of the picture's center. The picture faces its +z axis.
    pub(crate) pose: Pose,
    // The width and the height in meters. The width of a curved picture is its arc length.
    pub(crate) size: [f32; 2],
    // The radius of the cylinder that the picture bends around, or `None` for a flat picture
    pub(crate) radius: Option<f32>,
}

// The image of one eye that the pass draws into.
pub(crate) struct EyeTarget<'a> {
    pub(crate) color: &'a wgpu::TextureView,
    // The image's depth, in the depth format of the pass
    pub(crate) depth: Option<&'a wgpu::TextureView>,
    // The image's size in pixels
    pub(crate) size: (u32, u32),
    // Whether the image holds this frame's picture of the 3D content. If not, the pass clears it.
    pub(crate) drawn: bool,
}

// A texture that pictures sample, with the group that binds it.
struct Bound {
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
}

// The uniform buffer of one eye, with the group that binds it.
struct EyeUniform {
    buffer: wgpu::Buffer,
    group: wgpu::BindGroup,
}

pub(crate) struct ProjectedPass {
    // The pipelines that draw into an image without depth and into one with depth
    flat: wgpu::RenderPipeline,
    deep: Option<wgpu::RenderPipeline>,
    picture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    eyes: [EyeUniform; 2],
    // The pictures of the frame, `STRIDE` bytes for each
    instances: wgpu::Buffer,
    bytes: Vec<u8>,
    // The spheres of the handles, in the order of `HandleKind::ALL`
    handles: [Bound; 2],
    // The atlas that the frame's panels sample, bound once for each texture that the atlas is
    atlas: Option<Bound>,
}

impl ProjectedPass {
    // The pass that draws into images of `color` format, with depth of `depth` format where the
    // images carry it. `handle` gives the picture of each handle's sphere: its side in pixels and its
    // rows of RGBA bytes, the top row first.
    pub(crate) fn new(
        gpu: &GpuContext,
        color: wgpu::TextureFormat,
        depth: Option<wgpu::TextureFormat>,
        handle: impl Fn(HandleKind) -> (u32, Vec<u8>),
    ) -> Self {
        let device = &gpu.device;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("cranpose xr projected layers"),
            source: wgpu::ShaderSource::Wgsl(include_str!("projected_pass.wgsl").into()),
        });
        let (eye_layout, picture_layout) = layouts(device);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cranpose xr projected layers"),
            bind_group_layouts: &[Some(&eye_layout), Some(&picture_layout)],
            immediate_size: 0,
        });
        let pipeline =
            |depth: Option<wgpu::TextureFormat>| pipeline(device, &layout, &shader, color, depth);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cranpose xr projected layers"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let eye = || {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("cranpose xr projected eye"),
                size: 16 * size_of::<f32>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("cranpose xr projected eye"),
                layout: &eye_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            EyeUniform { buffer, group }
        };
        let handles = HandleKind::ALL.map(|kind| {
            let texture = picture_texture(gpu, color, handle(kind));
            bound(device, &picture_layout, &sampler, texture)
        });
        Self {
            flat: pipeline(None),
            deep: depth.map(|depth| pipeline(Some(depth))),
            sampler,
            eyes: [eye(), eye()],
            instances: instance_buffer(device, 0),
            bytes: Vec::new(),
            handles,
            atlas: None,
            picture_layout,
        }
    }

    // Takes the pictures of the frame, which sample `atlas`. Both eyes draw the same ones.
    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        layers: &[ProjectedLayer],
        atlas: Option<&wgpu::Texture>,
    ) {
        if let Some(atlas) = atlas
            && self
                .atlas
                .as_ref()
                .is_none_or(|bound| bound.texture != *atlas)
        {
            self.atlas = Some(bound(
                &gpu.device,
                &self.picture_layout,
                &self.sampler,
                atlas.clone(),
            ));
        }
        let atlas_size = atlas.map_or([1.0; 2], |atlas| {
            [atlas.width() as f32, atlas.height() as f32]
        });
        self.bytes.clear();
        for layer in layers {
            let part = match layer.picture {
                Picture::Atlas([x, y, width, height]) => [
                    x as f32 / atlas_size[0],
                    y as f32 / atlas_size[1],
                    (x + width) as f32 / atlas_size[0],
                    (y + height) as f32 / atlas_size[1],
                ],
                Picture::Handle(_) => [0.0, 0.0, 1.0, 1.0],
            };
            let axis = |direction, extent: f32| {
                let [x, y, z] = layer.pose.rotate(direction);
                [x, y, z, extent]
            };
            let [x, y, z] = layer.pose.position;
            let vectors = [
                [x, y, z, layer.radius.unwrap_or(0.0)],
                axis([1.0, 0.0, 0.0], layer.size[0]),
                axis([0.0, 1.0, 0.0], layer.size[1]),
                axis([0.0, 0.0, 1.0], 0.0),
                part,
            ];
            self.bytes.extend(
                vectors
                    .as_flattened()
                    .iter()
                    .flat_map(|value| value.to_le_bytes()),
            );
        }
        if self.instances.size() < self.bytes.len() as u64 {
            self.instances = instance_buffer(&gpu.device, self.bytes.len() as u64);
        }
        gpu.queue.write_buffer(&self.instances, 0, &self.bytes);
    }

    // Records the draw of `layers`, the pictures that `prepare` took, into `target`, the image of
    // the eye at `eye` (0 for the left one) that `camera` sees.
    pub(crate) fn draw(
        &self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        (eye, camera): (usize, &EyeCamera),
        target: &EyeTarget<'_>,
        layers: &[ProjectedLayer],
    ) {
        let uniform = &self.eyes[eye];
        let columns: [[f32; 4]; 4] = {
            let rows = camera.clip_from_room(target.size);
            std::array::from_fn(|column| rows.map(|row| row[column]))
        };
        let mut bytes = [0_u8; 64];
        for (chunk, value) in bytes
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(columns.as_flattened())
        {
            *chunk = value.to_le_bytes();
        }
        gpu.queue.write_buffer(&uniform.buffer, 0, &bytes);
        let (load_color, load_depth) = if target.drawn {
            (wgpu::LoadOp::Load, wgpu::LoadOp::Load)
        } else {
            (
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                wgpu::LoadOp::Clear(1.0),
            )
        };
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cranpose xr projected layers"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target.color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: load_color,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: target.depth.map(|view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: load_depth,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            ..Default::default()
        });
        let pipeline = match (target.depth, &self.deep) {
            (Some(_), Some(deep)) => deep,
            _ => &self.flat,
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &uniform.group, &[]);
        for (index, layer) in layers.iter().enumerate() {
            let bound = match layer.picture {
                Picture::Atlas(_) => self.atlas.as_ref(),
                Picture::Handle(HandleKind::Move) => Some(&self.handles[0]),
                Picture::Handle(HandleKind::Resize) => Some(&self.handles[1]),
            };
            let Some(bound) = bound else {
                continue;
            };
            let start = index as u64 * STRIDE;
            pass.set_bind_group(1, &bound.group, &[]);
            pass.set_vertex_buffer(0, self.instances.slice(start..start + STRIDE));
            pass.draw(0..VERTICES, 0..1);
        }
    }
}

// The layouts of the eye's uniform and of a picture's texture with its sampler.
fn layouts(device: &wgpu::Device) -> (wgpu::BindGroupLayout, wgpu::BindGroupLayout) {
    let eye_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cranpose xr projected eye"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let picture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("cranpose xr projected picture"),
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
        ],
    });
    (eye_layout, picture_layout)
}

// A texture of `color` format that holds `(side, pixels)`: a square picture in rows of RGBA bytes.
fn picture_texture(
    gpu: &GpuContext,
    color: wgpu::TextureFormat,
    (side, pixels): (u32, Vec<u8>),
) -> wgpu::Texture {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("cranpose xr projected handle"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: color,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 4),
            rows_per_image: Some(side),
        },
        texture.size(),
    );
    texture
}

fn bound(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    texture: wgpu::Texture,
) -> Bound {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cranpose xr projected picture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Bound { texture, group }
}

// A buffer for at least `bytes` of pictures, with room for 16 pictures at the least, so a frame
// that projects a few more pictures than the last one makes no new buffer.
fn instance_buffer(device: &wgpu::Device, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("cranpose xr projected layers"),
        size: bytes.next_power_of_two().max(16 * STRIDE),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

// The pipeline that draws pictures into images of `color` format and, with `depth`, tests and
// writes their depth. The pictures blend with premultiplied alpha, as the compositor blends layers.
fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    color: wgpu::TextureFormat,
    depth: Option<wgpu::TextureFormat>,
) -> wgpu::RenderPipeline {
    let vectors = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x4,
    ];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("cranpose xr projected layers"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: STRIDE,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &vectors,
            })],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: depth.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: color,
                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}
