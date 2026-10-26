//! A scene's mesh surfaces in a layer's render: each surface's content is
//! rendered as an isolated layer's is, then all surfaces of the scene draw
//! over what lies beneath the first of them, depth-tested against each
//! other: first the surfaces that are neither glass nor glow, then those
//! an app's shader colours, then the volumes, then the spaces the scene's
//! portals show inside their windows, then glass over a copy of the page
//! they left, then glow added onto it.

use std::{rc::Rc, sync::Arc};

use cranpose_ui_graphics::{
    Rect,
    space::{Mat4, Material, Vec3},
};

use super::{DeviceRect, Event, FrameExecutor, LayerPass, SurfaceRender, composites_nothing};
use crate::{
    collect::{ChildLayer, MeshChild},
    frame_graph::{FrameCommandRecorder, UploadAllocatorId},
    offscreen::OffscreenTarget,
    scene_pass::{
        SceneMeshBuffers, ScenePage, ScenePipeline, ShadedVariant, SpaceShading, SurfaceShading,
        VolumeVariant, scene_scissor, scene_shaded_uniform_spec, scene_surface_uniform_spec,
        scene_volume_uniform_spec, shaded_uniforms, surface_page_bounds, surface_uniforms,
        view_depth, volume_uniforms,
    },
};

#[path = "frame_portal.rs"]
mod portals;

use self::portals::{PortalPass, PreparedWindows, encode_portals};

/// The surfaces of a scene that draw together, in the order the phases
/// draw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScenePhase {
    /// The surfaces of the scene's own space that are neither glass nor
    /// glow nor volumes, nor coloured by an app's shader.
    Opaque,
    /// The surfaces of the scene's own space an app's shader colours, each
    /// with its own pipeline.
    Shaded,
    /// Volumes, each drawn with its own shader.
    Volume,
    /// The surfaces in the spaces the scene's portals show, which draw
    /// inside their windows: those that are neither glass nor glow.
    Portal,
    /// Glass in the spaces the scene's portals show.
    PortalGlass,
    /// Glow in the spaces the scene's portals show.
    PortalGlow,
    /// Glass, which reads a copy of the page the opaque surfaces and the
    /// volumes left.
    Glass,
    /// Glow, added onto the page.
    Glow,
}

impl ScenePhase {
    /// The phase `mesh` draws in: its material's, in the space the scene's
    /// eye stands in or through a doorway.
    fn of(mesh: &MeshChild) -> Self {
        let through_doorway = mesh.seen != 0;
        match (&mesh.surface.material, through_doorway) {
            (Material::Glass(_), false) => Self::Glass,
            (Material::Glass(_), true) => Self::PortalGlass,
            (Material::Glow, false) => Self::Glow,
            (Material::Glow, true) => Self::PortalGlow,
            (_, true) => Self::Portal,
            (Material::Volume(_), false) => Self::Volume,
            (Material::Shaded(_), false) => Self::Shaded,
            (Material::Lit | Material::Unlit | Material::Emissive, false) => Self::Opaque,
        }
    }
}

/// What a step draws its surfaces with.
#[derive(Clone, Copy)]
enum StepPipeline {
    /// One of the scene pipelines.
    Fixed(ScenePipeline),
    /// Each volume's own pipeline of the variant.
    Volumes(VolumeVariant),
    /// Each shaded surface's own pipeline of the variant, or the lit
    /// pipeline it takes the place of until it builds.
    Shaded(ShadedVariant),
    /// The portals' windows and the spaces they show.
    Portals(PortalPass),
}

/// Whether `mesh` casts a shadow: a volume's mesh only bounds what it
/// shows, so casts none, and a space seen through a doorway casts none into
/// the eye's.
fn casts_shadow(mesh: &MeshChild) -> bool {
    let surface = &mesh.surface;
    mesh.seen == 0 && surface.casts_shadow && !matches!(surface.material, Material::Volume(_))
}

/// The light and the cut of the space `mesh` lies in, as the scene's eye
/// sees it: a space seen through a doorway shows only past the doorway.
fn space_shading(mesh: &MeshChild) -> SpaceShading {
    let space = &mesh.scene.seen[mesh.seen];
    SpaceShading {
        light: space.light,
        clip: space.door.map_or([0.0; 4], |door| door.clip),
    }
}

/// A mesh surface whose content is rendered, waiting for its scene's pass.
pub(super) struct PendingMesh {
    z: usize,
    /// How far the surface lies from the eye, for drawing far to near.
    depth: f32,
    mesh: Arc<MeshChild>,
    surface: SurfaceRender,
    alpha: f32,
    clip: Option<Rect>,
}

impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    /// Renders the content of a mesh surface, whole, and holds it for its
    /// scene's pass.
    pub(super) fn resolve_mesh_child(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
        mesh: &Arc<MeshChild>,
    ) -> Result<(), String> {
        // A volume shows what its shader finds, with or without content.
        let volume = matches!(mesh.surface.material, Material::Volume(_));
        if composites_nothing(child) && (child.alpha == 0.0 || !volume) {
            return Ok(());
        }
        let shown = DeviceRect::from_logical(mesh.scene.rect, pass.scale);
        let Some(surface) = self.render_child_surface(pass, child, child.z_index, None, shown)?
        else {
            return Ok(());
        };
        pass.meshes.push(PendingMesh {
            z: child.z_index,
            depth: view_depth(&mesh.scene, &mesh.surface.transform),
            mesh: Arc::clone(mesh),
            surface,
            alpha: child.alpha,
            clip: child.clip,
        });
        Ok(())
    }

    /// Draws the held surfaces before `event`, unless it is another surface
    /// of their scene.
    pub(super) fn end_scene_before(
        &mut self,
        pass: &mut LayerPass<'_>,
        event: &Event,
    ) -> Result<(), String> {
        let Some(held) = pass.meshes.first() else {
            return Ok(());
        };
        let same_scene = match event {
            Event::Child(index) => pass.layer.children[*index]
                .mesh
                .as_ref()
                .is_some_and(|mesh| Arc::ptr_eq(&mesh.scene, &held.mesh.scene)),
            Event::Backdrop(_) | Event::Effect(_) | Event::Shadow(_) => false,
        };
        if same_scene {
            return Ok(());
        }
        self.draw_meshes(pass)
    }

    /// Draws the held surfaces of one scene in a pass of their own, over
    /// what the page holds beneath the first of them.
    pub(super) fn draw_meshes(&mut self, pass: &mut LayerPass<'_>) -> Result<(), String> {
        let Some(first) = pass.meshes.first() else {
            return Ok(());
        };
        let first_z = first.z;
        self.run_stages(pass)?;
        self.flush_page(pass, first_z)?;
        self.start_page(pass)?;
        let mut meshes = std::mem::take(&mut pass.meshes);
        meshes.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        self.encode_scene_pass(pass, &meshes, first_z)
    }

    fn encode_scene_pass(
        &mut self,
        pass: &mut LayerPass<'_>,
        meshes: &[PendingMesh],
        first_z: usize,
    ) -> Result<(), String> {
        let Some(first) = meshes.first() else {
            return Ok(());
        };
        let scene = Arc::clone(&first.mesh.scene);
        let page_texture = Rc::clone(&pass.page.texture);
        let page = ScenePage {
            scale: pass.scale,
            offset: pass.page.offset,
            size: (page_texture.width, page_texture.height),
        };
        // The space the eye stands in casts shadows, onto itself.
        let shadowing = scene.light.filter(|light| light.shadows);
        let light_projection = shadowing.and_then(|light| {
            light.shadow_projection(
                meshes
                    .iter()
                    .filter(|held| casts_shadow(&held.mesh))
                    .map(|held| &held.mesh.surface),
            )
        });
        let backdrop = glass_backdrop(meshes, page);
        let backdrop_region = backdrop.map_or([0.0; 4], |rect| {
            [
                rect.x - page.offset[0],
                rect.y - page.offset[1],
                rect.width,
                rect.height,
            ]
        });
        let eye = scene.camera.eye();
        let device = Arc::clone(&self.renderer.device);
        let mut draws = Vec::with_capacity(meshes.len());
        for held in meshes {
            let shown = (page, eye, light_projection, backdrop_region);
            if let Some(draw) = self.prepare_surface(held, shown, &device) {
                draws.push(draw);
            }
        }
        let shadows = light_projection.is_some() && self.draw_shadow_map(&draws, &device);
        let windows = self.prepare_windows(&draws, (&scene, page, eye), &device);
        let opaque = StepPipeline::Fixed(if shadows {
            ScenePipeline::Shadowed
        } else {
            ScenePipeline::Plain
        });
        let shaded = (
            StepPipeline::Shaded(if shadows {
                ShadedVariant::Shadowed
            } else {
                ShadedVariant::Plain
            }),
            ScenePhase::Shaded,
        );
        let volumes = StepPipeline::Volumes(VolumeVariant::Color);
        let portals = |pass: PortalPass| (StepPipeline::Portals(pass), ScenePhase::Portal);
        let glow = (StepPipeline::Fixed(ScenePipeline::Glow), ScenePhase::Glow);
        let depth = self
            .renderer
            .depth_targets
            .scene(&self.renderer.device, page.size);
        let Some(backdrop) = backdrop else {
            self.encode_steps(
                (&page_texture, &depth, shadows),
                &[
                    (opaque, ScenePhase::Opaque),
                    shaded,
                    (volumes, ScenePhase::Volume),
                    portals(PortalPass::Colour { glow: true }),
                    glow,
                ],
                (&draws, windows.as_ref(), None),
                &device,
            );
            return Ok(());
        };
        // Glass reads the page the other surfaces left, so draws in a pass
        // of its own after the copy, over their depth drawn again.
        self.encode_steps(
            (&page_texture, &depth, shadows),
            &[
                (opaque, ScenePhase::Opaque),
                shaded,
                (volumes, ScenePhase::Volume),
                portals(PortalPass::Colour { glow: false }),
            ],
            (&draws, windows.as_ref(), None),
            &device,
        );
        if self
            .renderer
            .scene_surface_pipeline(ScenePipeline::Glass)
            .is_none()
        {
            return Ok(());
        }
        let copy = self.capture(pass, first_z, backdrop, "Scene Glass Backdrop")?;
        self.encode_steps(
            (&page_texture, &depth, shadows),
            &[
                (
                    StepPipeline::Fixed(ScenePipeline::Depth),
                    ScenePhase::Opaque,
                ),
                (
                    StepPipeline::Fixed(ScenePipeline::Depth),
                    ScenePhase::Shaded,
                ),
                (
                    StepPipeline::Volumes(VolumeVariant::Depth),
                    ScenePhase::Volume,
                ),
                // Glass beyond the doorways first, then the nearer glass of
                // the eye's own space.
                portals(PortalPass::Glass),
                (StepPipeline::Fixed(ScenePipeline::Glass), ScenePhase::Glass),
                glow,
            ],
            (&draws, windows.as_ref(), Some(&copy)),
            &device,
        );
        Ok(())
    }

    /// Draws, in one pass onto the page, each step's surfaces with the
    /// step's pipeline; a step whose pipeline still compiles is left out,
    /// and one without surfaces asks for no pipeline.
    fn encode_steps(
        &mut self,
        (page, depth, shadows): (&OffscreenTarget, &wgpu::TextureView, bool),
        steps: &[(StepPipeline, ScenePhase)],
        (draws, windows, backdrop): (
            &[PreparedSurface<'_>],
            Option<&PreparedWindows>,
            Option<&Rc<OffscreenTarget>>,
        ),
        device: &wgpu::Device,
    ) {
        let drawn = |phase: ScenePhase| draws.iter().filter(move |draw| draw.phase == phase);
        // The portals' step draws while a window shows any space.
        let empty = |&(kind, phase): &(StepPipeline, ScenePhase)| match kind {
            StepPipeline::Portals(_) => windows.is_none(),
            _ => drawn(phase).next().is_none(),
        };
        if steps.iter().all(empty) {
            return;
        }
        let renderer = &*self.renderer;
        let shadow_group = match (shadows, &renderer.scene_pass.shadow_map) {
            (true, Some(map)) => &map.group,
            _ => renderer.scene_no_shadow(),
        };
        let effects = &renderer.effect_renderer;
        let backdrop = backdrop.map(|texture| {
            texture.get_or_create_bind_group(
                device,
                &effects.effect_texture_bind_group_layout,
                &effects.effect_linear_sampler,
            )
        });
        let mut render_pass = self
            .recorder
            .begin_scene_pass("Scene Surfaces", &page.view, depth);
        render_pass.set_bind_group(2, shadow_group, &[]);
        for step in steps {
            // A pipeline no surface takes is not asked for, so never compiles.
            if empty(step) {
                continue;
            }
            let &(kind, phase) = step;
            let phase_draws = drawn(phase);
            let fixed = match kind {
                StepPipeline::Fixed(kind) => match renderer.scene_surface_pipeline(kind) {
                    Some(pipeline) => Some(pipeline),
                    None => continue,
                },
                StepPipeline::Volumes(_) | StepPipeline::Shaded(_) => None,
                StepPipeline::Portals(portal_pass) => {
                    if let (Some(first), Some(windows)) = (draws.first(), windows) {
                        let texture = first.texture((device, effects));
                        encode_portals(
                            &mut render_pass,
                            renderer,
                            (draws, windows, portal_pass),
                            (0, 0),
                            (device, texture, backdrop),
                        );
                    }
                    continue;
                }
            };
            if let Some(pipeline) = fixed {
                render_pass.set_pipeline(pipeline);
            }
            draw_surfaces(
                &mut render_pass,
                (renderer, kind),
                phase_draws,
                (device, backdrop),
            );
        }
    }

    /// Draws the scene's shadow casters into the shadow map, as the light
    /// sees them; false while the shadow pipelines compile.
    fn draw_shadow_map(&mut self, draws: &[PreparedSurface<'_>], device: &wgpu::Device) -> bool {
        // Both pipelines are asked for, so both compile side by side.
        let shadowed = self
            .renderer
            .scene_surface_pipeline(ScenePipeline::Shadowed)
            .is_some();
        if !shadowed || self.renderer.scene_pass.shadow_map(device).is_none() {
            return false;
        }
        let renderer = &*self.renderer;
        let (Some(pipeline), Some(map)) = (
            renderer.scene_surface_pipeline(ScenePipeline::Shadow),
            renderer.scene_pass.shadow_map.as_ref(),
        ) else {
            return false;
        };
        let effects = &renderer.effect_renderer;
        let mut shadow_pass = self
            .recorder
            .begin_timed_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Scene Shadow Map"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &map.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
        shadow_pass.set_pipeline(pipeline);
        shadow_pass.set_bind_group(2, renderer.scene_no_shadow(), &[]);
        for draw in draws.iter().filter(|draw| casts_shadow(&draw.held.mesh)) {
            draw.bind(&mut shadow_pass, (device, effects), None);
            shadow_pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
        }
        true
    }

    /// One surface's buffers, uniforms and scissor; `None` when nothing of
    /// it can show.
    fn prepare_surface<'m>(
        &mut self,
        held: &'m PendingMesh,
        (page, eye, light_projection, backdrop_region): (ScenePage, Vec3, Option<Mat4>, [f32; 4]),
        device: &wgpu::Device,
    ) -> Option<PreparedSurface<'m>> {
        let scene = &held.mesh.scene;
        let surface = &held.mesh.surface;
        let phase = ScenePhase::of(&held.mesh);
        let scissor = scene_scissor(scene, held.clip, page)?;
        let layout = self.renderer.scene_uniform_layout();
        let space = space_shading(&held.mesh);
        let uniform = match &surface.material {
            Material::Volume(volume) => {
                let uniforms = volume_uniforms(
                    scene,
                    (&surface.transform, &surface.mesh, volume),
                    (held.alpha, source_region(&held.surface, held.mesh.content)),
                    (page, eye, space),
                )?;
                self.recorder.upload_uniform(
                    UploadAllocatorId::SceneVolume,
                    scene_volume_uniform_spec(),
                    device,
                    layout,
                    bytemuck::bytes_of(&uniforms),
                )
            }
            material => {
                let uniforms = surface_uniforms(
                    scene,
                    &SurfaceShading {
                        transform: surface.transform,
                        material,
                        alpha: held.alpha,
                        source_region: source_region(&held.surface, held.mesh.content),
                        light_projection: light_projection.filter(|_| held.mesh.seen == 0),
                        backdrop_region,
                        space,
                    },
                    (page, eye),
                )?;
                self.recorder.upload_uniform(
                    UploadAllocatorId::SceneSurface,
                    scene_surface_uniform_spec(),
                    device,
                    layout,
                    bytemuck::bytes_of(&uniforms),
                )
            }
        };
        let shaded = match &surface.material {
            Material::Shaded(shaded) => {
                let values = shaded_uniforms((&surface.transform, shaded), (eye, space))?;
                Some(self.recorder.upload_uniform(
                    UploadAllocatorId::SceneShaded,
                    scene_shaded_uniform_spec(),
                    device,
                    layout,
                    bytemuck::bytes_of(&values),
                ))
            }
            _ => None,
        };
        // The pipelines of an app's shader stay while frames draw with it.
        match &surface.material {
            Material::Volume(volume) => self.renderer.scene_pass.note_volume(&volume.shader),
            Material::Shaded(shaded) => self.renderer.scene_pass.note_shaded(&shaded.shader),
            _ => {}
        }
        let (buffers, staged) = self
            .renderer
            .scene_pass
            .mesh_buffers((device, &mut *self.recorder), &surface.mesh);
        self.renderer.frame_stats.record_command_stats(staged);
        Some(PreparedSurface {
            held,
            phase,
            buffers,
            uniform,
            shaded,
            scissor,
        })
    }
}

struct PreparedSurface<'m> {
    held: &'m PendingMesh,
    phase: ScenePhase,
    buffers: SceneMeshBuffers,
    uniform: crate::frame_graph::UniformUpload,
    /// For a shaded surface, the values its shader reads.
    shaded: Option<crate::frame_graph::UniformUpload>,
    scissor: (u32, u32, u32, u32),
}

impl PreparedSurface<'_> {
    /// The seen space the surface lies in.
    fn seen(&self) -> usize {
        self.held.mesh.seen
    }

    /// The surface's texture, bound as the scene's shaders read it.
    fn texture(
        &self,
        (device, effects): (&wgpu::Device, &crate::effect_renderer::EffectRenderer),
    ) -> &wgpu::BindGroup {
        self.held.surface.source.texture.get_or_create_bind_group(
            device,
            &effects.effect_texture_bind_group_layout,
            &effects.effect_linear_sampler,
        )
    }

    /// The pipeline of `variant` the shader of a shaded surface built, and
    /// the values it reads, which [`Self::bind_shaded`] binds; `None` for
    /// any other surface, and for a shaded one while its shader builds and
    /// when it does not, which then draws lit.
    fn shaded_pipeline<'r>(
        &self,
        renderer: &'r crate::render::GpuRenderer,
        variant: ShadedVariant,
    ) -> Option<&'r wgpu::RenderPipeline> {
        let Material::Shaded(shaded) = &self.held.mesh.surface.material else {
            return None;
        };
        self.shaded.as_ref()?;
        renderer.scene_shaded_pipeline(&shaded.shader, variant)
    }

    /// Binds the values a shaded surface's shader reads, in the place of
    /// the copy of the page glass reads, after [`Self::bind`].
    fn bind_shaded(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(values) = &self.shaded {
            pass.set_bind_group(3, &values.bind_group, &[values.offset]);
        }
    }

    /// Binds the surface's texture, uniforms and mesh, and the copy of the
    /// page glass reads: a surface that reads none binds its own texture in
    /// the copy's place.
    fn bind(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (device, effects): (&wgpu::Device, &crate::effect_renderer::EffectRenderer),
        backdrop: Option<&wgpu::BindGroup>,
    ) {
        let texture = self.texture((device, effects));
        pass.set_bind_group(0, texture, &[]);
        pass.set_bind_group(3, backdrop.unwrap_or(texture), &[]);
        pass.set_bind_group(1, &self.uniform.bind_group, &[self.uniform.offset]);
        pass.set_vertex_buffer(0, self.buffers.vertices.slice(..));
        pass.set_index_buffer(self.buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
    }
}

/// Draws `draws`, a step's surfaces, on the pipeline the step set, or each
/// volume and shaded surface on its own pipeline of the step's variant: a
/// shaded surface whose shader has not built on the lit pipeline.
fn draw_surfaces<'d>(
    render_pass: &mut wgpu::RenderPass<'_>,
    (renderer, kind): (&crate::render::GpuRenderer, StepPipeline),
    draws: impl Iterator<Item = &'d PreparedSurface<'d>>,
    (device, backdrop): (&wgpu::Device, Option<&wgpu::BindGroup>),
) {
    let effects = &renderer.effect_renderer;
    for draw in draws {
        let mut shaded = false;
        match (kind, &draw.held.mesh.surface.material) {
            (StepPipeline::Volumes(variant), Material::Volume(volume)) => {
                let Some(pipeline) = renderer.scene_volume_pipeline(&volume.shader, variant) else {
                    continue;
                };
                render_pass.set_pipeline(pipeline);
            }
            (StepPipeline::Shaded(variant), _) => {
                let own = draw.shaded_pipeline(renderer, variant);
                shaded = own.is_some();
                let Some(pipeline) = own.or_else(|| renderer.scene_surface_pipeline(variant.lit()))
                else {
                    continue;
                };
                render_pass.set_pipeline(pipeline);
            }
            _ => {}
        }
        let (x, y, width, height) = draw.scissor;
        render_pass.set_scissor_rect(x, y, width, height);
        draw.bind(render_pass, (device, effects), backdrop);
        if shaded {
            draw.bind_shaded(render_pass);
        }
        render_pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
    }
}

/// The device pixels a scene's glass reads behind it: the pixels each glass
/// surface covers, widened by half on each side for the rays it bends
/// outward, within the scene; the whole scene where a surface reaches
/// behind the eye. `None` without glass.
fn glass_backdrop(meshes: &[PendingMesh], page: ScenePage) -> Option<DeviceRect> {
    let mut covered: Option<DeviceRect> = None;
    for held in meshes {
        if !matches!(held.mesh.surface.material, Material::Glass(_)) {
            continue;
        }
        let scene = &held.mesh.scene;
        let Some((left, top, width, height)) = scene_scissor(scene, held.clip, page) else {
            continue;
        };
        let shown = DeviceRect {
            x: left as f32 + page.offset[0],
            y: top as f32 + page.offset[1],
            width: width as f32,
            height: height as f32,
        };
        let surface = &held.mesh.surface;
        let reach = surface_page_bounds(scene, &surface.transform, &surface.mesh, page)
            .and_then(|[left, top, right, bottom]| {
                let margin = (right - left).max(bottom - top) * 0.5;
                DeviceRect {
                    x: left + page.offset[0],
                    y: top + page.offset[1],
                    width: right - left,
                    height: bottom - top,
                }
                .expand(margin)
                .intersect(shown)
            })
            .unwrap_or(shown);
        covered = Some(covered.map_or(reach, |covered| covered.union(reach)));
    }
    covered.map(DeviceRect::snap_out)
}

/// The texels of `content`, in the surface's local coordinates, within the
/// surface's texture: its top left texel and its size.
fn source_region(surface: &SurfaceRender, content: Rect) -> [f32; 4] {
    let (region_x, region_y) = surface
        .region
        .map_or((0.0, 0.0), |region| (region.x, region.y));
    [
        region_x + content.x * surface.scale - surface.rect.x,
        region_y + content.y * surface.scale - surface.rect.y,
        content.width * surface.scale,
        content.height * surface.scale,
    ]
}
