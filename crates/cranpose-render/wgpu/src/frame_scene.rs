//! A scene's mesh surfaces in a layer's render: each surface's content is
//! rendered as an isolated layer's is, then all surfaces of the scene draw
//! over what lies beneath the first of them, depth-tested against each
//! other: first the surfaces that are neither glass nor glow, then those
//! an app's shader colours, then the volumes, then the spaces the scene's
//! portals show inside their windows, then glass over a copy of the page
//! they left, then glow added onto it, then the glow an app's shader
//! colours.

use std::{rc::Rc, sync::Arc};

use cranpose_ui_graphics::{
    Rect,
    space::{Mat4, Material, Multisample, Vec3},
};

use super::{
    CompositeSource, DeviceRect, Event, FrameExecutor, LayerPass, SurfaceRender,
    composites_nothing, draws_nothing,
};
use crate::{
    collect::{ChildLayer, MeshChild, SceneDraw},
    draw_pass::SourceContent,
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
#[path = "frame_scene_target.rs"]
mod target;

use self::{
    portals::{PortalPass, PreparedWindows, encode_portals},
    target::SceneTarget,
};

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
    // Glow an app's shader colours, added onto the page, each shader with
    // its own pipeline.
    ShadedGlow,
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
            (Material::Glow | Material::ShadedGlow(_), true) => Self::PortalGlow,
            (Material::ShadedGlow(_), false) => Self::ShadedGlow,
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

// Whether a shadow can fall on `mesh`: a surface that emits its colour or
// glows takes none, nor does a volume, nor a space seen through a doorway.
fn takes_shadow(mesh: &MeshChild) -> bool {
    mesh.seen == 0
        && !matches!(
            mesh.surface.material,
            Material::Emissive | Material::Glow | Material::ShadedGlow(_) | Material::Volume(_)
        )
}

// The map of `scene`'s light into its shadow map, for the shadows the
// space the eye stands in casts onto itself; `None` without shadows.
fn shadow_projection(scene: &SceneDraw, meshes: &[PendingMesh]) -> Option<Mat4> {
    let light = scene.light.filter(|light| light.shadows)?;
    light.shadow_projection(
        meshes
            .iter()
            .filter(|held| casts_shadow(&held.mesh))
            .map(|held| &held.mesh.surface),
        meshes
            .iter()
            .filter(|held| takes_shadow(&held.mesh))
            .map(|held| &held.mesh.surface),
        (&scene.camera, scene.rect.size()),
    )
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

#[expect(clippy::multiple_inherent_impl, reason = "3D steps in their files")]
impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    // Renders the content of a mesh surface, whole, once, and holds it
    // for its scene's pass on each of its meshes.
    pub(super) fn resolve_mesh_child(
        &mut self,
        pass: &mut LayerPass<'_>,
        child: &ChildLayer,
    ) -> Result<(), String> {
        let Some(owner) = child.mesh.first() else {
            return Ok(());
        };
        // A volume shows what its shader finds, with or without content.
        let volume = child
            .mesh
            .iter()
            .any(|mesh| matches!(mesh.surface.material, Material::Volume(_)));
        if composites_nothing(child) && (child.alpha == 0.0 || !volume) {
            return Ok(());
        }
        // An app's shader colours a mesh whose node draws nothing on its whole surface. The mesh
        // reads one white texel, so no pass renders the empty content.
        let shaded = |mesh: &Arc<MeshChild>| {
            matches!(
                mesh.surface.material,
                Material::Shaded(_) | Material::ShadedGlow(_)
            )
        };
        let surface = if draws_nothing(&child.content) && child.mesh.iter().all(shaded) {
            self.white_surface()
        } else {
            let shown = DeviceRect::from_logical(owner.scene.rect, pass.scale);
            let Some(surface) =
                self.render_child_surface(pass, child, child.z_index, None, shown)?
            else {
                return Ok(());
            };
            surface
        };
        for mesh in &child.mesh {
            pass.meshes.push(PendingMesh {
                z: child.z_index,
                depth: view_depth(&mesh.scene, &mesh.surface),
                mesh: Arc::clone(mesh),
                surface: surface.clone(),
                alpha: child.alpha,
                clip: child.clip,
            });
        }
        Ok(())
    }

    // The surface of a shaded mesh with no content: one white texel, which every texture
    // coordinate of the mesh reads.
    fn white_surface(&mut self) -> SurfaceRender {
        let renderer = &*self.renderer;
        let texture = renderer
            .scene_pass
            .white
            .texture(&renderer.device, &renderer.queue);
        SurfaceRender {
            source: CompositeSource {
                texture,
                content: SourceContent::Transient,
            },
            rect: DeviceRect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
            scale: 1.0,
            grid_dest: None,
            region: None,
        }
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
                .first()
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
        let scene = Arc::clone(&first.mesh.scene);
        self.run_stages(pass)?;
        // A scene that averages its samples into a blank page replaces the page's clear
        let into_page = pass.resolves_into_page(&scene, first_z);
        if !into_page {
            self.flush_page(pass, first_z)?;
            self.start_page(pass)?;
        }
        let mut meshes = std::mem::take(&mut pass.meshes);
        meshes.sort_by(|a, b| b.depth.total_cmp(&a.depth));
        let drew = self.encode_scene_pass(pass, &meshes, (first_z, into_page))?;
        if into_page {
            if drew {
                pass.load_op = None;
                pass.drawn_z = pass.drawn_z.max(first_z);
            } else {
                // No pass ran, so the page still owes its clear
                self.flush_page(pass, first_z)?;
                self.start_page(pass)?;
            }
        }
        Ok(())
    }

    // Draws the scene's surfaces; returns whether the pass of the first colour ran.
    fn encode_scene_pass(
        &mut self,
        pass: &mut LayerPass<'_>,
        meshes: &[PendingMesh],
        (first_z, into_page): (usize, bool),
    ) -> Result<bool, String> {
        let Some(first) = meshes.first() else {
            return Ok(false);
        };
        let scene = Arc::clone(&first.mesh.scene);
        let Some(target) = self.scene_target(pass, &scene, (first_z, into_page)) else {
            return Ok(false);
        };
        let full_page = ScenePage {
            scale: pass.scale,
            offset: pass.page.offset,
            size: (pass.page.texture.width, pass.page.texture.height),
        };
        let page = target.page;
        let light_projection = shadow_projection(&scene, meshes);
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
        let mut draws = Vec::with_capacity(meshes.len());
        for held in meshes {
            let shown = (page, eye, light_projection, backdrop_region);
            if let Some(draw) = self.prepare_surface(held, shown) {
                draws.push(draw);
            }
        }
        let shadows = light_projection.is_some() && self.draw_shadow_map(&draws, target.samples);
        let windows = self.prepare_windows(&draws, (&scene, page, eye));
        let opaque = StepPipeline::Fixed(if shadows {
            ScenePipeline::Shadowed
        } else {
            ScenePipeline::Plain
        });
        let shaded = shaded_step(shadows);
        let volumes = StepPipeline::Volumes(VolumeVariant::Color);
        let portals = |pass: PortalPass| (StepPipeline::Portals(pass), ScenePhase::Portal);
        let glow = (StepPipeline::Fixed(ScenePipeline::Glow), ScenePhase::Glow);
        let Some(backdrop) = backdrop else {
            let steps = [
                (opaque, ScenePhase::Opaque),
                shaded,
                (volumes, ScenePhase::Volume),
                portals(PortalPass::Colour { glow: true }),
                glow,
                SHADED_GLOW,
            ];
            let drawn = self.encode_steps(
                (pass, &target, shadows),
                &steps,
                (&draws, windows.as_ref(), None),
            );
            self.write_host_depth(
                (pass, &target, full_page),
                (&steps, drawn.filter(|_| into_page)),
                &draws,
            );
            return Ok(drawn.is_some());
        };
        // Glass reads the page the other surfaces left, so draws in a pass
        // of its own after the copy, over their depth drawn again.
        let first = self.encode_steps(
            (pass, &target, shadows),
            &[
                (opaque, ScenePhase::Opaque),
                shaded,
                (volumes, ScenePhase::Volume),
                portals(PortalPass::Colour { glow: false }),
            ],
            (&draws, windows.as_ref(), None),
        );
        self.write_host_depth((pass, &target, full_page), (&[], None), &draws);
        self.encode_glass(
            (pass, &target, shadows),
            (&draws, windows.as_ref(), backdrop),
            first_z,
        )?;
        Ok(first.is_some())
    }

    // Draws the scene's glass in a pass of its own, over a copy of the page that the other
    // surfaces left and their depth drawn again.
    fn encode_glass(
        &mut self,
        (pass, target, shadows): (&mut LayerPass<'_>, &SceneTarget, bool),
        (draws, windows, backdrop): (&[PreparedSurface<'_>], Option<&PreparedWindows>, DeviceRect),
        first_z: usize,
    ) -> Result<(), String> {
        if self
            .renderer
            .scene_surface_pipeline(ScenePipeline::Glass, target.samples)
            .is_none()
        {
            return Ok(());
        }
        let below = target.drawn_below(first_z);
        let copy = self.capture(pass, below, backdrop, "Scene Glass Backdrop")?;
        self.encode_steps(
            (pass, target, shadows),
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
                (StepPipeline::Portals(PortalPass::Glass), ScenePhase::Portal),
                (StepPipeline::Fixed(ScenePipeline::Glass), ScenePhase::Glass),
                (StepPipeline::Fixed(ScenePipeline::Glow), ScenePhase::Glow),
                SHADED_GLOW,
            ],
            (draws, windows, Some(&copy)),
        );
        Ok(())
    }

    // Leaves the host's depth for a scene of several samples, which the pass of the samples cannot
    // store: one wgpu depth attachment holds one sample count, and a depth of four samples per
    // pixel costs four times the bandwidth to store and read back. A second pass with one sample
    // draws the surfaces again with the depth pipelines, so each pixel holds the depth of its
    // centre. The pass draws only the steps that the colour pass drew, so the depth never shows
    // what the picture does not. `drawn` is `None` where the scene did not average into the page
    // whole: the host's depth then holds the far plane, as the clear leaves it. A host with no
    // depth, and a scene of one sample, which stores its depth in its own pass, run nothing.
    fn write_host_depth(
        &mut self,
        (pass, target, page): (&mut LayerPass<'_>, &SceneTarget, ScenePage),
        (steps, drawn): (&[(StepPipeline, ScenePhase)], Option<u8>),
        draws: &[PreparedSurface<'_>],
    ) {
        if target.samples == Multisample::Off {
            return;
        }
        let Some(depth) = self.host_depth_target(page) else {
            return;
        };
        let mut depth_steps = [(
            StepPipeline::Fixed(ScenePipeline::Depth),
            ScenePhase::Opaque,
        ); 5];
        let mut count = 0;
        for (index, &(kind, phase)) in steps.iter().enumerate() {
            let swapped = match kind {
                StepPipeline::Fixed(ScenePipeline::Plain | ScenePipeline::Shadowed) => {
                    StepPipeline::Fixed(ScenePipeline::Depth)
                }
                StepPipeline::Shaded(variant) if !variant.adds() => {
                    StepPipeline::Fixed(ScenePipeline::Depth)
                }
                StepPipeline::Volumes(_) => StepPipeline::Volumes(VolumeVariant::Depth),
                _ => continue,
            };
            if drawn.is_some_and(|mask| mask & (1 << index) != 0) {
                depth_steps[count] = (swapped, phase);
                count += 1;
            }
        }
        let ran = self.encode_steps(
            (pass, &depth, false),
            &depth_steps[..count],
            (draws, None, None),
        );
        if ran.is_none() {
            // The pass clears the depth even where it draws nothing
            drop(self.recorder.begin_scene_pass(
                "Scene Host Depth",
                depth.colour(None),
                depth.depth(),
            ));
        }
    }

    // Draws, in one pass into `target`, each step's surfaces with the
    /// step's pipeline; a step whose pipeline still compiles is left out,
    /// and one without surfaces asks for no pipeline.
    fn encode_steps(
        &mut self,
        (pass, target, shadows): (&mut LayerPass<'_>, &SceneTarget, bool),
        steps: &[(StepPipeline, ScenePhase)],
        (draws, windows, backdrop): (
            &[PreparedSurface<'_>],
            Option<&PreparedWindows>,
            Option<&Rc<OffscreenTarget>>,
        ),
    ) -> Option<u8> {
        let drawn = |phase: ScenePhase| draws.iter().filter(move |draw| draw.phase == phase);
        // The portals' step draws while a window shows any space.
        let empty = |&(kind, phase): &(StepPipeline, ScenePhase)| match kind {
            StepPipeline::Portals(_) => windows.is_none(),
            _ => drawn(phase).next().is_none(),
        };
        if steps.iter().all(empty) {
            return None;
        }
        let mut drew = 0_u8;
        let resolved = self.scene_resolved(target);
        let samples = target.samples;
        let renderer = &*self.renderer;
        let shadow_group = match (shadows, &renderer.scene_pass.shadow_map) {
            (true, Some(map)) => &map.group,
            _ => renderer.scene_no_shadow(),
        };
        let effects = &renderer.effect_renderer;
        let device = &renderer.device;
        let backdrop = backdrop.map(|texture| {
            texture.get_or_create_bind_group(
                device,
                &effects.effect_texture_bind_group_layout,
                &effects.effect_linear_sampler,
            )
        });
        let colour = target.colour(resolved.as_deref());
        let mut render_pass =
            self.recorder
                .begin_scene_pass("Scene Surfaces", colour, target.depth());
        render_pass.set_bind_group(2, shadow_group, &[]);
        for (index, step) in steps.iter().enumerate() {
            // A pipeline no surface takes is not asked for, so never compiles.
            if empty(step) {
                continue;
            }
            let &(kind, phase) = step;
            let phase_draws = drawn(phase);
            let fixed = match kind {
                StepPipeline::Fixed(kind) => match renderer.scene_surface_pipeline(kind, samples) {
                    Some(pipeline) => Some(pipeline),
                    None => continue,
                },
                StepPipeline::Volumes(_) | StepPipeline::Shaded(_) => None,
                StepPipeline::Portals(portal_pass) => {
                    drew |= 1 << index;
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
            drew |= 1 << index;
            if let Some(pipeline) = fixed {
                render_pass.set_pipeline(pipeline);
            }
            draw_surfaces(
                &mut render_pass,
                (renderer, kind, samples),
                phase_draws,
                (device, backdrop),
            );
        }
        drop(render_pass);
        pass.pending.extend(target.composite(resolved));
        Some(drew)
    }

    /// Draws the scene's shadow casters into the shadow map, as the light
    /// sees them; false while the shadow pipelines compile.
    fn draw_shadow_map(&mut self, draws: &[PreparedSurface<'_>], samples: Multisample) -> bool {
        // Both pipelines are asked for, so both compile side by side.
        let shadowed = self
            .renderer
            .scene_surface_pipeline(ScenePipeline::Shadowed, samples)
            .is_some();
        if !shadowed
            || self
                .renderer
                .scene_pass
                .shadow_map(&self.renderer.device)
                .is_none()
        {
            return false;
        }
        let renderer = &*self.renderer;
        let (Some(pipeline), Some(map)) = (
            renderer.scene_surface_pipeline(ScenePipeline::Shadow, Multisample::Off),
            renderer.scene_pass.shadow_map.as_ref(),
        ) else {
            return false;
        };
        let effects = &renderer.effect_renderer;
        let device = &renderer.device;
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
                    &self.renderer.device,
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
                        edges: surface.mesh.texture_edges(),
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
                    &self.renderer.device,
                    layout,
                    bytemuck::bytes_of(&uniforms),
                )
            }
        };
        let shaded = match &surface.material {
            Material::Shaded(shaded) | Material::ShadedGlow(shaded) => {
                let values = shaded_uniforms((&surface.transform, shaded), (eye, space))?;
                Some(self.recorder.upload_uniform(
                    UploadAllocatorId::SceneShaded,
                    scene_shaded_uniform_spec(),
                    &self.renderer.device,
                    layout,
                    bytemuck::bytes_of(&values),
                ))
            }
            _ => None,
        };
        // The pipelines of an app's shader stay while frames draw with it.
        match &surface.material {
            Material::Volume(volume) => self.renderer.scene_pass.note_volume(&volume.shader),
            Material::Shaded(shaded) | Material::ShadedGlow(shaded) => {
                self.renderer.scene_pass.note_shaded(&shaded.shader);
            }
            _ => {}
        }
        let (buffers, staged) = self
            .renderer
            .scene_pass
            .mesh_buffers((&self.renderer.device, &mut *self.recorder), &surface.mesh);
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
        (variant, samples): (ShadedVariant, Multisample),
    ) -> Option<&'r wgpu::RenderPipeline> {
        let (Material::Shaded(shaded) | Material::ShadedGlow(shaded)) =
            &self.held.mesh.surface.material
        else {
            return None;
        };
        self.shaded.as_ref()?;
        renderer.scene_shaded_pipeline(&shaded.shader, (variant, samples))
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
    (renderer, kind, samples): (&crate::render::GpuRenderer, StepPipeline, Multisample),
    draws: impl Iterator<Item = &'d PreparedSurface<'d>>,
    (device, backdrop): (&wgpu::Device, Option<&wgpu::BindGroup>),
) {
    let effects = &renderer.effect_renderer;
    for draw in draws {
        let mut shaded = false;
        match (kind, &draw.held.mesh.surface.material) {
            (StepPipeline::Volumes(variant), Material::Volume(volume)) => {
                let pipeline = renderer.scene_volume_pipeline(&volume.shader, (variant, samples));
                let Some(pipeline) = pipeline else {
                    continue;
                };
                render_pass.set_pipeline(pipeline);
            }
            (StepPipeline::Shaded(variant), _) => {
                let own = draw.shaded_pipeline(renderer, (variant, samples));
                shaded = own.is_some();
                // A glowing surface draws nothing until its shader builds,
                // so its first frames show no box of its content's colour
                let lit = || {
                    (!variant.adds())
                        .then(|| renderer.scene_surface_pipeline(variant.lit(), samples))
                        .flatten()
                };
                let Some(pipeline) = own.or_else(lit) else {
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

// The step that draws the glow an app's shader colours.
const SHADED_GLOW: (StepPipeline, ScenePhase) = (
    StepPipeline::Shaded(ShadedVariant::Glow),
    ScenePhase::ShadedGlow,
);

// The step that draws the shaded surfaces, lit by the shadow map when the
// scene casts shadows.
fn shaded_step(shadows: bool) -> (StepPipeline, ScenePhase) {
    let variant = if shadows {
        ShadedVariant::Shadowed
    } else {
        ShadedVariant::Plain
    };
    (StepPipeline::Shaded(variant), ScenePhase::Shaded)
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
