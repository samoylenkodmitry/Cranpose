use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{BlendMode, ColorFilter, ImageBitmap, ImageSampling, Point, Rect};

use super::{
    DeviceRoundedClip, DrawSpace, FrameCommandRecorder, ImageTextures, PassDepth, TargetRect,
    blend_state_for_mode, confined_scissor, create_render_pipeline_logged,
    frame_uploads::FrameUploads, scissor_rect_for_layer,
};
use crate::{
    frame_graph::{BufferUpload, UploadAllocatorSpec},
    geometry::{
        SegmentTransform, axis_aligned_quad_rect, canonicalized_scaled_quad, scaled_quad,
        snap_delta_for_anchor, translate_quad,
    },
    instant_triangles::InstantTexels,
    rect_to_quad,
    scene::ImageDraw,
    shared_shader::SharedShader,
};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub(crate) struct Vertex {
    pub(crate) position: [f32; 2],
    pub(crate) color: [f32; 4],
    pub(crate) uv: [f32; 2],
    pub(crate) uv_bounds: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
        2 => Float32x2,
        3 => Float32x4,
    ];

    pub(crate) fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

pub(super) fn create_image_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    surface_format: wgpu::TextureFormat,
    shader: &SharedShader,
    blend_mode: BlendMode,
    variant: ImageVariant,
) -> wgpu::RenderPipeline {
    let module = shader.module();
    let ImageVariant {
        depth,
        alpha_mask,
        rounded,
    } = variant;
    let (vertex_entry, fragment_entry) = match (alpha_mask, rounded) {
        (false, false) => ("image_vs_main", "image_fs_main"),
        (true, false) => ("image_vs_main", "image_mask_fs_main"),
        (false, true) => ("image_rounded_vs_main", "image_rounded_fs_main"),
        (true, true) => ("image_rounded_vs_main", "image_mask_rounded_fs_main"),
    };
    // A rounded image reads its clip from an instance beside its vertices.
    let buffers = [Some(Vertex::desc()), Some(DeviceRoundedClip::desc())];
    create_render_pipeline_logged(
        device,
        cache,
        &format!(
            "image blend={blend_mode:?} depth={} alpha_mask={alpha_mask} rounded={rounded}",
            depth.is_on()
        ),
        wgpu::RenderPipelineDescriptor {
            label: Some("Image Pipeline"),
            layout: Some(shader.layout()),
            vertex: wgpu::VertexState {
                module,
                entry_point: Some(vertex_entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: if rounded { &buffers } else { &buffers[..1] },
            },
            fragment: Some(wgpu::FragmentState {
                module,
                entry_point: Some(fragment_entry),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(blend_state_for_mode(blend_mode)),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: depth.overlay_state(),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        },
    )
}

// The image draws a pass collects: their quads' vertices and indices,
// their rounded clips and their commands.
#[derive(Default)]
pub(crate) struct ImageBatch {
    pub(crate) vertices: Vec<Vertex>,
    pub(crate) indices: Vec<u32>,
    pub(crate) clips: Vec<DeviceRoundedClip>,
    pub(crate) cmds: Vec<ImageDrawCmd>,
}

impl ImageBatch {
    pub(crate) fn push_image_draw(
        &mut self,
        image_draw: &ImageDraw,
        space: DrawSpace,
        (textures, upload): (&mut ImageTextures, &mut FrameUploads<'_>),
    ) -> Result<(), String> {
        let (viewport, root_scale) = (space.viewport, space.scale);
        let snap_delta = image_draw
            .snap_anchor
            .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
            .unwrap_or_default();
        let rect = image_draw.rect.translate(snap_delta.x, snap_delta.y);
        if rect.width <= 0.0 || rect.height <= 0.0 || image_draw.alpha <= 0.0 {
            return Ok(());
        }

        let (mut tint, cpu_filter) =
            tint_for_image(image_draw.color_filter.as_ref(), image_draw.alpha);
        if tint[3] <= 0.0 {
            return Ok(());
        }

        let filtered_image = cpu_filter
            .map(|filter| apply_filter_to_bitmap(&image_draw.image, filter))
            .transpose()?;
        let prepared_image = filtered_image.as_ref().unwrap_or(&image_draw.image);
        if let cranpose_ui_graphics::ImagePixelFormat::Alpha8 { color } = prepared_image.format() {
            for (channel, value) in tint[..3].iter_mut().zip(color) {
                *channel *= value as f32 / 255.0;
            }
        }

        let geometry = resolve_image_geometry_with_delta(image_draw, root_scale, snap_delta);
        let Some(scissor) =
            scissor_rect_for_layer(geometry.rect, geometry.clip, root_scale, viewport)
        else {
            return Ok(());
        };

        let Some(uv_rect) = image_uv_rect(&image_draw.image, image_draw.src_rect) else {
            return Ok(());
        };
        let device_quad = geometry.device_quad(root_scale);
        let index_start = push_image_quad(
            (&mut self.vertices, &mut self.indices),
            device_quad,
            tint,
            &uv_rect,
        );
        let clip = DeviceRoundedClip::of(image_draw, root_scale).map(|clip| {
            self.clips.push(clip);
            self.clips.len() as u32 - 1
        });
        self.cmds.push(ImageDrawCmd {
            index_start,
            scissor,
            image: textures.binding(prepared_image, geometry.sampling, upload)?,
            clip,
        });
        Ok(())
    }

    pub(super) fn push_bitmap(
        &mut self,
        image: &ImageBitmap,
        (rect, clip): (Rect, Option<Rect>),
        sampling: ImageSampling,
        space: DrawSpace,
        (textures, upload): (&mut ImageTextures, &mut FrameUploads<'_>),
    ) -> Result<(), String> {
        let (viewport, root_scale) = (space.viewport, space.scale);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Ok(());
        }

        let (device_quad, scissor_rect) =
            if sampling == ImageSampling::Nearest && root_scale.is_finite() && root_scale > 0.0 {
                let left_px = (rect.x * root_scale).round();
                let top_px = (rect.y * root_scale).round();
                let width_px = (rect.width * root_scale).round().max(1.0);
                let height_px = (rect.height * root_scale).round().max(1.0);
                let snapped_rect = Rect {
                    x: left_px / root_scale,
                    y: top_px / root_scale,
                    width: width_px / root_scale,
                    height: height_px / root_scale,
                };
                let right_px = left_px + width_px;
                let bottom_px = top_px + height_px;
                (
                    [
                        [left_px, top_px],
                        [right_px, top_px],
                        [left_px, bottom_px],
                        [right_px, bottom_px],
                    ],
                    snapped_rect,
                )
            } else {
                (
                    rect_to_quad(rect).map(|[x, y]| [x * root_scale, y * root_scale]),
                    rect,
                )
            };

        let Some(scissor) = scissor_rect_for_layer(scissor_rect, clip, root_scale, viewport) else {
            return Ok(());
        };
        let Some(uv_rect) = image_uv_rect(image, None) else {
            return Ok(());
        };

        let index_start = push_image_quad(
            (&mut self.vertices, &mut self.indices),
            device_quad,
            [1.0, 1.0, 1.0, 1.0],
            &uv_rect,
        );
        self.cmds.push(ImageDrawCmd {
            index_start,
            scissor,
            image: textures.binding(image, sampling, upload)?,
            clip: None,
        });
        Ok(())
    }
}

pub(super) struct CachedImageTexture {
    pub(super) bind_groups: ImageBindGroups,
    pub(super) _texture: wgpu::Texture,
    pub(super) _view: wgpu::TextureView,
    pub(super) bytes: usize,
}

pub(super) struct ImageBindGroups {
    pub(super) alpha_mask: bool,
    pub(super) nearest: wgpu::BindGroup,
    pub(super) linear: wgpu::BindGroup,
}

impl ImageBindGroups {
    pub(super) fn binding(&self, sampling: ImageSampling) -> ImageBinding {
        ImageBinding {
            bind_group: match sampling {
                ImageSampling::Nearest => self.nearest.clone(),
                ImageSampling::Linear => self.linear.clone(),
            },
            alpha_mask: self.alpha_mask,
        }
    }
}

/// What an image's draw binds: its texture's bind group for the draw's
/// sampling, which keeps the texture alive whatever the cache evicts before
/// the pass is drawn, and whether the texture is an alpha mask.
pub(crate) struct ImageBinding {
    pub(super) bind_group: wgpu::BindGroup,
    pub(super) alpha_mask: bool,
}

/// The image pipeline a draw takes: in a pass with depth or not, for an
/// alpha mask or a color image, and taking a rounded clip in place or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ImageVariant {
    pub(super) depth: PassDepth,
    pub(super) alpha_mask: bool,
    pub(super) rounded: bool,
}

impl ImageVariant {
    // How the instant pipeline reads the image while this variant compiles.
    pub(super) fn texels(self) -> InstantTexels {
        if self.alpha_mask {
            InstantTexels::Coverage
        } else {
            InstantTexels::Straight
        }
    }
}

pub(crate) struct ImageDrawCmd {
    pub(super) index_start: u32,
    pub(super) scissor: (u32, u32, u32, u32),
    pub(super) image: ImageBinding,
    /// The rounded clip whose coverage the image takes in place: its index
    /// among the pass's clip instances.
    pub(super) clip: Option<u32>,
}

impl ImageDrawCmd {
    /// Target pixels the draw can touch: its scissor.
    pub(crate) fn bounds(&self) -> TargetRect {
        self.scissor
    }
}

/// Moves image quads and the rounded clips they take by `shift` (see
/// [`PlacementData::of`]) and keeps their commands within `slot`.
pub(crate) fn place_image_draws(
    (vertices, clips, cmds): (&mut [Vertex], &mut [DeviceRoundedClip], &mut [ImageDrawCmd]),
    [x, y]: [f32; 2],
    slot: TargetRect,
) {
    for vertex in vertices {
        vertex.position[0] += x;
        vertex.position[1] += y;
    }
    for clip in clips {
        clip.rect[0] += x;
        clip.rect[1] += y;
    }
    for cmd in cmds {
        cmd.scissor = confined_scissor(Some(cmd.scissor), slot);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ImageUvRect {
    pub(super) min: [f32; 2],
    pub(super) max: [f32; 2],
    pub(super) sample_bounds: [f32; 4],
}

/// A growable vertex buffer and index buffer pair.
/// One pass's image and shared glyph quads: the frame's vertex and index
/// uploads they were appended to.
pub(crate) struct ImageSlot {
    pub(super) vertices: BufferUpload,
    pub(super) indices: BufferUpload,
    /// The rounded clips the pass's images take in place, one instance each.
    pub(super) clips: Option<BufferUpload>,
}

impl ImageSlot {
    /// Uploads a pass's image and glyph quads into the frame's buffers.
    pub(crate) fn upload<C: FrameCommandRecorder>(
        recorder: &mut C,
        device: &wgpu::Device,
        vertices: &[Vertex],
        indices: &[u32],
        clips: &[DeviceRoundedClip],
    ) -> Self {
        Self {
            vertices: recorder.upload_buffer(
                image_vertex_spec(),
                device,
                bytemuck::cast_slice(vertices),
            ),
            indices: recorder.upload_buffer(
                image_index_spec(),
                device,
                bytemuck::cast_slice(indices),
            ),
            clips: (!clips.is_empty()).then(|| {
                recorder.upload_buffer(
                    UploadAllocatorSpec::vertex(
                        "Image Clip Buffer",
                        size_of::<DeviceRoundedClip>() as u64,
                    ),
                    device,
                    bytemuck::cast_slice(clips),
                )
            }),
        }
    }
}

pub(super) fn image_vertex_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::vertex("Image Vertex Buffer", size_of::<Vertex>() as u64)
}

pub(super) fn image_index_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::index("Image Index Buffer", size_of::<u32>() as u64)
}

pub(super) fn image_sampler_descriptor(
    sampling: ImageSampling,
) -> wgpu::SamplerDescriptor<'static> {
    let filter = match sampling {
        ImageSampling::Nearest => wgpu::FilterMode::Nearest,
        ImageSampling::Linear => wgpu::FilterMode::Linear,
    };
    wgpu::SamplerDescriptor {
        label: Some(match sampling {
            ImageSampling::Nearest => "Nearest Image Sampler",
            ImageSampling::Linear => "Linear Image Sampler",
        }),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    }
}

pub(super) fn tint_for_image(
    color_filter: Option<&ColorFilter>,
    alpha: f32,
) -> ([f32; 4], Option<&ColorFilter>) {
    let alpha = alpha.clamp(0.0, 1.0);
    match color_filter {
        Some(filter) if filter.supports_gpu_vertex_modulation() => {
            let Some(tint) = filter.gpu_vertex_tint() else {
                return ([1.0, 1.0, 1.0, alpha], Some(filter));
            };
            (
                [
                    tint[0].clamp(0.0, 1.0),
                    tint[1].clamp(0.0, 1.0),
                    tint[2].clamp(0.0, 1.0),
                    (tint[3] * alpha).clamp(0.0, 1.0),
                ],
                None,
            )
        }
        Some(filter) => ([1.0, 1.0, 1.0, alpha], Some(filter)),
        None => ([1.0, 1.0, 1.0, alpha], None),
    }
}

/// Appends an image quad over `device_quad` and its two triangles, and
/// returns the index its triangles start at.
pub(super) fn push_image_quad(
    (vertices, indices): (&mut Vec<Vertex>, &mut Vec<u32>),
    device_quad: [[f32; 2]; 4],
    color: [f32; 4],
    uv_rect: &ImageUvRect,
) -> u32 {
    let base_vertex = vertices.len() as u32;
    let index_start = indices.len() as u32;
    indices.extend_from_slice(&[
        base_vertex,
        base_vertex + 1,
        base_vertex + 2,
        base_vertex + 2,
        base_vertex + 1,
        base_vertex + 3,
    ]);
    let corners = [
        [uv_rect.min[0], uv_rect.min[1]],
        [uv_rect.max[0], uv_rect.min[1]],
        [uv_rect.min[0], uv_rect.max[1]],
        [uv_rect.max[0], uv_rect.max[1]],
    ];
    vertices.extend(
        device_quad
            .into_iter()
            .zip(corners)
            .map(|(position, uv)| Vertex {
                position,
                color,
                uv,
                uv_bounds: uv_rect.sample_bounds,
            }),
    );
    index_start
}

pub(super) fn image_uv_rect(image: &ImageBitmap, src_rect: Option<Rect>) -> Option<ImageUvRect> {
    let Some(src) = src_rect else {
        return Some(ImageUvRect {
            min: [0.0, 0.0],
            max: [1.0, 1.0],
            sample_bounds: [0.0, 0.0, 1.0, 1.0],
        });
    };

    let (u_min, u_max, u_bound_min, u_bound_max) =
        source_axis_uv(src.x, src.width, image.width() as f32)?;
    let (v_min, v_max, v_bound_min, v_bound_max) =
        source_axis_uv(src.y, src.height, image.height() as f32)?;

    Some(ImageUvRect {
        min: [u_min, v_min],
        max: [u_max, v_max],
        sample_bounds: [u_bound_min, v_bound_min, u_bound_max, v_bound_max],
    })
}

#[derive(Clone, Copy)]
pub(crate) struct ResolvedImageGeometry {
    pub(crate) rect: Rect,
    pub(crate) clip: Option<Rect>,
    pub(crate) sampling: ImageSampling,
    quad: [[f32; 2]; 4],
    snap_anchored: bool,
}

pub(crate) fn resolve_image_geometry(image: &ImageDraw, root_scale: f32) -> ResolvedImageGeometry {
    let snap_delta = image
        .snap_anchor
        .map(|anchor| snap_delta_for_anchor(anchor, root_scale))
        .unwrap_or_default();
    resolve_image_geometry_with_delta(image, root_scale, snap_delta)
}

pub(super) fn resolve_image_geometry_with_delta(
    image: &ImageDraw,
    root_scale: f32,
    snap_delta: Point,
) -> ResolvedImageGeometry {
    let mut rect = image.rect.translate(snap_delta.x, snap_delta.y);
    let mut quad = translate_quad(image.quad, snap_delta);
    snap_nearest_image_to_device_pixels(image.sampling, &mut rect, &mut quad, root_scale);
    ResolvedImageGeometry {
        rect,
        clip: image.clip,
        sampling: image.sampling,
        quad,
        snap_anchored: image.snap_anchor.is_some(),
    }
}

impl ResolvedImageGeometry {
    pub(crate) fn device_quad(self, root_scale: f32) -> [[f32; 2]; 4] {
        nearest_image_device_quad(self.sampling, self.quad, root_scale).unwrap_or_else(|| {
            if self.snap_anchored {
                canonicalized_scaled_quad(self.quad, root_scale)
            } else {
                scaled_quad(self.quad, root_scale)
            }
        })
    }
}

fn snap_nearest_image_to_device_pixels(
    sampling: ImageSampling,
    rect: &mut Rect,
    quad: &mut [[f32; 2]; 4],
    root_scale: f32,
) {
    if sampling != ImageSampling::Nearest || !root_scale.is_finite() || root_scale <= 0.0 {
        return;
    }

    let Some(quad_rect) = axis_aligned_quad_rect(*quad) else {
        return;
    };

    let left_px = (quad_rect.x * root_scale).round();
    let top_px = (quad_rect.y * root_scale).round();
    let width_px = (quad_rect.width * root_scale).round().max(1.0);
    let height_px = (quad_rect.height * root_scale).round().max(1.0);
    let snapped = Rect {
        x: left_px / root_scale,
        y: top_px / root_scale,
        width: width_px / root_scale,
        height: height_px / root_scale,
    };

    *rect = snapped;
    *quad = rect_to_quad(snapped);
}

fn nearest_image_device_quad(
    sampling: ImageSampling,
    quad: [[f32; 2]; 4],
    root_scale: f32,
) -> Option<[[f32; 2]; 4]> {
    if sampling != ImageSampling::Nearest || !root_scale.is_finite() || root_scale <= 0.0 {
        return None;
    }

    let rect = axis_aligned_quad_rect(quad)?;
    let left_px = (rect.x * root_scale).round();
    let top_px = (rect.y * root_scale).round();
    let width_px = (rect.width * root_scale).round().max(1.0);
    let height_px = (rect.height * root_scale).round().max(1.0);
    let right_px = left_px + width_px;
    let bottom_px = top_px + height_px;
    Some([
        [left_px, top_px],
        [right_px, top_px],
        [left_px, bottom_px],
        [right_px, bottom_px],
    ])
}

fn source_axis_uv(start: f32, extent: f32, image_extent: f32) -> Option<(f32, f32, f32, f32)> {
    if !start.is_finite()
        || !extent.is_finite()
        || !image_extent.is_finite()
        || extent == 0.0
        || image_extent <= 0.0
    {
        return None;
    }

    let end = start + extent;
    let edge_min = start.min(end).clamp(0.0, image_extent);
    let edge_max = start.max(end).clamp(0.0, image_extent);
    if edge_max <= edge_min {
        return None;
    }

    let center_min = edge_min + 0.5;
    let center_max = edge_max - 0.5;
    let (bound_min, bound_max) = if center_min <= center_max {
        (center_min, center_max)
    } else {
        let center = (edge_min + edge_max) * 0.5;
        (center, center)
    };

    Some((
        edge_min / image_extent,
        edge_max / image_extent,
        bound_min / image_extent,
        bound_max / image_extent,
    ))
}

pub(super) fn apply_filter_to_bitmap(
    image: &ImageBitmap,
    filter: &ColorFilter,
) -> Result<ImageBitmap, String> {
    let pixel_count = image.width() as usize * image.height() as usize;
    let mut filtered = Vec::with_capacity(pixel_count * 4);
    for index in 0..pixel_count {
        let pixel = image.rgba8_pixel(index);
        let rgba = [
            pixel[0] as f32 / 255.0,
            pixel[1] as f32 / 255.0,
            pixel[2] as f32 / 255.0,
            pixel[3] as f32 / 255.0,
        ];
        let out = filter.apply_rgba(rgba);
        filtered.push((out[0].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[1].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[2].clamp(0.0, 1.0) * 255.0).round() as u8);
        filtered.push((out[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    ImageBitmap::from_rgba8(image.width(), image.height(), filtered)
        .map_err(|error| format!("failed to build filtered bitmap: {error}"))
}

/// How a text raster samples its texels under a segment transform: as it
/// asks on the pixel grid, and filtered once a transform turns it off the
/// grid, where no texel lands on a pixel for nearest sampling to keep.
pub(super) fn sampling_under(
    sampling: ImageSampling,
    transform: SegmentTransform,
) -> ImageSampling {
    if transform.is_identity() {
        sampling
    } else {
        ImageSampling::Linear
    }
}
