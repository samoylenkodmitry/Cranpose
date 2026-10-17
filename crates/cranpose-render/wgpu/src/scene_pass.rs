//! The 3D pass of a scene: each of its mesh surfaces drawn from its rendered
//! content, with a depth buffer so nearer surfaces hide farther ones.

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
    idle_pool::IdlePool,
    pipeline_compiler::CompilerSend,
    render::{DEPTH_FORMAT, create_render_pipeline_logged},
    shaders,
    shared_shader::SharedShader,
};

/// The meshes whose buffers stay on the GPU between frames.
const KEPT_MESHES: usize = 64;

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
    normal_rows: [[f32; 4]; 3],
    eye: [f32; 4],
    light: [f32; 4],
    source_region: [f32; 4],
    params: [f32; 4],
}

/// One mesh's vertices and indices on the GPU.
#[derive(Clone)]
pub(crate) struct SceneMeshBuffers {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
}

/// What the scene pipeline is built from, made when a scene first draws.
pub(crate) struct ScenePipelineParts {
    shader: SharedShader,
    pub(crate) uniform_layout: wgpu::BindGroupLayout,
}

/// The scene pipeline and the buffers of the meshes recent frames drew.
pub(crate) struct ScenePass {
    parts: OnceCell<ScenePipelineParts>,
    pub(crate) pipeline: FixedPipeline,
    meshes: IdlePool<(u64, SceneMeshBuffers)>,
}

impl Default for ScenePass {
    fn default() -> Self {
        Self {
            parts: OnceCell::new(),
            pipeline: FixedPipeline::new("scene/mesh-surface"),
            meshes: IdlePool::default(),
        }
    }
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
/// not, at `alpha`, from the content's texels in its texture.
pub(crate) struct SurfaceShading {
    pub(crate) transform: Mat4,
    pub(crate) lit: bool,
    pub(crate) alpha: f32,
    /// The content's top left texel and its size in texels.
    pub(crate) source_region: [f32; 4],
}

impl ScenePass {
    /// The shader and uniform layout, made on the first call;
    /// `texture_layout` binds the surface's texture.
    pub(crate) fn parts(
        &self,
        device: &wgpu::Device,
        backend: wgpu::Backend,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> &ScenePipelineParts {
        self.parts.get_or_init(|| {
            let uniform_layout =
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
            let shader = SharedShader::new(
                device,
                backend,
                "Scene Surface Shader",
                shaders::MESH_SURFACE,
                &[Some(texture_layout), Some(&uniform_layout)],
            );
            ScenePipelineParts {
                shader,
                uniform_layout,
            }
        })
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

    /// Drops the buffers of meshes no recent frame drew.
    pub(crate) fn end_frame(&mut self) {
        self.meshes.end_frame();
    }
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

/// The uniforms of one surface of `scene` drawn on `page`; `None` when its
/// transform flattens space.
pub(crate) fn surface_uniforms(
    scene: &SceneDraw,
    surface: &SurfaceShading,
    page: ScenePage,
) -> Option<SceneSurfaceUniforms> {
    let inverse = surface.transform.inverse()?.rows();
    let normal_row = |row: usize| [inverse[0][row], inverse[1][row], inverse[2][row], 0.0];
    let light = match (surface.lit, scene.light) {
        (true, Some(light)) => {
            let direction = light.direction.normalized().unwrap_or(Vec3::ZERO);
            [
                direction.x,
                direction.y,
                direction.z,
                light.ambient.clamp(0.0, 1.0),
            ]
        }
        _ => [0.0, 0.0, 0.0, -1.0],
    };
    Some(SceneSurfaceUniforms {
        clip_from_model: clip_from_model(scene, &surface.transform, page)
            .transpose()
            .rows(),
        scene_from_model: surface.transform.transpose().rows(),
        normal_rows: [normal_row(0), normal_row(1), normal_row(2)],
        eye: {
            let eye = scene.camera.eye();
            [eye.x, eye.y, eye.z, 1.0]
        },
        light,
        source_region: surface.source_region,
        params: [surface.alpha.clamp(0.0, 1.0), 0.0, 0.0, 0.0],
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

/// The build of the scene pipeline for targets of `format`.
pub(crate) fn scene_pipeline_job(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    format: wgpu::TextureFormat,
    parts: &ScenePipelineParts,
) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
    let device = Arc::clone(device);
    let shader = parts.shader.clone();
    move || {
        let module = shader.module();
        create_render_pipeline_logged(
            &device,
            cache.as_ref(),
            "scene mesh-surface",
            wgpu::RenderPipelineDescriptor {
                label: Some("Scene Surface Pipeline"),
                layout: Some(shader.layout()),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("mesh_surface_vs"),
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
                    entry_point: Some("mesh_surface_fs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            },
        )
    }
}
