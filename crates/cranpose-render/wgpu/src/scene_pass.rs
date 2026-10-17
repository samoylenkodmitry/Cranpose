//! The 3D pass of a scene: each of its mesh surfaces drawn from its rendered
//! content, with a depth buffer so nearer surfaces hide farther ones, and,
//! for a light that casts shadows, a shadow map its casters are drawn into
//! first.

use std::{cell::OnceCell, sync::Arc};

use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{
    Rect,
    space::{Mat4, Mesh, Vec3},
};
use wgpu::util::DeviceExt;

use crate::{
    collect::SceneDraw,
    fixed_pipeline::FixedPipeline,
    frame_graph::UploadAllocatorSpec,
    idle_pool::{IDLE_FRAMES, IdlePool},
    pipeline_compiler::CompilerSend,
    render::{DEPTH_FORMAT, create_render_pipeline_logged},
    shaders,
    shared_shader::SharedShader,
};

/// The meshes whose buffers stay on the GPU between frames.
const KEPT_MESHES: usize = 64;
/// The side of the shadow map, in texels.
const SHADOW_MAP_SIZE: u32 = 1024;
/// How far toward the light a point is taken, in the shadow map's depth,
/// so a caster does not shadow itself where it is drawn.
const SHADOW_DEPTH_BIAS: f32 = 0.003;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SceneVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}

/// The block `mesh_surface.wgsl` reads for one surface.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SceneSurfaceUniforms {
    clip_from_model: [[f32; 4]; 4],
    scene_from_model: [[f32; 4]; 4],
    light_clip_from_model: [[f32; 4]; 4],
    normal_rows: [[f32; 4]; 3],
    eye: [f32; 4],
    light: [f32; 4],
    source_region: [f32; 4],
    params: [f32; 4],
    shadow: [f32; 4],
}

/// One mesh's vertices and indices on the GPU.
#[derive(Clone)]
pub(crate) struct SceneMeshBuffers {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
}

/// What the scene pipelines are built and bound from, made when a scene
/// first draws.
pub(crate) struct ScenePipelineParts {
    shader: SharedShader,
    pub(crate) uniform_layout: wgpu::BindGroupLayout,
    shadow_layout: wgpu::BindGroupLayout,
    shadow_sampler: wgpu::Sampler,
    /// Bound in the shadow map's place for a scene that casts no shadow.
    pub(crate) no_shadow: wgpu::BindGroup,
}

/// The depth of a scene's shadow casters as its light sees them.
pub(crate) struct ShadowMap {
    pub(crate) view: wgpu::TextureView,
    pub(crate) group: wgpu::BindGroup,
}

/// The scene pipelines, the buffers of the meshes recent frames drew and
/// the shadow map.
pub(crate) struct ScenePass {
    parts: OnceCell<ScenePipelineParts>,
    pub(crate) pipeline: FixedPipeline,
    pub(crate) shadowed_pipeline: FixedPipeline,
    pub(crate) shadow_pipeline: FixedPipeline,
    meshes: IdlePool<(u64, SceneMeshBuffers)>,
    pub(crate) shadow_map: Option<ShadowMap>,
    /// The frames since a scene last drew shadows.
    shadow_map_idle: u64,
}

impl Default for ScenePass {
    fn default() -> Self {
        Self {
            parts: OnceCell::new(),
            pipeline: FixedPipeline::new("scene/mesh-surface"),
            shadowed_pipeline: FixedPipeline::new("scene/mesh-surface-shadowed"),
            shadow_pipeline: FixedPipeline::new("scene/mesh-shadow"),
            meshes: IdlePool::default(),
            shadow_map: None,
            shadow_map_idle: 0,
        }
    }
}

/// Which of the scene pipelines a draw takes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenePipeline {
    /// Surfaces of a scene without shadows.
    Plain,
    /// Surfaces that read the scene's shadow map.
    Shadowed,
    /// Casters drawn into the shadow map.
    Shadow,
}

/// Where a scene's 3D pass draws: a page's pixels per logical pixel, the
/// device offset its origin sits at, and its size in pixels.
#[derive(Clone, Copy)]
pub(crate) struct ScenePage {
    pub(crate) scale: f32,
    pub(crate) offset: [f32; 2],
    pub(crate) size: (u32, u32),
}

/// How one surface is drawn: placed by `transform` in the scene, shaded or
/// not, at `alpha`, from the content's texels in its texture, and where it
/// lies in the scene's shadow map.
pub(crate) struct SurfaceShading {
    pub(crate) transform: Mat4,
    pub(crate) lit: bool,
    pub(crate) alpha: f32,
    /// The content's top left texel and its size in texels.
    pub(crate) source_region: [f32; 4],
    /// Maps the scene into its shadow map, for a scene that casts shadows.
    pub(crate) light_projection: Option<Mat4>,
}

impl ScenePass {
    /// The shader, layouts and samplers, made on the first call;
    /// `texture_layout` binds the surface's texture.
    pub(crate) fn parts(
        &self,
        device: &wgpu::Device,
        backend: wgpu::Backend,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> &ScenePipelineParts {
        self.parts
            .get_or_init(|| pipeline_parts(device, backend, texture_layout))
    }

    /// The shadow map, made the first time a scene casts shadows.
    pub(crate) fn shadow_map(&mut self, device: &wgpu::Device) -> Option<&ShadowMap> {
        self.shadow_map_idle = 0;
        if self.shadow_map.is_none() {
            let parts = self.parts.get()?;
            self.shadow_map = Some(create_shadow_map(device, parts));
        }
        self.shadow_map.as_ref()
    }

    /// The buffers of `mesh`, uploaded the first time a frame draws it.
    pub(crate) fn mesh_buffers(&mut self, device: &wgpu::Device, mesh: &Mesh) -> SceneMeshBuffers {
        let id = mesh.id();
        let buffers = match self.meshes.take(|(kept, _)| *kept == id) {
            Some((_, buffers)) => buffers,
            None => upload_mesh(device, mesh),
        };
        self.meshes
            .put((id, buffers.clone()), KEPT_MESHES, u64::MAX, |_| 0);
        buffers
    }

    /// Drops the buffers of meshes no recent frame drew, and the shadow map
    /// once no recent frame cast shadows.
    pub(crate) fn end_frame(&mut self) {
        self.meshes.end_frame();
        self.shadow_map_idle += 1;
        if self.shadow_map_idle > IDLE_FRAMES {
            self.shadow_map = None;
        }
    }
}

fn pipeline_parts(
    device: &wgpu::Device,
    backend: wgpu::Backend,
    texture_layout: &wgpu::BindGroupLayout,
) -> ScenePipelineParts {
    let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Scene Surface Uniform Bind Group Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Scene Shadow Map Bind Group Layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
        ],
    });
    let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("Scene Shadow Sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..wgpu::SamplerDescriptor::default()
    });
    let shader = SharedShader::new(
        device,
        backend,
        "Scene Surface Shader",
        shaders::MESH_SURFACE,
        &[
            Some(texture_layout),
            Some(&uniform_layout),
            Some(&shadow_layout),
        ],
    );
    let no_shadow_view = depth_texture(device, "Scene No Shadow", 1);
    let no_shadow = shadow_group(device, (&shadow_layout, &shadow_sampler), &no_shadow_view);
    ScenePipelineParts {
        shader,
        uniform_layout,
        shadow_layout,
        shadow_sampler,
        no_shadow,
    }
}

fn depth_texture(device: &wgpu::Device, label: &'static str, side: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&wgpu::TextureViewDescriptor::default())
}

fn shadow_group(
    device: &wgpu::Device,
    (layout, sampler): (&wgpu::BindGroupLayout, &wgpu::Sampler),
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Scene Shadow Map Bind Group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_shadow_map(device: &wgpu::Device, parts: &ScenePipelineParts) -> ShadowMap {
    let view = depth_texture(device, "Scene Shadow Map", SHADOW_MAP_SIZE);
    let group = shadow_group(device, (&parts.shadow_layout, &parts.shadow_sampler), &view);
    ShadowMap { view, group }
}

fn upload_mesh(device: &wgpu::Device, mesh: &Mesh) -> SceneMeshBuffers {
    let vertices: Vec<SceneVertex> = mesh
        .positions()
        .iter()
        .zip(mesh.normals())
        .zip(mesh.uvs())
        .map(|((&position, &normal), &uv)| SceneVertex {
            position,
            normal,
            uv,
        })
        .collect();
    SceneMeshBuffers {
        vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scene Mesh Vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Scene Mesh Indices"),
            contents: bytemuck::cast_slice(mesh.indices()),
            usage: wgpu::BufferUsages::INDEX,
        }),
        index_count: mesh.indices().len() as u32,
    }
}

pub(crate) fn scene_surface_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Scene Surface Uniform Buffer",
        "Scene Surface Uniform Bind Group",
        std::mem::size_of::<SceneSurfaceUniforms>() as u64,
    )
}

/// The uniforms of one surface of `scene` drawn on `page`, whose eye is at
/// `eye`; `None` when its transform flattens space.
pub(crate) fn surface_uniforms(
    scene: &SceneDraw,
    surface: &SurfaceShading,
    (page, eye): (ScenePage, Vec3),
) -> Option<SceneSurfaceUniforms> {
    let inverse = surface.transform.inverse()?.rows();
    let normal_row = |row: usize| [inverse[0][row], inverse[1][row], inverse[2][row], 0.0];
    let light = match scene.light {
        Some(light) => {
            let direction = light.direction.normalized().unwrap_or(Vec3::ZERO);
            [
                direction.x,
                direction.y,
                direction.z,
                light.ambient.clamp(0.0, 1.0),
            ]
        }
        None => [0.0, 0.0, 0.0, -1.0],
    };
    let columns = |matrix: Mat4| matrix.transpose().rows();
    Some(SceneSurfaceUniforms {
        clip_from_model: columns(clip_from_model(scene, &surface.transform, page)),
        scene_from_model: columns(surface.transform),
        light_clip_from_model: columns(
            surface
                .light_projection
                .map_or(Mat4::IDENTITY, |projection| {
                    surface.transform.then(&projection)
                }),
        ),
        normal_rows: [normal_row(0), normal_row(1), normal_row(2)],
        eye: [eye.x, eye.y, eye.z, 1.0],
        light,
        source_region: surface.source_region,
        params: [
            surface.alpha.clamp(0.0, 1.0),
            f32::from(u8::from(surface.lit)),
            0.0,
            0.0,
        ],
        shadow: [1.0 / SHADOW_MAP_SIZE as f32, SHADOW_DEPTH_BIAS, 0.0, 0.0],
    })
}

/// The map from a surface's mesh to the clip space of `page`, through the
/// camera of `scene`: what lies nearer than the camera's near plane falls
/// outside it, and depth grows with distance.
fn clip_from_model(scene: &SceneDraw, transform: &Mat4, page: ScenePage) -> Mat4 {
    let camera = scene.camera;
    let (scale, focal_length) = (page.scale, camera.focal_length);
    let (width, height) = (page.size.0 as f32, page.size.1 as f32);
    let center_x = scale * (scene.rect.x + scene.rect.width * 0.5) - page.offset[0];
    let center_y = scale * (scene.rect.y + scene.rect.height * 0.5) - page.offset[1];
    let projection = Mat4::from_rows([
        [
            2.0 * scale * focal_length / width,
            0.0,
            2.0 * center_x / width - 1.0,
            0.0,
        ],
        [
            0.0,
            -2.0 * scale * focal_length / height,
            1.0 - 2.0 * center_y / height,
            0.0,
        ],
        [0.0, 0.0, 1.0, -camera.near()],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    transform.then(&camera.view).then(&projection)
}

/// How far from the eye a surface placed by `transform` is, along the
/// camera's view, for drawing the scene's surfaces far to near.
pub(crate) fn view_depth(scene: &SceneDraw, transform: &Mat4) -> f32 {
    transform
        .then(&scene.camera.view)
        .transform_point(Vec3::ZERO)
        .z
}

/// The pixels of `page` the scene and the clip above a surface leave, as a
/// scissor; `None` when they leave none.
pub(crate) fn scene_scissor(
    scene: &SceneDraw,
    clip: Option<Rect>,
    page: ScenePage,
) -> Option<(u32, u32, u32, u32)> {
    let mut rect = scene.rect;
    for cut in [scene.clip, clip].into_iter().flatten() {
        rect = rect.intersect(cut)?;
    }
    let left = (rect.x * page.scale - page.offset[0]).floor().max(0.0);
    let top = (rect.y * page.scale - page.offset[1]).floor().max(0.0);
    let right = ((rect.x + rect.width) * page.scale - page.offset[0])
        .ceil()
        .min(page.size.0 as f32);
    let bottom = ((rect.y + rect.height) * page.scale - page.offset[1])
        .ceil()
        .min(page.size.1 as f32);
    (right > left && bottom > top).then_some((
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

/// The build of one scene pipeline for targets of `format`.
pub(crate) fn scene_pipeline_job(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    (format, kind): (wgpu::TextureFormat, ScenePipeline),
    parts: &ScenePipelineParts,
) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
    let device = Arc::clone(device);
    let shader = parts.shader.clone();
    move || {
        let module = shader.module();
        let (vertex, fragment, label) = match kind {
            ScenePipeline::Plain => (
                "mesh_surface_vs",
                "mesh_surface_fs",
                "Scene Surface Pipeline",
            ),
            ScenePipeline::Shadowed => (
                "mesh_surface_vs",
                "mesh_surface_shadowed_fs",
                "Scene Shadowed Surface Pipeline",
            ),
            ScenePipeline::Shadow => ("mesh_shadow_vs", "mesh_shadow_fs", "Scene Shadow Pipeline"),
        };
        let color_target = [Some(wgpu::ColorTargetState {
            format,
            blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let targets: &[Option<wgpu::ColorTargetState>] = match kind {
            ScenePipeline::Shadow => &[],
            ScenePipeline::Plain | ScenePipeline::Shadowed => &color_target,
        };
        let bias = match kind {
            ScenePipeline::Shadow => wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            },
            ScenePipeline::Plain | ScenePipeline::Shadowed => wgpu::DepthBiasState::default(),
        };
        create_render_pipeline_logged(
            &device,
            cache.as_ref(),
            label,
            wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(shader.layout()),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<SceneVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x3,
                            1 => Float32x3,
                            2 => Float32x2
                        ],
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets,
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
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias,
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        )
    }
}
