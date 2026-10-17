//! The 3D pass of a scene: each of its mesh surfaces drawn from its rendered
//! content, with a depth buffer so nearer surfaces hide farther ones, and,
//! for a light that casts shadows, a shadow map its casters are drawn into
//! first. Volumes draw after the other surfaces, each with its own shader.
//! Glass draws after them from a copy of what they left, and glow after it,
//! added onto the page.

use std::{
    cell::{Cell, OnceCell},
    sync::{Arc, Weak},
};

use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::{
    Rect,
    space::{Mat4, Material, Mesh, Vec3, Volume},
};

use crate::{
    collect::SceneDraw,
    fixed_pipeline::FixedPipeline,
    frame_graph::{FrameCommandRecorder, FrameCommandStats, UploadAllocatorSpec},
    idle_pool::{IDLE_FRAMES, IdlePool},
    lazy_resource::LazyGpuResource,
    pipeline_compiler::{CompileLane, CompilerSend, PipelineCompiler},
    pipeline_recorder::PipelineRecorder,
    render::{DEPTH_FORMAT, create_render_pipeline_logged},
    shader_cache::create_volume_shader_module,
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
    clip_from_scene: [[f32; 4]; 4],
    normal_rows: [[f32; 4]; 3],
    eye: [f32; 4],
    light: [f32; 4],
    source_region: [f32; 4],
    params: [f32; 4],
    shadow: [f32; 4],
    shadow_soft: [f32; 4],
    glass: [f32; 4],
    glass_tint: [f32; 4],
    backdrop_region: [f32; 4],
}

/// The block `mesh_volume.wgsl` reads for one volume.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SceneVolumeUniforms {
    clip_from_model: [[f32; 4]; 4],
    eye: [f32; 4],
    light: [f32; 4],
    footprint: [f32; 4],
    low: [f32; 4],
    high: [f32; 4],
    params: [[f32; 4]; Volume::PARAMS / 4],
}

/// One mesh's vertices and indices on the GPU.
#[derive(Clone)]
pub(crate) struct SceneMeshBuffers {
    pub(crate) vertices: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) index_count: u32,
}

/// The buffers of a mesh a recent frame drew, and the mesh while anyone
/// still holds it. A mesh nobody holds is never drawn again, so its buffers
/// take the next mesh that fits them: a shape that changes every frame
/// keeps writing into the same buffers.
struct KeptMesh {
    id: u64,
    mesh: Weak<Mesh>,
    buffers: SceneMeshBuffers,
}

/// What the scene pipelines are built and bound from, made when a scene
/// first draws.
pub(crate) struct ScenePipelineParts {
    shader: SharedShader,
    pub(crate) uniform_layout: wgpu::BindGroupLayout,
    shadow_layout: wgpu::BindGroupLayout,
    shadow_samplers: ShadowSamplers,
    /// Bound in the shadow map's place for a scene that casts no shadow.
    pub(crate) no_shadow: wgpu::BindGroup,
    /// What a volume's pipeline binds: its texture and its uniforms.
    volume_layout: wgpu::PipelineLayout,
}

/// The pipelines of one volume shader, kept while anyone holds the shader:
/// one draws its volumes, the other only their depth, for the pass glass
/// draws in.
pub(crate) struct VolumeShader {
    source: Weak<str>,
    module: LazyGpuResource<Option<wgpu::ShaderModule>>,
    pipelines: [VolumePipeline; 2],
}

impl VolumeShader {
    /// The pipeline that draws its volumes, or with `depth_only` only their
    /// depth.
    pub(crate) fn pipeline(&self, depth_only: bool) -> &VolumePipeline {
        &self.pipelines[usize::from(depth_only)]
    }
}

/// One of a volume shader's pipelines; `None` once built when the shader
/// does not build.
pub(crate) struct VolumePipeline {
    resource: LazyGpuResource<Option<wgpu::RenderPipeline>>,
    /// Whether its build is queued, so it is queued once.
    queued: Cell<bool>,
}

impl VolumePipeline {
    fn new(label: &'static str) -> Self {
        Self {
            resource: LazyGpuResource::new(label),
            queued: Cell::new(false),
        }
    }

    /// The pipeline once built, or `None` while it compiles: a missing one
    /// is queued on the demand lane, and built here when nothing else can
    /// build it.
    pub(crate) fn when_ready<J>(
        &self,
        (recorder, compiler, backend): (&PipelineRecorder, &PipelineCompiler, wgpu::Backend),
        job: impl FnOnce() -> J,
    ) -> Option<&Option<wgpu::RenderPipeline>>
    where
        J: FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static,
    {
        if let Some(built) = self.resource.get() {
            return Some(built);
        }
        if !compiler.is_active() {
            return Some(self.resource.for_draw(recorder, backend, job()));
        }
        if !self.queued.replace(true) {
            self.resource
                .queue(compiler, CompileLane::Demanded, backend, job());
        }
        None
    }
}

/// The two ways a surface reads the shadow map: compared with its own depth
/// and filtered, and as the depth of the nearest caster, which the search
/// for a soft shadow's casters reads. The map is bound once for each, as
/// GLSL samples a texture with one sampler.
struct ShadowSamplers {
    compare: wgpu::Sampler,
    depth: wgpu::Sampler,
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
    pub(crate) glass_pipeline: FixedPipeline,
    pub(crate) glow_pipeline: FixedPipeline,
    pub(crate) depth_pipeline: FixedPipeline,
    meshes: IdlePool<KeptMesh>,
    volumes: Vec<VolumeShader>,
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
            glass_pipeline: FixedPipeline::new("scene/mesh-glass"),
            glow_pipeline: FixedPipeline::new("scene/mesh-glow"),
            depth_pipeline: FixedPipeline::new("scene/mesh-depth"),
            meshes: IdlePool::default(),
            volumes: Vec::new(),
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
    /// Glass, which reads the page behind it.
    Glass,
    /// Glow, added onto the page without hiding what lies behind it.
    Glow,
    /// The depth of the surfaces that are not glass, again, in the pass that
    /// draws glass after the copy it reads.
    Depth,
}

impl ScenePipeline {
    /// Every scene pipeline.
    pub(crate) const ALL: [Self; 6] = [
        Self::Plain,
        Self::Shadowed,
        Self::Shadow,
        Self::Glass,
        Self::Glow,
        Self::Depth,
    ];
}

/// Where a scene's 3D pass draws: a page's pixels per logical pixel, the
/// device offset its origin sits at, and its size in pixels.
#[derive(Clone, Copy)]
pub(crate) struct ScenePage {
    pub(crate) scale: f32,
    pub(crate) offset: [f32; 2],
    pub(crate) size: (u32, u32),
}

/// How one surface is drawn: placed by `transform` in the scene, of
/// `material`, at `alpha`, from the content's texels in its texture, and
/// where it lies in the scene's shadow map.
pub(crate) struct SurfaceShading<'m> {
    pub(crate) transform: Mat4,
    pub(crate) material: &'m Material,
    pub(crate) alpha: f32,
    /// The content's top left texel and its size in texels.
    pub(crate) source_region: [f32; 4],
    /// Maps the scene into its shadow map, for a scene that casts shadows.
    pub(crate) light_projection: Option<Mat4>,
    /// The page pixels the copy glass reads starts at, and its size.
    pub(crate) backdrop_region: [f32; 4],
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

    /// The buffers of `mesh`, staged through `recorder` the first time a
    /// frame draws it, into the buffers of a mesh nobody holds any more
    /// where they fit; and what the staging wrote.
    pub(crate) fn mesh_buffers(
        &mut self,
        (device, recorder): (&wgpu::Device, &mut impl FrameCommandRecorder),
        mesh: &Arc<Mesh>,
    ) -> (SceneMeshBuffers, FrameCommandStats) {
        let id = mesh.id();
        let mut stats = FrameCommandStats::default();
        let buffers = match self.meshes.take(|kept| kept.id == id) {
            Some(kept) => kept.buffers,
            None => {
                let vertices = scene_vertices(mesh);
                let vertices: &[u8] = bytemuck::cast_slice(&vertices);
                let indices: &[u8] = bytemuck::cast_slice(mesh.indices());
                let freed = self.meshes.take(|kept| {
                    kept.mesh.strong_count() == 0
                        && kept.buffers.vertices.size() >= vertices.len() as u64
                        && kept.buffers.indices.size() >= indices.len() as u64
                });
                let (vertex_buffer, index_buffer) = match freed {
                    Some(kept) => (kept.buffers.vertices, kept.buffers.indices),
                    None => (
                        mesh_buffer(
                            device,
                            "Scene Mesh Vertices",
                            wgpu::BufferUsages::VERTEX,
                            vertices,
                        ),
                        mesh_buffer(
                            device,
                            "Scene Mesh Indices",
                            wgpu::BufferUsages::INDEX,
                            indices,
                        ),
                    ),
                };
                stats += recorder.stage_buffer_copy(device, &vertex_buffer, 0, vertices);
                stats += recorder.stage_buffer_copy(device, &index_buffer, 0, indices);
                SceneMeshBuffers {
                    vertices: vertex_buffer,
                    indices: index_buffer,
                    index_count: mesh.indices().len() as u32,
                }
            }
        };
        self.meshes.put(
            KeptMesh {
                id,
                mesh: Arc::downgrade(mesh),
                buffers: buffers.clone(),
            },
            KEPT_MESHES,
            u64::MAX,
            |_| 0,
        );
        (buffers, stats)
    }

    /// Keeps the pipelines of `shader`, whose volumes a frame draws.
    pub(crate) fn note_volume(&mut self, shader: &Arc<str>) {
        if self.volume(shader).is_none() {
            self.volumes.push(VolumeShader {
                source: Arc::downgrade(shader),
                module: LazyGpuResource::new("scene/volume-module"),
                pipelines: [
                    VolumePipeline::new("scene/volume"),
                    VolumePipeline::new("scene/volume-depth"),
                ],
            });
        }
    }

    /// The pipelines of `shader`, once a frame noted it.
    pub(crate) fn volume(&self, shader: &Arc<str>) -> Option<&VolumeShader> {
        // A weak handle keeps the shader's memory, so no other shader takes
        // its address while the pipelines are kept.
        self.volumes
            .iter()
            .find(|kept| std::ptr::addr_eq(kept.source.as_ptr(), Arc::as_ptr(shader)))
    }

    /// Drops the buffers of meshes no recent frame drew, the pipelines of
    /// volume shaders nobody holds, and the shadow map once no recent frame
    /// cast shadows.
    pub(crate) fn end_frame(&mut self) {
        self.meshes.end_frame();
        self.volumes.retain(|kept| kept.source.strong_count() > 0);
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
            shadow_map_entry(0),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            shadow_map_entry(2),
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
        ],
    });
    let shadow_samplers = ShadowSamplers {
        compare: device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Scene Shadow Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..wgpu::SamplerDescriptor::default()
        }),
        depth: device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Scene Shadow Depth Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..wgpu::SamplerDescriptor::default()
        }),
    };
    let shader = SharedShader::new(
        device,
        backend,
        "Scene Surface Shader",
        shaders::MESH_SURFACE,
        &[
            Some(texture_layout),
            Some(&uniform_layout),
            Some(&shadow_layout),
            Some(texture_layout),
        ],
    );
    let no_shadow_view = depth_texture(device, "Scene No Shadow", 1);
    let no_shadow = shadow_group(device, (&shadow_layout, &shadow_samplers), &no_shadow_view);
    let volume_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Scene Volume Pipeline Layout"),
        bind_group_layouts: &[Some(texture_layout), Some(&uniform_layout)],
        immediate_size: 0,
    });
    ScenePipelineParts {
        shader,
        uniform_layout,
        shadow_layout,
        shadow_samplers,
        no_shadow,
        volume_layout,
    }
}

/// A shadow map binding of the scene's shadow bind group.
fn shadow_map_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Depth,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
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
    (layout, samplers): (&wgpu::BindGroupLayout, &ShadowSamplers),
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
                resource: wgpu::BindingResource::Sampler(&samplers.compare),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&samplers.depth),
            },
        ],
    })
}

fn create_shadow_map(device: &wgpu::Device, parts: &ScenePipelineParts) -> ShadowMap {
    let view = depth_texture(device, "Scene Shadow Map", SHADOW_MAP_SIZE);
    let group = shadow_group(
        device,
        (&parts.shadow_layout, &parts.shadow_samplers),
        &view,
    );
    ShadowMap { view, group }
}

/// `mesh`'s vertices as the pipelines read them.
fn scene_vertices(mesh: &Mesh) -> Vec<SceneVertex> {
    mesh.positions()
        .iter()
        .zip(mesh.normals())
        .zip(mesh.uvs())
        .map(|((&position, &normal), &uv)| SceneVertex {
            position,
            normal,
            uv,
        })
        .collect()
}

/// A buffer of `usage` with room for `bytes` and, rounded up to a power of
/// two, for the next shape of a mesh that changes.
fn mesh_buffer(
    device: &wgpu::Device,
    label: &'static str,
    usage: wgpu::BufferUsages,
    bytes: &[u8],
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (bytes.len() as u64).next_power_of_two().max(256),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn scene_surface_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Scene Surface Uniform Buffer",
        "Scene Surface Uniform Bind Group",
        std::mem::size_of::<SceneSurfaceUniforms>() as u64,
    )
}

pub(crate) fn scene_volume_uniform_spec() -> UploadAllocatorSpec {
    UploadAllocatorSpec::uniform(
        "Scene Volume Uniform Buffer",
        "Scene Volume Uniform Bind Group",
        std::mem::size_of::<SceneVolumeUniforms>() as u64,
    )
}

/// The scene's light for a shader: the direction it travels, of length one,
/// through `into`, and its ambient share; an ambient share below zero for a
/// scene without light.
fn light_uniform(scene: &SceneDraw, into: impl Fn(Vec3) -> Vec3) -> [f32; 4] {
    match scene.light {
        Some(light) => {
            let direction = into(light.direction).normalized().unwrap_or(Vec3::ZERO);
            [
                direction.x,
                direction.y,
                direction.z,
                light.ambient.clamp(0.0, 1.0),
            ]
        }
        None => [0.0, 0.0, 0.0, -1.0],
    }
}

/// The uniforms of `volume` on `mesh`, placed by `transform` in `scene` and
/// drawn on `page` at `alpha`, whose eye is at `eye`; `None` when its
/// transform flattens space.
pub(crate) fn volume_uniforms(
    scene: &SceneDraw,
    (transform, mesh, volume, alpha): (&Mat4, &Mesh, &Volume, f32),
    (page, eye): (ScenePage, Vec3),
) -> Option<SceneVolumeUniforms> {
    let model_from_scene = transform.inverse()?;
    let point = |point: Vec3| [point.x, point.y, point.z, 1.0];
    let (low, high) = mesh.bounds();
    Some(SceneVolumeUniforms {
        clip_from_model: transform
            .then(&clip_from_scene(scene, page))
            .transpose()
            .rows(),
        eye: point(model_from_scene.transform_point(eye)),
        light: light_uniform(scene, |direction| {
            model_from_scene.transform_vector(direction)
        }),
        footprint: [
            1.0 / (scene.camera.focal_length * page.scale).max(f32::MIN_POSITIVE),
            alpha.clamp(0.0, 1.0),
            0.0,
            0.0,
        ],
        low: point(low),
        high: point(high),
        params: bytemuck::cast(volume.params),
    })
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
    let light = light_uniform(scene, |direction| direction);
    let columns = |matrix: Mat4| matrix.transpose().rows();
    let clip_from_scene = clip_from_scene(scene, page);
    let (mode, glass) = match surface.material {
        Material::Unlit => (0.0, None),
        Material::Lit => (1.0, None),
        Material::Emissive | Material::Glow => (2.0, None),
        Material::Glass(glass) => (0.0, Some(*glass)),
        // A volume takes its own uniforms.
        Material::Volume(_) => (0.0, None),
    };
    let glass_tint = glass.map_or([1.0; 4], |glass| {
        [glass.tint.0, glass.tint.1, glass.tint.2, 1.0]
    });
    Some(SceneSurfaceUniforms {
        clip_from_model: columns(surface.transform.then(&clip_from_scene)),
        scene_from_model: columns(surface.transform),
        light_clip_from_model: columns(
            surface
                .light_projection
                .map_or(Mat4::IDENTITY, |projection| {
                    surface.transform.then(&projection)
                }),
        ),
        clip_from_scene: columns(clip_from_scene),
        normal_rows: [normal_row(0), normal_row(1), normal_row(2)],
        eye: [eye.x, eye.y, eye.z, 1.0],
        light,
        source_region: surface.source_region,
        params: [surface.alpha.clamp(0.0, 1.0), mode, 0.0, 0.0],
        shadow: [1.0 / SHADOW_MAP_SIZE as f32, SHADOW_DEPTH_BIAS, 0.0, 0.0],
        shadow_soft: penumbra_scale(scene, surface.light_projection),
        glass: [
            glass.map_or(1.0, |glass| 1.0 / glass.index.max(1.0)),
            glass.map_or(0.0, |glass| glass.thickness.max(0.0)),
            page.size.0 as f32,
            page.size.1 as f32,
        ],
        glass_tint,
        backdrop_region: surface.backdrop_region,
    })
}

/// How wide a penumbra of the scene's light opens, in shadow map
/// coordinates along each axis, a unit of shadow map depth behind its
/// caster: the light's spread over the map's depth range, measured in the
/// map's width and height. Zero for a light of no size.
fn penumbra_scale(scene: &SceneDraw, light_projection: Option<Mat4>) -> [f32; 4] {
    let (Some(light), Some(projection)) = (scene.light, light_projection) else {
        return [0.0; 4];
    };
    if light.size <= 0.0 {
        return [0.0; 4];
    }
    // The projection's rows scale x and y by 2 over the map's width and
    // height, and depth by 1 over its depth range.
    let rows = projection.rows();
    let length = |row: [f32; 4]| Vec3::new(row[0], row[1], row[2]).length();
    let depth = length(rows[2]).max(f32::MIN_POSITIVE);
    let spread = light.size.tan();
    [
        spread * length(rows[0]) * 0.5 / depth,
        spread * length(rows[1]) * 0.5 / depth,
        1.0,
        0.0,
    ]
}

/// The map from `scene` to the clip space of `page`, through the scene's
/// camera: what lies nearer than the camera's near plane falls outside it,
/// and depth grows with distance.
fn clip_from_scene(scene: &SceneDraw, page: ScenePage) -> Mat4 {
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
    camera.view.then(&projection)
}

/// The pixels of `page` a mesh placed by `transform` in `scene` covers,
/// from the corners of its bounds: left, top, right and bottom. `None` when
/// a corner lies behind the eye.
pub(crate) fn surface_page_bounds(
    scene: &SceneDraw,
    transform: &Mat4,
    mesh: &Mesh,
    page: ScenePage,
) -> Option<[f32; 4]> {
    let rows = transform.then(&clip_from_scene(scene, page)).rows();
    let (low, high) = mesh.bounds();
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for corner in 0..8 {
        let pick = |bit: usize, low: f32, high: f32| if corner & bit == 0 { low } else { high };
        let point = [
            pick(1, low.x, high.x),
            pick(2, low.y, high.y),
            pick(4, low.z, high.z),
            1.0,
        ];
        let [x, y, _, w] = rows.map(|row| row.iter().zip(point).map(|(a, b)| a * b).sum::<f32>());
        if w <= 0.0 {
            return None;
        }
        let pixel = [
            (x / w * 0.5 + 0.5) * page.size.0 as f32,
            (0.5 - y / w * 0.5) * page.size.1 as f32,
        ];
        bounds = [
            bounds[0].min(pixel[0]),
            bounds[1].min(pixel[1]),
            bounds[2].max(pixel[0]),
            bounds[3].max(pixel[1]),
        ];
    }
    Some(bounds)
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
            ScenePipeline::Glass => ("mesh_surface_vs", "mesh_glass_fs", "Scene Glass Pipeline"),
            ScenePipeline::Glow => ("mesh_surface_vs", "mesh_surface_fs", "Scene Glow Pipeline"),
            ScenePipeline::Depth => ("mesh_surface_vs", "mesh_depth_fs", "Scene Depth Pipeline"),
        };
        let blend = match kind {
            // Premultiplied colour added onto the page, which keeps its alpha.
            ScenePipeline::Glow => wgpu::BlendState {
                color: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha: wgpu::BlendComponent {
                    src_factor: wgpu::BlendFactor::Zero,
                    dst_factor: wgpu::BlendFactor::One,
                    operation: wgpu::BlendOperation::Add,
                },
            },
            _ => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        };
        let color_target = color_target(format, blend, kind != ScenePipeline::Depth);
        let (targets, bias): (&[Option<wgpu::ColorTargetState>], _) = match kind {
            ScenePipeline::Shadow => (
                &[],
                wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            ),
            _ => (&color_target, wgpu::DepthBiasState::default()),
        };
        create_scene_pipeline(
            &device,
            cache.as_ref(),
            label,
            (shader.layout(), module, (vertex, fragment)),
            (targets, kind != ScenePipeline::Glow, bias),
        )
    }
}

/// The build of one volume shader's pipeline for targets of `format`,
/// which with `depth_only` draws only the volumes' depth; `None` when the
/// shader does not build or nobody holds it any more.
pub(crate) fn volume_pipeline_job(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    (format, backend, depth_only): (wgpu::TextureFormat, wgpu::Backend, bool),
    (parts, kept): (&ScenePipelineParts, &VolumeShader),
) -> impl FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static {
    let device = Arc::clone(device);
    let layout = parts.volume_layout.clone();
    let module = kept.module.clone();
    let source = kept.source.upgrade();
    move || {
        let source = source?;
        let module = module
            .get_or_init(backend, || {
                create_volume_shader_module(&device, &source, backend)
            })
            .as_ref()?;
        let label = if depth_only {
            "Scene Volume Depth Pipeline"
        } else {
            "Scene Volume Pipeline"
        };
        Some(create_scene_pipeline(
            &device,
            cache.as_ref(),
            label,
            (&layout, module, ("mesh_volume_vs", "mesh_volume_fs")),
            (
                &color_target(
                    format,
                    wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
                    !depth_only,
                ),
                true,
                wgpu::DepthBiasState::default(),
            ),
        ))
    }
}

/// The colour target of a scene pipeline, which writes colour unless
/// `writes` is false.
fn color_target(
    format: wgpu::TextureFormat,
    blend: wgpu::BlendState,
    writes: bool,
) -> [Option<wgpu::ColorTargetState>; 1] {
    [Some(wgpu::ColorTargetState {
        format,
        blend: Some(blend),
        write_mask: if writes {
            wgpu::ColorWrites::ALL
        } else {
            wgpu::ColorWrites::empty()
        },
    })]
}

/// A pipeline that draws the scene's meshes, depth tested, from `module`'s
/// `entry_points` onto `targets`.
fn create_scene_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    label: &'static str,
    (layout, module, (vertex, fragment)): (
        &wgpu::PipelineLayout,
        &wgpu::ShaderModule,
        (&'static str, &'static str),
    ),
    (targets, depth_write, bias): (
        &[Option<wgpu::ColorTargetState>],
        bool,
        wgpu::DepthBiasState,
    ),
) -> wgpu::RenderPipeline {
    create_render_pipeline_logged(
        device,
        cache,
        label,
        wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(layout),
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
                depth_write_enabled: Some(depth_write),
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
