//! The 3D pass of a scene: each of its mesh surfaces drawn from its rendered
//! content, with a depth buffer so nearer surfaces hide farther ones, and,
//! for a light that casts shadows, a shadow map its casters are drawn into
//! first. Volumes draw after the other surfaces, each with its own shader.
//! Glass draws after them from a copy of what they left, and glow after it,
//! added onto the page.

use std::{
    cell::OnceCell,
    sync::{Arc, Weak},
};

use bytemuck::{Pod, Zeroable};
use cranpose_ui_graphics::space::{
    Light, Mat4, Material, Mesh, Multisample, Shaded, TextureEdges, Volume,
};

use crate::{
    fixed_pipeline::FixedPipeline,
    frame_graph::{FrameCommandRecorder, FrameCommandStats},
    idle_pool::{IDLE_FRAMES, IdlePool},
    pipeline_compiler::CompilerSend,
    render::{DEPTH_FORMAT, create_render_pipeline_logged},
    shader_cache::{create_shaded_shader_module, create_volume_shader_module},
    shaders,
    shared_shader::SharedShader,
};

mod app_shader;
mod shadow_map;
mod uniforms;
mod variants;
mod white_texel;

use app_shader::kept_shader;
pub(crate) use app_shader::{AppShader, ShadedShader, ShaderPipeline, VolumeShader};
pub(crate) use shadow_map::ShadowMap;
use shadow_map::{
    ShadowSamplers, create_shadow_map, depth_texture, shadow_group, shadow_map_entry,
};
pub(crate) use uniforms::{
    scene_scissor, scene_shaded_uniform_spec, scene_surface_uniform_spec,
    scene_volume_uniform_spec, shaded_uniforms, surface_page_bounds, surface_uniforms, view_depth,
    volume_uniforms,
};
pub(crate) use variants::{ShadedVariant, VolumeVariant};
use white_texel::WhiteTexel;

// The sample counts a scene's pass draws with, each with pipelines of its
// own.
pub(crate) const SAMPLES: [Multisample; 2] = [Multisample::Off, Multisample::X4];

// The place of `samples` in `SAMPLES`.
fn sample_slot(samples: Multisample) -> usize {
    match samples {
        Multisample::Off => 0,
        Multisample::X4 => 1,
    }
}

/// The meshes whose buffers stay on the GPU between frames.
const KEPT_MESHES: usize = 64;
/// The depth and stencil a scene's pass draws against: the stencil counts
/// how many portal windows deep each pixel shows.
pub const SCENE_DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
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
    pub(crate) clip_from_model: [[f32; 4]; 4],
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
    clip: [f32; 4],
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
    clip: [f32; 4],
    source_region: [f32; 4],
}

/// The block `mesh_shaded.wgsl` reads for one shaded surface, beside the
/// surface's own.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct SceneShadedUniforms {
    eye: [f32; 4],
    light: [f32; 4],
    params: [[f32; 4]; Shaded::PARAMS / 4],
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
    /// What a shaded surface's pipeline binds: a surface's groups, and its
    /// shader's values in the place of the backdrop glass reads.
    shaded_layout: wgpu::PipelineLayout,
}

/// The scene pipelines, the buffers of the meshes recent frames drew and
/// the shadow map.
pub(crate) struct ScenePass {
    parts: OnceCell<ScenePipelineParts>,
    // The scene pipelines of each of `SAMPLES`, in the order of
    // `ScenePipeline::ALL`.
    pipelines: [[FixedPipeline; ScenePipeline::ALL.len()]; SAMPLES.len()],
    /// The unit square portals' windows are drawn on, made when a scene
    /// first shows a portal.
    window: OnceCell<Arc<Mesh>>,
    meshes: IdlePool<KeptMesh>,
    volumes: Vec<VolumeShader>,
    shaded: Vec<ShadedShader>,
    pub(crate) shadow_map: Option<ShadowMap>,
    /// The frames since a scene last drew shadows.
    shadow_map_idle: u64,
    // The content of every shaded mesh whose node draws nothing
    pub(crate) white: WhiteTexel,
}

impl Default for ScenePass {
    fn default() -> Self {
        Self {
            parts: OnceCell::new(),
            pipelines: SAMPLES.map(|samples| {
                ScenePipeline::ALL.map(|kind| FixedPipeline::new(kind.record_label(samples)))
            }),
            window: OnceCell::new(),
            meshes: IdlePool::default(),
            volumes: Vec::new(),
            shaded: Vec::new(),
            shadow_map: None,
            shadow_map_idle: 0,
            white: WhiteTexel::default(),
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
    /// A portal's window, marked in the stencil where it shows.
    WindowMark,
    /// The depth inside a portal's window, cleared to the far plane.
    WindowClear,
    /// A portal's window, its depth written again and its mark taken back.
    WindowRestore,
    /// Surfaces in a portal's space, inside its window and beyond its plane.
    Portal,
    /// Glow in a portal's space, added inside its window beyond its plane.
    PortalGlow,
    /// The depth of the surfaces in a portal's space that are not glass,
    /// again, for the pass that draws glass.
    PortalDepth,
    /// Glass in a portal's space, inside its window beyond its plane.
    PortalGlass,
}

impl ScenePipeline {
    /// Every scene pipeline.
    pub(crate) const ALL: [Self; 13] = [
        Self::Plain,
        Self::Shadowed,
        Self::Shadow,
        Self::Glass,
        Self::Glow,
        Self::Depth,
        Self::WindowMark,
        Self::WindowClear,
        Self::WindowRestore,
        Self::Portal,
        Self::PortalGlow,
        Self::PortalDepth,
        Self::PortalGlass,
    ];

    // The label a launch records the pipeline of `samples` under. The
    // shadow map takes one sample, so no pass asks for its pipeline of four.
    fn record_label(self, samples: Multisample) -> &'static str {
        let labels = match self {
            Self::Plain => ["scene/mesh-surface", "scene/mesh-surface-x4"],
            Self::Shadowed => [
                "scene/mesh-surface-shadowed",
                "scene/mesh-surface-shadowed-x4",
            ],
            Self::Shadow => ["scene/mesh-shadow", "scene/mesh-shadow-x4"],
            Self::Glass => ["scene/mesh-glass", "scene/mesh-glass-x4"],
            Self::Glow => ["scene/mesh-glow", "scene/mesh-glow-x4"],
            Self::Depth => ["scene/mesh-depth", "scene/mesh-depth-x4"],
            Self::WindowMark => ["scene/window-mark", "scene/window-mark-x4"],
            Self::WindowClear => ["scene/window-clear", "scene/window-clear-x4"],
            Self::WindowRestore => ["scene/window-restore", "scene/window-restore-x4"],
            Self::Portal => ["scene/mesh-portal", "scene/mesh-portal-x4"],
            Self::PortalGlow => ["scene/mesh-portal-glow", "scene/mesh-portal-glow-x4"],
            Self::PortalDepth => ["scene/mesh-portal-depth", "scene/mesh-portal-depth-x4"],
            Self::PortalGlass => ["scene/mesh-portal-glass", "scene/mesh-portal-glass-x4"],
        };
        labels[sample_slot(samples)]
    }

    fn spec(self, samples: Multisample) -> PipelineSpec {
        let (label, entry_points) = self.shader();
        let (color, depth, stencil) = self.targets();
        PipelineSpec {
            label,
            entry_points,
            color,
            depth,
            stencil,
            samples,
        }
    }

    /// The pipeline's label and its vertex and fragment entry points.
    fn shader(self) -> (&'static str, (&'static str, &'static str)) {
        match self {
            Self::Plain => (
                "Scene Surface Pipeline",
                ("mesh_surface_vs", "mesh_surface_fs"),
            ),
            Self::Shadowed => (
                "Scene Shadowed Surface Pipeline",
                ("mesh_surface_vs", "mesh_surface_shadowed_fs"),
            ),
            Self::Shadow => (
                "Scene Shadow Pipeline",
                ("mesh_shadow_vs", "mesh_shadow_fs"),
            ),
            Self::Glass => ("Scene Glass Pipeline", ("mesh_surface_vs", "mesh_glass_fs")),
            Self::Glow => (
                "Scene Glow Pipeline",
                ("mesh_surface_vs", "mesh_surface_fs"),
            ),
            Self::Depth => ("Scene Depth Pipeline", ("mesh_surface_vs", "mesh_depth_fs")),
            Self::WindowMark => (
                "Scene Window Mark Pipeline",
                ("mesh_surface_vs", "mesh_depth_fs"),
            ),
            Self::WindowClear => (
                "Scene Window Clear Pipeline",
                ("mesh_surface_vs", "mesh_window_far_fs"),
            ),
            Self::WindowRestore => (
                "Scene Window Restore Pipeline",
                ("mesh_surface_vs", "mesh_depth_fs"),
            ),
            Self::Portal => (
                "Scene Portal Surface Pipeline",
                ("mesh_surface_vs", "mesh_portal_surface_fs"),
            ),
            Self::PortalGlow => (
                "Scene Portal Glow Pipeline",
                ("mesh_surface_vs", "mesh_portal_surface_fs"),
            ),
            Self::PortalDepth => (
                "Scene Portal Depth Pipeline",
                ("mesh_surface_vs", "mesh_portal_depth_fs"),
            ),
            Self::PortalGlass => (
                "Scene Portal Glass Pipeline",
                ("mesh_surface_vs", "mesh_portal_glass_fs"),
            ),
        }
    }

    /// How the pipeline blends colour, if it writes any, and how it uses
    /// depth and the stencil.
    fn targets(self) -> (Option<Blend>, Depth, Stencil) {
        match self {
            Self::Plain | Self::Shadowed | Self::Glass => {
                (Some(Blend::Over), Depth::Tested, Stencil::Ignored)
            }
            Self::Shadow => (None, Depth::Tested, Stencil::Ignored),
            Self::Glow => (Some(Blend::Add), Depth::Read, Stencil::Ignored),
            Self::Depth => (None, Depth::Tested, Stencil::Ignored),
            Self::WindowMark => (None, Depth::Read, Stencil::Mark),
            Self::WindowClear | Self::WindowRestore => (
                None,
                Depth::Overwrite,
                if self == Self::WindowClear {
                    Stencil::Inside
                } else {
                    Stencil::Release
                },
            ),
            Self::Portal | Self::PortalGlass => (Some(Blend::Over), Depth::Tested, Stencil::Inside),
            Self::PortalGlow => (Some(Blend::Add), Depth::Read, Stencil::Inside),
            Self::PortalDepth => (None, Depth::Tested, Stencil::Inside),
        }
    }
}

/// How a scene pipeline draws: its entry points, how it blends colour, if it
// writes any, how it uses depth and the stencil, and the samples it takes.
struct PipelineSpec {
    label: &'static str,
    entry_points: (&'static str, &'static str),
    color: Option<Blend>,
    depth: Depth,
    stencil: Stencil,
    samples: Multisample,
}

#[derive(Clone, Copy, PartialEq)]
enum Blend {
    /// Premultiplied colour over the page.
    Over,
    /// Premultiplied colour added onto the page, which keeps its alpha.
    Add,
}

#[derive(Clone, Copy, PartialEq)]
enum Depth {
    /// Drawn where nearer than what the page holds, and written.
    Tested,
    /// Drawn where no farther than what the page holds, and not written.
    Read,
    /// Drawn everywhere the stencil lets it, and written.
    Overwrite,
}

/// How a scene pipeline uses the stencil, which counts how many windows
/// deep a pixel shows, against the reference a draw sets.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Stencil {
    /// Not at all.
    Ignored,
    /// Where it equals the reference, it counts one deeper.
    Mark,
    /// Drawn only where it equals the reference.
    Inside,
    /// Where it equals the reference, it counts one shallower.
    Release,
}

impl Stencil {
    fn state(self) -> wgpu::StencilState {
        let pass_op = match self {
            Self::Ignored => return wgpu::StencilState::default(),
            Self::Mark => wgpu::StencilOperation::IncrementClamp,
            Self::Inside => wgpu::StencilOperation::Keep,
            Self::Release => wgpu::StencilOperation::DecrementClamp,
        };
        let face = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op,
        };
        wgpu::StencilState {
            front: face,
            back: face,
            read_mask: 0xff,
            write_mask: if self == Self::Inside { 0 } else { 0xff },
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

/// How one surface is drawn: placed by `transform` in the scene, of
/// `material`, at `alpha`, from the content's texels in its texture, and
/// where it lies in the scene's shadow map.
pub(crate) struct SurfaceShading<'m> {
    pub(crate) transform: Mat4,
    pub(crate) material: &'m Material,
    // What the mesh's texture's edges are: an outline is drawn smooth.
    pub(crate) edges: TextureEdges,
    pub(crate) alpha: f32,
    /// The content's top left texel and its size in texels.
    pub(crate) source_region: [f32; 4],
    /// Maps the scene into its shadow map, for a scene that casts shadows.
    pub(crate) light_projection: Option<Mat4>,
    /// The page pixels the copy glass reads starts at, and its size.
    pub(crate) backdrop_region: [f32; 4],
    /// The light and the window of the space the surface is in.
    pub(crate) space: SpaceShading,
}

/// The light on a space's surfaces, in the scene, and for a portal's space
/// the plane of its window, `(normal, offset)`, positive beyond it; all
/// zero for the scene's own space.
#[derive(Clone, Copy)]
pub(crate) struct SpaceShading {
    pub(crate) light: Option<Light>,
    pub(crate) clip: [f32; 4],
}

impl ScenePass {
    // The fixed pipeline of `kind` that takes `samples`.
    pub(crate) fn pipeline(&self, kind: ScenePipeline, samples: Multisample) -> &FixedPipeline {
        &self.pipelines[sample_slot(samples)][kind as usize]
    }

    /// The buffers of the unit square doorways are drawn on, as
    /// [`Self::mesh_buffers`] gives them.
    pub(crate) fn window_buffers(
        &mut self,
        gpu: (&wgpu::Device, &mut impl FrameCommandRecorder),
    ) -> (SceneMeshBuffers, FrameCommandStats) {
        let mesh = self.window.get_or_init(|| Arc::new(Mesh::plane()));
        kept_mesh_buffers(&mut self.meshes, gpu, mesh)
    }

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
        gpu: (&wgpu::Device, &mut impl FrameCommandRecorder),
        mesh: &Arc<Mesh>,
    ) -> (SceneMeshBuffers, FrameCommandStats) {
        kept_mesh_buffers(&mut self.meshes, gpu, mesh)
    }

    /// Keeps the pipelines of `shader`, whose volumes a frame draws.
    pub(crate) fn note_volume(&mut self, shader: &Arc<str>) {
        if self.volume(shader).is_none() {
            self.volumes.push(AppShader::new(
                shader,
                "scene/volume-module",
                [
                    [
                        "scene/volume",
                        "scene/volume-depth",
                        "scene/volume-portal",
                        "scene/volume-portal-depth",
                    ],
                    [
                        "scene/volume-x4",
                        "scene/volume-depth-x4",
                        "scene/volume-portal-x4",
                        "scene/volume-portal-depth-x4",
                    ],
                ],
            ));
        }
    }

    /// The pipelines of `shader`, once a frame noted it.
    pub(crate) fn volume(&self, shader: &Arc<str>) -> Option<&VolumeShader> {
        kept_shader(&self.volumes, shader)
    }

    /// Keeps the pipelines of `shader`, whose shaded surfaces a frame draws.
    pub(crate) fn note_shaded(&mut self, shader: &Arc<str>) {
        if self.shaded(shader).is_none() {
            self.shaded.push(AppShader::new(
                shader,
                "scene/shaded-module",
                [
                    [
                        "scene/shaded",
                        "scene/shaded-shadowed",
                        "scene/shaded-portal",
                        "scene/shaded-glow",
                        "scene/shaded-portal-glow",
                    ],
                    [
                        "scene/shaded-x4",
                        "scene/shaded-shadowed-x4",
                        "scene/shaded-portal-x4",
                        "scene/shaded-glow-x4",
                        "scene/shaded-portal-glow-x4",
                    ],
                ],
            ));
        }
    }

    /// The pipelines of `shader`, once a frame noted it.
    pub(crate) fn shaded(&self, shader: &Arc<str>) -> Option<&ShadedShader> {
        kept_shader(&self.shaded, shader)
    }

    /// Drops the buffers of meshes no recent frame drew, the pipelines of
    /// volume shaders nobody holds, and the shadow map once no recent frame
    /// cast shadows.
    pub(crate) fn end_frame(&mut self) {
        self.meshes.end_frame();
        self.volumes.retain(|kept| kept.source.strong_count() > 0);
        self.shaded.retain(|kept| kept.source.strong_count() > 0);
        self.shadow_map_idle += 1;
        if self.shadow_map_idle > IDLE_FRAMES {
            self.shadow_map = None;
        }
    }
}

/// The buffers of `mesh` among `meshes`, staged through `recorder` the
/// first time a frame draws it (see [`ScenePass::mesh_buffers`]).
fn kept_mesh_buffers(
    meshes: &mut IdlePool<KeptMesh>,
    (device, recorder): (&wgpu::Device, &mut impl FrameCommandRecorder),
    mesh: &Arc<Mesh>,
) -> (SceneMeshBuffers, FrameCommandStats) {
    let id = mesh.id();
    let mut stats = FrameCommandStats::default();
    let buffers = match meshes.take(|kept| kept.id == id) {
        Some(kept) => kept.buffers,
        None => {
            let vertices = scene_vertices(mesh);
            let vertices: &[u8] = bytemuck::cast_slice(&vertices);
            let indices: &[u8] = bytemuck::cast_slice(mesh.indices());
            let freed = meshes.take(|kept| {
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
    meshes.put(
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

fn pipeline_parts(
    device: &wgpu::Device,
    backend: wgpu::Backend,
    texture_layout: &wgpu::BindGroupLayout,
) -> ScenePipelineParts {
    let uniform_layout = crate::offscreen::dynamic_uniform_layout(
        device,
        "Scene Surface Uniform Bind Group Layout",
        wgpu::ShaderStages::VERTEX_FRAGMENT,
    );
    let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Scene Shadow Map Bind Group Layout"),
        entries: &[
            shadow_map_entry(0, wgpu::TextureSampleType::Depth),
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
            shadow_map_entry(2, wgpu::TextureSampleType::Float { filterable: false }),
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
    let shaded_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Scene Shaded Surface Pipeline Layout"),
        bind_group_layouts: &[
            Some(texture_layout),
            Some(&uniform_layout),
            Some(&shadow_layout),
            Some(&uniform_layout),
        ],
        immediate_size: 0,
    });
    ScenePipelineParts {
        shader,
        uniform_layout,
        shadow_layout,
        shadow_samplers,
        no_shadow,
        volume_layout,
        shaded_layout,
    }
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

// The build of one scene pipeline for targets of `format` and `samples`.
pub(crate) fn scene_pipeline_job(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    (format, kind, samples): (wgpu::TextureFormat, ScenePipeline, Multisample),
    parts: &ScenePipelineParts,
) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
    let device = Arc::clone(device);
    let shader = parts.shader.clone();
    move || {
        let module = shader.module();
        let spec = kind.spec(samples);
        let color_target = color_target(format, spec.color);
        let shadow = kind == ScenePipeline::Shadow;
        let targets: &[Option<wgpu::ColorTargetState>] = if shadow { &[] } else { &color_target };
        create_scene_pipeline(
            &device,
            cache.as_ref(),
            spec.label,
            (shader.layout(), module, spec.entry_points),
            (targets, depth_state(&spec, shadow), spec.samples),
        )
    }
}

/// The depth and stencil a pipeline of `spec` uses: the shadow map's for
/// the shadow pass, biased so a caster does not shadow itself, and the
/// scene's otherwise.
fn depth_state(spec: &PipelineSpec, shadow: bool) -> wgpu::DepthStencilState {
    let (compare, write) = match spec.depth {
        Depth::Tested => (wgpu::CompareFunction::Less, true),
        Depth::Read => (wgpu::CompareFunction::LessEqual, false),
        Depth::Overwrite => (wgpu::CompareFunction::Always, true),
    };
    wgpu::DepthStencilState {
        format: if shadow {
            DEPTH_FORMAT
        } else {
            SCENE_DEPTH_FORMAT
        },
        depth_write_enabled: Some(write),
        depth_compare: Some(compare),
        stencil: spec.stencil.state(),
        bias: if shadow {
            wgpu::DepthBiasState {
                constant: 2,
                slope_scale: 2.0,
                clamp: 0.0,
            }
        } else {
            wgpu::DepthBiasState::default()
        },
    }
}

/// The build of one volume shader's pipeline for targets of `format`, of
// `variant` and `samples`; `None` when the shader does not build or nobody
// holds it any more.
pub(crate) fn volume_pipeline_job(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    (format, backend): (wgpu::TextureFormat, wgpu::Backend),
    (variant, samples): (VolumeVariant, Multisample),
    (parts, kept): (&ScenePipelineParts, &VolumeShader),
) -> impl FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static {
    app_pipeline_job(
        device,
        cache,
        (format, backend, variant.spec(samples)),
        (&parts.volume_layout, kept, create_volume_shader_module),
    )
}

/// The build of one shaded surface's shader's pipeline for targets of
// `format`, of `variant` and `samples`; `None` when the shader does not
// build or nobody holds it any more.
pub(crate) fn shaded_pipeline_job(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    (format, backend): (wgpu::TextureFormat, wgpu::Backend),
    (variant, samples): (ShadedVariant, Multisample),
    (parts, kept): (&ScenePipelineParts, &ShadedShader),
) -> impl FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static {
    app_pipeline_job(
        device,
        cache,
        (format, backend, variant.spec(samples)),
        (&parts.shaded_layout, kept, create_shaded_shader_module),
    )
}

/// The module an app shader's source builds, after the framework's part,
/// for a backend; `None` when it does not build.
type AppModule = fn(&wgpu::Device, &str, wgpu::Backend) -> Option<wgpu::ShaderModule>;

/// The build of one of an app shader's pipelines, of `spec`, for targets of
/// `format`, its module built once by `module_of`.
fn app_pipeline_job<const N: usize>(
    device: &Arc<wgpu::Device>,
    cache: Option<wgpu::PipelineCache>,
    (format, backend, spec): (wgpu::TextureFormat, wgpu::Backend, PipelineSpec),
    (layout, kept, module_of): (&wgpu::PipelineLayout, &AppShader<N>, AppModule),
) -> impl FnOnce() -> Option<wgpu::RenderPipeline> + CompilerSend + 'static {
    let device = Arc::clone(device);
    let layout = layout.clone();
    let module = kept.module.clone();
    let source = kept.source.upgrade();
    move || {
        let source = source?;
        let module = module
            .get_or_init(backend, || module_of(&device, &source, backend))
            .as_ref()?;
        Some(create_scene_pipeline(
            &device,
            cache.as_ref(),
            spec.label,
            (&layout, module, spec.entry_points),
            (
                &color_target(format, spec.color),
                depth_state(&spec, false),
                spec.samples,
            ),
        ))
    }
}

/// The colour target of a scene pipeline, which blends colour as `blend`
/// does, and writes none without one.
fn color_target(
    format: wgpu::TextureFormat,
    blend: Option<Blend>,
) -> [Option<wgpu::ColorTargetState>; 1] {
    let added = wgpu::BlendState {
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
    };
    [Some(wgpu::ColorTargetState {
        format,
        blend: Some(match blend {
            Some(Blend::Add) => added,
            _ => wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        }),
        write_mask: if blend.is_some() {
            wgpu::ColorWrites::ALL
        } else {
            wgpu::ColorWrites::empty()
        },
    })]
}

/// A pipeline that draws the scene's meshes from `module`'s `entry_points`
// onto `targets` of `samples`, against `depth_stencil`.
fn create_scene_pipeline(
    device: &wgpu::Device,
    cache: Option<&wgpu::PipelineCache>,
    label: &'static str,
    (layout, module, (vertex, fragment)): (
        &wgpu::PipelineLayout,
        &wgpu::ShaderModule,
        (&'static str, &'static str),
    ),
    (targets, depth_stencil, samples): (
        &[Option<wgpu::ColorTargetState>],
        wgpu::DepthStencilState,
        Multisample,
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
                    array_stride: size_of::<SceneVertex>() as u64,
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
            depth_stencil: Some(depth_stencil),
            multisample: wgpu::MultisampleState {
                count: samples.samples(),
                ..wgpu::MultisampleState::default()
            },
            multiview_mask: None,
            cache: None,
        },
    )
}
