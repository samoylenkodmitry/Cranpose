//! A scene's mesh surfaces in a layer's render: each surface's content is
//! rendered as an isolated layer's is, then all surfaces of the scene draw
//! in one depth-tested pass over what lies beneath the first of them.

use std::sync::Arc;

use cranpose_ui_graphics::{
    Rect,
    space::{Mat4, Vec3},
};

use super::{DeviceRect, Event, FrameExecutor, LayerPass, SurfaceRender, composites_nothing};
use crate::{
    collect::{ChildLayer, MeshChild},
    frame_graph::{FrameCommandRecorder, UploadAllocatorId},
    scene_pass::{
        SceneMeshBuffers, ScenePage, ScenePipeline, SurfaceShading, scene_scissor,
        scene_surface_uniform_spec, surface_uniforms, view_depth,
    },
};

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
        self.encode_scene_pass(pass, &meshes);
        Ok(())
    }

    fn encode_scene_pass(&mut self, pass: &LayerPass<'_>, meshes: &[PendingMesh]) {
        let Some(first) = meshes.first() else {
            return;
        };
        let scene = Arc::clone(&first.mesh.scene);
        let target = pass.page.pass_target();
        let page = ScenePage {
            scale: pass.scale,
            offset: pass.page.offset,
            size: (target.width, target.height),
        };
        let light_projection = scene.light.filter(|light| light.shadows).and_then(|light| {
            light.shadow_projection(
                meshes
                    .iter()
                    .map(|held| &held.mesh.surface)
                    .filter(|surface| surface.casts_shadow),
            )
        });
        let eye = scene.camera.eye();
        let device = Arc::clone(&self.renderer.device);
        let mut draws = Vec::with_capacity(meshes.len());
        for held in meshes {
            if let Some(draw) = self.prepare_surface(held, (page, eye, light_projection), &device) {
                draws.push(draw);
            }
        }
        if draws.is_empty() {
            return;
        }
        let shadows = light_projection.is_some() && self.draw_shadow_map(&draws, &device);
        let depth = self.renderer.depth_target(page.size);
        let kind = if shadows {
            ScenePipeline::Shadowed
        } else {
            ScenePipeline::Plain
        };
        let Some(pipeline) = self.renderer.scene_surface_pipeline(kind) else {
            return;
        };
        let shadow_group = match (shadows, &self.renderer.scene_pass.shadow_map) {
            (true, Some(map)) => &map.group,
            _ => self.renderer.scene_no_shadow(),
        };
        let effects = &self.renderer.effect_renderer;
        let mut render_pass = self.recorder.begin_depth_pass(
            "Scene Surfaces",
            target.view,
            wgpu::LoadOp::Load,
            &depth,
        );
        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(2, shadow_group, &[]);
        for draw in &draws {
            let (x, y, width, height) = draw.scissor;
            render_pass.set_scissor_rect(x, y, width, height);
            draw.bind(&mut render_pass, (&device, effects));
            render_pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
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
            draw.bind(&mut shadow_pass, (device, effects));
            shadow_pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
        }
        true
    }

    /// One surface's buffers, uniforms and scissor; `None` when nothing of
    /// it can show.
    fn prepare_surface<'m>(
        &mut self,
        held: &'m PendingMesh,
        (page, eye, light_projection): (ScenePage, Vec3, Option<Mat4>),
        device: &wgpu::Device,
    ) -> Option<PreparedSurface<'m>> {
        let scene = &held.mesh.scene;
        let scissor = scene_scissor(scene, held.clip, page)?;
        let uniforms = surface_uniforms(
            scene,
            &SurfaceShading {
                transform: held.mesh.surface.transform,
                lit: held.mesh.surface.lit,
                alpha: held.alpha,
                source_region: source_region(&held.surface, held.mesh.content),
                light_projection,
            },
            (page, eye),
        )?;
        let buffers = self
            .renderer
            .scene_pass
            .mesh_buffers(device, &held.mesh.surface.mesh);
        let uniform = self.recorder.upload_uniform(
            UploadAllocatorId::SceneSurface,
            scene_surface_uniform_spec(),
            device,
            self.renderer.scene_uniform_layout(),
            bytemuck::bytes_of(&uniforms),
        );
        Some(PreparedSurface {
            held,
            buffers,
            uniform,
            scissor,
        })
    }
}

struct PreparedSurface<'m> {
    held: &'m PendingMesh,
    buffers: SceneMeshBuffers,
    uniform: crate::frame_graph::UniformUpload,
    scissor: (u32, u32, u32, u32),
}

impl PreparedSurface<'_> {
    /// Binds the surface's texture, uniforms and mesh.
    fn bind(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (device, effects): (&wgpu::Device, &crate::effect_renderer::EffectRenderer),
    ) {
        let texture = self.held.surface.source.texture.get_or_create_bind_group(
            device,
            &effects.effect_texture_bind_group_layout,
            &effects.effect_linear_sampler,
        );
        pass.set_bind_group(0, texture, &[]);
        pass.set_bind_group(1, &self.uniform.bind_group, &[self.uniform.offset]);
        pass.set_vertex_buffer(0, self.buffers.vertices.slice(..));
        pass.set_index_buffer(self.buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
    }
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
