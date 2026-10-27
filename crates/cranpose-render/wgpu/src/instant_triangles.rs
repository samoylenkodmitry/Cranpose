// Draws whose own pipeline still compiles, through one pipeline of textured
// triangles the renderer builds as it starts: #1947's instant tier.
//
// A pipeline takes 80 to 760 ms to compile on a Mali-G31 (Mi TV Stick) and
// 20 to 210 ms on a Mali-G76, most of it a cost every program pays however
// short, and a fresh install's first screen draws with about fifteen. So the
// renderer queues three programs as it starts, ahead of every other: the
// fallback triangles of shapes (`crate::mesh_fallback`), the plain glyph
// program, and this one, which shades triangles of image vertices with a
// texture. Images draw their own vertices through it; turned glyphs, shadows
// and layer composites lay triangles here while their pipelines compile. It
// draws what theirs draw, except that a rounded clip cuts as its scissor, a
// shadow's falloff runs straight from its edge to its umbra, and a tilted
// layer's texels follow its corners without perspective. A draw that blends other
// than source-over waits for its own pipeline. Each draw counts as a
// stand-in, so the frame after its pipeline lands draws it exactly.

use std::ops::Range;

use cranpose_render_common::layer_shadow::ShadowRRect;

use crate::{
    draw_pass::TrackedPass,
    frame_graph::{BufferUpload, FrameCommandRecorder, UploadAllocatorSpec},
    offscreen::OffscreenTarget,
    render::Vertex,
};

// How the instant fragment reads a draw's texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstantTexels {
    // A straight colour the vertex colour tints: an image.
    Straight,
    // An alpha mask whose red channel scales the vertex colour: glyphs and
    // alpha-mask images.
    Coverage,
    // A premultiplied colour the vertex colour scales: a layer's surface.
    Premultiplied,
    // No texel: the vertex colour alone, as a shadow's.
    Solid,
}

// The instant pipelines and what their draws bind beside their own. The
// web builds every pipeline where it draws, and takes none of this.
#[cfg(not(target_arch = "wasm32"))]
mod pipelines {
    use std::{borrow::Cow, sync::Arc};

    use wgpu::util::DeviceExt;

    use super::{InstantStandIn, InstantTexels};
    use crate::{
        fixed_pipeline::{FixedPipeline, FixedPipelineJob},
        render::{PassDepth, Vertex},
        shared_shader::{ShaderCode, SharedShader},
    };

    const INSTANT_FS_WGSL: &str = r"
    // How `instant_fs_main` takes its texel: 0 a straight colour, 1 the red
    // channel of an alpha mask, 2 a premultiplied colour, 3 none, for a solid
    // colour. The pipeline blends premultiplied colours.
    @group(2) @binding(0)
    var<uniform> instant_texels: vec4<u32>;

    @fragment
    fn instant_fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
        let mode = instant_texels.x;
        let uv = clamp(input.uv, input.uv_bounds.xy, input.uv_bounds.zw);
        let sampled = textureSampleLevel(image_texture, image_sampler, uv, 0.0);
        var texel = select(sampled, vec4<f32>(1.0, 1.0, 1.0, sampled.r), mode == 1u);
        texel = select(texel, vec4<f32>(1.0), mode == 3u);
        let color = texel * input.color;
        return vec4<f32>(color.rgb * select(color.a, 1.0, mode == 2u), color.a);
    }
    ";

    fn instant_wgsl() -> Cow<'static, str> {
        Cow::Owned(format!(
            "{}{INSTANT_FS_WGSL}",
            cranpose_ui_graphics::framework_shaders::IMAGE_WGSL
        ))
    }

    const INSTANT: ShaderCode = ShaderCode::wgsl(instant_wgsl);

    const INSTANT_PIPELINES: [&str; 2] = ["instant", "instant/depth"];

    const TEXELS: [InstantTexels; 4] = [
        InstantTexels::Straight,
        InstantTexels::Coverage,
        InstantTexels::Premultiplied,
        InstantTexels::Solid,
    ];

    // The instant pipelines, for passes without and with a depth buffer, and
    // what their draws bind beside their own: a bind group for each way of
    // reading texels, and a texture for draws that read none.
    pub(crate) struct InstantPipelines {
        pipelines: [FixedPipeline; 2],
        shader: SharedShader,
        texels: [wgpu::BindGroup; 4],
        untextured: wgpu::BindGroup,
    }

    impl InstantPipelines {
        // `layouts` are the viewport uniform's and the image texture's, which
        // the instant draws share with the image pipelines.
        pub(crate) fn new(
            device: &wgpu::Device,
            backend: wgpu::Backend,
            (uniform, texture): (&wgpu::BindGroupLayout, &wgpu::BindGroupLayout),
        ) -> Self {
            let texels_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Instant Texels Bind Group Layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                }],
            });
            let texels = TEXELS.map(|texels| {
                let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Instant Texels Buffer"),
                    contents: bytemuck::bytes_of(&[texels as u32, 0, 0, 0]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Instant Texels Bind Group"),
                    layout: &texels_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    }],
                })
            });
            let pixel = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Instant Untextured Pixel"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let untextured = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Instant Untextured Bind Group"),
                layout: texture,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(
                            &pixel.create_view(&wgpu::TextureViewDescriptor::default()),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(
                            &device.create_sampler(&wgpu::SamplerDescriptor::default()),
                        ),
                    },
                ],
            });
            Self {
                pipelines: INSTANT_PIPELINES.map(FixedPipeline::new),
                shader: SharedShader::new(
                    device,
                    backend,
                    "Instant Shader",
                    INSTANT,
                    &[Some(uniform), Some(texture), Some(&texels_layout)],
                ),
                texels,
                untextured,
            }
        }

        pub(crate) fn pipeline(&self, depth: PassDepth) -> &FixedPipeline {
            &self.pipelines[depth.index()]
        }

        // What a draw needs to stand in for its own pipeline in a pass with a
        // depth buffer or not, once the instant pipeline for it is built.
        pub(crate) fn stand_in(&self, depth: PassDepth) -> Option<InstantStandIn<'_>> {
            self.pipeline(depth).built().map(|pipeline| InstantStandIn {
                pipeline,
                texels: &self.texels,
                untextured: &self.untextured,
            })
        }

        // The creation of the instant pipeline for targets of `format`, in a
        // pass with a depth buffer when `depth`.
        pub(crate) fn job(
            &self,
            device: &Arc<wgpu::Device>,
            cache: Option<wgpu::PipelineCache>,
            format: wgpu::TextureFormat,
            depth: PassDepth,
        ) -> FixedPipelineJob {
            let device = Arc::clone(device);
            let shader = self.shader.clone();
            Box::new(move || {
                let module = shader.module();
                crate::render::create_render_pipeline_logged(
                    &device,
                    cache.as_ref(),
                    INSTANT_PIPELINES[depth.index()],
                    wgpu::RenderPipelineDescriptor {
                        label: Some("Instant Pipeline"),
                        layout: Some(shader.layout()),
                        vertex: wgpu::VertexState {
                            module,
                            entry_point: Some("image_vs_main"),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            buffers: &[Some(Vertex::desc())],
                        },
                        fragment: Some(wgpu::FragmentState {
                            module,
                            entry_point: Some("instant_fs_main"),
                            compilation_options: wgpu::PipelineCompilationOptions::default(),
                            targets: &[Some(wgpu::ColorTargetState {
                                format,
                                blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                                write_mask: wgpu::ColorWrites::ALL,
                            })],
                        }),
                        primitive: wgpu::PrimitiveState::default(),
                        depth_stencil: depth.overlay_state(),
                        multisample: wgpu::MultisampleState::default(),
                        multiview_mask: None,
                        cache: None,
                    },
                )
            })
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use pipelines::InstantPipelines;

// The built instant pipeline and what its draws bind beside their own.
#[derive(Clone, Copy)]
pub(crate) struct InstantStandIn<'a> {
    pipeline: &'a wgpu::RenderPipeline,
    texels: &'a [wgpu::BindGroup; 4],
    untextured: &'a wgpu::BindGroup,
}

impl InstantStandIn<'_> {
    // Sets the pipeline and how it reads the texture bound at group 1; the
    // viewport at group 0 and the texture are the draw's own.
    pub(crate) fn bind(self, pass: &mut TrackedPass<'_, '_>, texels: InstantTexels) {
        pass.set_pipeline(self.pipeline);
        pass.set_bind_group(2, &self.texels[texels as usize], &[]);
    }

    // Draws `indices` of the pass's instant triangles, under the viewport
    // bound at group 0, with `texture`, or with none.
    pub(crate) fn draw(
        self,
        pass: &mut TrackedPass<'_, '_>,
        buffers: &InstantBuffers,
        (texture, texels): (Option<&wgpu::BindGroup>, InstantTexels),
        indices: Range<u32>,
    ) {
        self.bind(pass, texels);
        pass.set_bind_group(1, texture.unwrap_or(self.untextured), &[]);
        pass.set_vertex_buffer(0, buffers.vertices.slice());
        pass.set_index_buffer(buffers.indices.slice(), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(indices, 0, 0..1);
    }
}

// The triangles a pass lays for the instant pipeline: image vertices in the
// space of the viewport each draw binds, and their indices.
#[derive(Default)]
pub(crate) struct InstantTriangles {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    // Where the pass's turned glyphs start, six indices a glyph, when it
    // laid them.
    turned_glyphs: Option<u32>,
    // The lines of a shadow part's grid across and down.
    grid: (Vec<f32>, Vec<f32>),
    // Whether a pass of the frame laid any.
    laid: bool,
}

// The pass's instant triangles on the GPU, and where its turned glyphs
// start among them when it laid those.
pub(crate) struct InstantBuffers {
    vertices: BufferUpload,
    indices: BufferUpload,
    turned_glyphs: Option<u32>,
}

impl InstantBuffers {
    // The pass's turned glyphs, when it laid them.
    pub(crate) fn glyphs<'a>(&'a self, stand_in: InstantStandIn<'a>) -> Option<InstantGlyphs<'a>> {
        Some(InstantGlyphs {
            stand_in,
            buffers: self,
            first: self.turned_glyphs?,
        })
    }
}

// Instant triangles a pass draws in place of a draw whose pipeline compiles:
// the texture they shade with, or none, how they read it, the viewport slot
// they draw under, their indices and their scissor.
pub(crate) struct InstantDraw<'a> {
    pub(crate) texture: Option<&'a wgpu::BindGroup>,
    pub(crate) texels: InstantTexels,
    pub(crate) uniform_slot: usize,
    pub(crate) indices: Range<u32>,
    pub(crate) scissor: Option<(u32, u32, u32, u32)>,
}

impl<'a> InstantDraw<'a> {
    // A layer's surface, `texture`, composited.
    pub(crate) fn layer(
        texture: &'a wgpu::BindGroup,
        uniform_slot: usize,
        indices: Range<u32>,
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> Self {
        Self {
            texture: Some(texture),
            texels: InstantTexels::Premultiplied,
            uniform_slot,
            indices,
            scissor,
        }
    }

    // Shadows, which shade with their colour alone.
    pub(crate) fn shadows(
        uniform_slot: usize,
        indices: Range<u32>,
        scissor: Option<(u32, u32, u32, u32)>,
    ) -> Self {
        Self {
            texture: None,
            texels: InstantTexels::Solid,
            uniform_slot,
            indices,
            scissor,
        }
    }
}

// A pass's turned glyphs laid as instant triangles, for a pass whose turned
// glyph pipeline still compiles.
#[derive(Clone, Copy)]
pub(crate) struct InstantGlyphs<'a> {
    stand_in: InstantStandIn<'a>,
    buffers: &'a InstantBuffers,
    first: u32,
}

impl InstantGlyphs<'_> {
    // Draws the turned glyphs `glyphs` of the pass with the atlas bound at
    // group 1, under the viewport bound at group 0.
    pub(crate) fn draw(self, pass: &mut TrackedPass<'_, '_>, glyphs: Range<u32>) {
        self.stand_in.bind(pass, InstantTexels::Coverage);
        pass.set_vertex_buffer(0, self.buffers.vertices.slice());
        pass.set_index_buffer(self.buffers.indices.slice(), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(
            self.first + glyphs.start * 6..self.first + glyphs.end * 6,
            0,
            0..1,
        );
    }
}

impl InstantTriangles {
    // Opens a frame. Triangles are laid only while pipelines compile, so the
    // storage left by a frame that laid none is freed: the first frames of a
    // launch lay a grid for every shadow on screen.
    pub(crate) fn begin_frame(&mut self) {
        if !std::mem::take(&mut self.laid) && self.indices.is_empty() {
            *self = Self::default();
        }
    }

    // Empties the triangles for the next pass of the frame.
    pub(crate) fn clear(&mut self) {
        self.laid |= !self.indices.is_empty();
        self.vertices.clear();
        self.indices.clear();
        self.turned_glyphs = None;
    }

    pub(crate) fn upload<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        device: &wgpu::Device,
    ) -> Option<InstantBuffers> {
        (!self.indices.is_empty()).then(|| InstantBuffers {
            turned_glyphs: self.turned_glyphs,
            vertices: recorder.upload_buffer(
                UploadAllocatorSpec::vertex("Instant Vertex Buffer", size_of::<Vertex>() as u64),
                device,
                bytemuck::cast_slice(&self.vertices),
            ),
            indices: recorder.upload_buffer(
                UploadAllocatorSpec::index("Instant Index Buffer", size_of::<u32>() as u64),
                device,
                bytemuck::cast_slice(&self.indices),
            ),
        })
    }

    fn quad(&mut self, corners: [Vertex; 4]) {
        let first = index(self.vertices.len());
        self.indices
            .extend([0, 1, 2, 2, 1, 3].map(|corner| first + corner));
        self.vertices.extend(corners);
    }

    // Lays the pass's turned glyphs, each by its four corners.
    pub(crate) fn lay_turned_glyphs(&mut self, glyphs: impl Iterator<Item = [Vertex; 4]>) {
        self.turned_glyphs = Some(index(self.indices.len()));
        for corners in glyphs {
            self.quad(corners);
        }
    }

    // Lays a layer's surface, `texels`, composited over `dest` (x, y, width
    // and height in the space of the viewport its draw binds) at `alpha`.
    pub(crate) fn lay_composite(
        &mut self,
        dest: (f32, f32, f32, f32),
        texels: LayerTexels,
        alpha: f32,
    ) -> Range<u32> {
        let start = index(self.indices.len());
        let (extent, uv_bounds) = (texels.extent(), texels.bounds());
        let (x, y, w, h) = dest;
        let corner = |right: bool, bottom: bool| Vertex {
            position: [
                x + if right { w } else { 0.0 },
                y + if bottom { h } else { 0.0 },
            ],
            color: [alpha; 4],
            uv: texels.uv([
                if right { extent[0] } else { 0.0 },
                if bottom { extent[1] } else { 0.0 },
            ]),
            uv_bounds,
        };
        self.quad([
            corner(false, false),
            corner(true, false),
            corner(false, true),
            corner(true, true),
        ]);
        start..index(self.indices.len())
    }

    // Lays a tilted layer's surface, `texels`, at `alpha`: the triangle
    // strip `strip`, whose points `inverse` maps back to the surface's
    // texels. The texels follow the strip's corners without perspective.
    pub(crate) fn lay_projective(
        &mut self,
        strip: &[[f32; 2]],
        (inverse, texels): ([[f32; 3]; 3], LayerTexels),
        alpha: f32,
    ) -> Range<u32> {
        let start = index(self.indices.len());
        let uv_bounds = texels.bounds();
        let first = index(self.vertices.len());
        self.vertices.extend(strip.iter().map(|&[x, y]| {
            let row = |row: [f32; 3]| row[0] * x + row[1] * y + row[2];
            let w = row(inverse[2]);
            let w = if w.abs() > f32::EPSILON {
                w
            } else {
                f32::EPSILON
            };
            Vertex {
                position: [x, y],
                color: [alpha; 4],
                uv: texels.uv([row(inverse[0]) / w, row(inverse[1]) / w]),
                uv_bounds,
            }
        }));
        for corner in 0..index(strip.len()).saturating_sub(2) {
            self.indices
                .extend([first + corner, first + corner + 1, first + corner + 2]);
        }
        start..index(self.indices.len())
    }

    // Lays the shadow parts `parts` gives, each a rect of the shadow it
    // falls in and that shadow's colour: a grid over the rect (see
    // `grid_lines`), each corner taking the shadow's coverage there.
    pub(crate) fn lay_shadows(
        &mut self,
        parts: impl Iterator<Item = ([f32; 4], ShadowRRect, [f32; 4])>,
    ) -> Range<u32> {
        let start = index(self.indices.len());
        let Self {
            vertices,
            indices,
            grid: (xs, ys),
            ..
        } = self;
        for (rect, shadow, color) in parts {
            let bounds = shadow.bounds;
            let hole = shadow.hole.map_or(-1.0, |hole| hole.inset);
            grid_lines(
                xs,
                (rect[0], rect[2]),
                (bounds.x, bounds.x + bounds.width),
                (shadow.umbra_inset, hole),
            );
            grid_lines(
                ys,
                (rect[1], rect[3]),
                (bounds.y, bounds.y + bounds.height),
                (shadow.umbra_inset, hole),
            );
            let first = index(vertices.len());
            for &y in ys.iter() {
                for &x in xs.iter() {
                    let coverage = shadow.coverage(x, y);
                    vertices.push(Vertex {
                        position: [x, y],
                        color: [color[0], color[1], color[2], color[3] * coverage],
                        uv: [0.0; 2],
                        uv_bounds: [0.0; 4],
                    });
                }
            }
            let row = index(xs.len());
            for line in 0..index(ys.len()).saturating_sub(1) {
                for column in 0..row.saturating_sub(1) {
                    let corner = first + line * row + column;
                    indices.extend([
                        corner,
                        corner + 1,
                        corner + row,
                        corner + row,
                        corner + 1,
                        corner + row + 1,
                    ]);
                }
            }
        }
        start..index(indices.len())
    }
}

// The texels a layer's composite reads: its surface's texture, and the
// region of it that holds the layer, or none when the layer has all of it.
#[derive(Clone, Copy)]
pub(crate) struct LayerTexels {
    size: [f32; 2],
    region: Option<(f32, f32, f32, f32)>,
}

impl LayerTexels {
    pub(crate) fn new(surface: &OffscreenTarget, region: Option<(f32, f32, f32, f32)>) -> Self {
        Self {
            size: [surface.width as f32, surface.height as f32],
            region,
        }
    }

    // The region's width and height in texels.
    fn extent(self) -> [f32; 2] {
        self.region
            .map_or(self.size, |(_, _, width, height)| [width, height])
    }

    // Where the region's texel `[x, y]` is in the texture.
    fn uv(self, [x, y]: [f32; 2]) -> [f32; 2] {
        let (left, top) = self
            .region
            .map_or((0.0, 0.0), |(left, top, _, _)| (left, top));
        [(left + x) / self.size[0], (top + y) / self.size[1]]
    }

    // The bounds a draw of the region samples within: its texel centres, as
    // a texture of its own would clamp to its edge, or the whole texture.
    fn bounds(self) -> [f32; 4] {
        let [width, height] = self.size;
        self.region.map_or([0.0, 0.0, 1.0, 1.0], |(x, y, w, h)| {
            [
                (x + 0.5) / width,
                (y + 0.5) / height,
                (x + w - 0.5) / width,
                (y + h - 0.5) / height,
            ]
        })
    }
}

// A count of vertices or indices as an index: a pass lays far fewer than
// `u32::MAX`.
fn index(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

// The grid lines across `span` of a shadow part that spans `edges` of its
// shadow: the span's ends, and the lines within it at the edges, at the
// umbra, `umbra` in from them, and at the hole, `hole` in when it has one.
// Each falloff runs straight between edge and umbra: on a card's elevation
// shadow within 4 levels of the shadow pipeline's curve, where four steps
// across it took five times the vertices.
fn grid_lines(
    lines: &mut Vec<f32>,
    span: (f32, f32),
    edges: (f32, f32),
    (umbra, hole): (f32, f32),
) {
    lines.clear();
    lines.extend([span.0, span.1]);
    lines.extend([edges.0, edges.0 + umbra, edges.1 - umbra, edges.1]);
    if hole >= 0.0 {
        lines.extend([edges.0 + hole, edges.1 - hole]);
    }
    lines.retain(|&line| line >= span.0 && line <= span.1);
    lines.sort_by(f32::total_cmp);
    lines.dedup();
}
