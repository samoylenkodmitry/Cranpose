//! A scene's mesh surfaces in a layer's render: each surface's content is
//! rendered as an isolated layer's is, then all surfaces of the scene draw
//! over what lies beneath the first of them, depth-tested against each
//! other: first the surfaces that are neither glass nor glow, then glass
//! over a copy of the page they left, then glow added onto it.

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
        SceneMeshBuffers, ScenePage, ScenePipeline, SurfaceShading, scene_scissor,
        scene_surface_uniform_spec, surface_page_bounds, surface_uniforms, view_depth,
    },
};

/// The surfaces of a scene that draw together, in the order the phases
/// draw.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScenePhase {
    /// The surfaces that are neither glass nor glow.
    Opaque,
    /// Glass, which reads a copy of the page the opaque surfaces left.
    Glass,
    /// Glow, added onto the page.
    Glow,
}

impl ScenePhase {
    fn of(material: &Material) -> Self {
        match material {
            Material::Glass(_) => Self::Glass,
            Material::Glow => Self::Glow,
            Material::Lit | Material::Unlit | Material::Emissive => Self::Opaque,
        }
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
        if composites_nothing(child) {
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
        self.start_page(pass);
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
        let light_projection = scene.light.filter(|light| light.shadows).and_then(|light| {
            light.shadow_projection(
                meshes
                    .iter()
                    .map(|held| &held.mesh.surface)
                    .filter(|surface| surface.casts_shadow),
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
        let opaque = if shadows {
            ScenePipeline::Shadowed
        } else {
            ScenePipeline::Plain
        };
        let depth = self.renderer.depth_target(page.size);
        let Some(backdrop) = backdrop else {
            self.encode_steps(
                (&page_texture, &depth, shadows),
                &[
                    (opaque, ScenePhase::Opaque),
                    (ScenePipeline::Glow, ScenePhase::Glow),
                ],
                (&draws, None),
                &device,
            );
            return Ok(());
        };
        // Glass reads the page the other surfaces left, so draws in a pass
        // of its own after the copy, over their depth drawn again.
        self.encode_steps(
            (&page_texture, &depth, shadows),
            &[(opaque, ScenePhase::Opaque)],
            (&draws, None),
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
                (ScenePipeline::Depth, ScenePhase::Opaque),
                (ScenePipeline::Glass, ScenePhase::Glass),
                (ScenePipeline::Glow, ScenePhase::Glow),
            ],
            (&draws, Some(&copy)),
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
        steps: &[(ScenePipeline, ScenePhase)],
        (draws, backdrop): (&[PreparedSurface<'_>], Option<&Rc<OffscreenTarget>>),
        device: &wgpu::Device,
    ) {
        let drawn = |phase: ScenePhase| draws.iter().filter(move |draw| draw.phase == phase);
        if steps
            .iter()
            .all(|&(_, phase)| drawn(phase).next().is_none())
        {
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
        let mut render_pass =
            self.recorder
                .begin_depth_pass("Scene Surfaces", &page.view, wgpu::LoadOp::Load, depth);
        render_pass.set_bind_group(2, shadow_group, &[]);
        for &(kind, phase) in steps {
            // A pipeline no surface takes is not asked for, so never compiles.
            let mut phase_draws = drawn(phase).peekable();
            if phase_draws.peek().is_none() {
                continue;
            }
            let Some(pipeline) = renderer.scene_surface_pipeline(kind) else {
                continue;
            };
            render_pass.set_pipeline(pipeline);
            for draw in phase_draws {
                let (x, y, width, height) = draw.scissor;
                render_pass.set_scissor_rect(x, y, width, height);
                draw.bind(&mut render_pass, (device, effects), backdrop);
                render_pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
            }
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
        for draw in draws
            .iter()
            .filter(|draw| draw.held.mesh.surface.casts_shadow)
        {
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
        let scissor = scene_scissor(scene, held.clip, page)?;
        let uniforms = surface_uniforms(
            scene,
            &SurfaceShading {
                transform: held.mesh.surface.transform,
                material: held.mesh.surface.material,
                alpha: held.alpha,
                source_region: source_region(&held.surface, held.mesh.content),
                light_projection,
                backdrop_region,
            },
            (page, eye),
        )?;
        let (buffers, staged) = self
            .renderer
            .scene_pass
            .mesh_buffers((device, &mut *self.recorder), &held.mesh.surface.mesh);
        self.renderer.frame_stats.record_command_stats(staged);
        let uniform = self.recorder.upload_uniform(
            UploadAllocatorId::SceneSurface,
            scene_surface_uniform_spec(),
            device,
            self.renderer.scene_uniform_layout(),
            bytemuck::bytes_of(&uniforms),
        );
        Some(PreparedSurface {
            held,
            phase: ScenePhase::of(&held.mesh.surface.material),
            buffers,
            uniform,
            scissor,
        })
    }
}

struct PreparedSurface<'m> {
    held: &'m PendingMesh,
    phase: ScenePhase,
    buffers: SceneMeshBuffers,
    uniform: crate::frame_graph::UniformUpload,
    scissor: (u32, u32, u32, u32),
}

impl PreparedSurface<'_> {
    /// Binds the surface's texture, uniforms and mesh, and the copy of the
    /// page glass reads: a surface that reads none binds its own texture in
    /// the copy's place.
    fn bind(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (device, effects): (&wgpu::Device, &crate::effect_renderer::EffectRenderer),
        backdrop: Option<&wgpu::BindGroup>,
    ) {
        let texture = self.held.surface.source.texture.get_or_create_bind_group(
            device,
            &effects.effect_texture_bind_group_layout,
            &effects.effect_linear_sampler,
        );
        pass.set_bind_group(0, texture, &[]);
        pass.set_bind_group(3, backdrop.unwrap_or(texture), &[]);
        pass.set_bind_group(1, &self.uniform.bind_group, &[self.uniform.offset]);
        pass.set_vertex_buffer(0, self.buffers.vertices.slice(..));
        pass.set_index_buffer(self.buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
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
