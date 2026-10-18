//! The spaces a scene's portals show, drawn inside their windows in the
//! scene's pass. A window is first marked in the stencil where it shows,
//! one count deeper than the space it lies in; the depth inside it is
//! cleared to the far plane; the surfaces of its space draw where the mark
//! holds, beyond the window's plane, and the windows on them the same way,
//! deeper; and the window's own depth is written back, its mark taken back,
//! so what the scene draws after sees the window as the surface it lies on.

use std::sync::Arc;

use cranpose_ui_graphics::space::{Mat4, Material, Vec3};

use super::{FrameExecutor, PreparedSurface};
use crate::{
    collect::PortalDraw,
    frame_graph::{FrameCommandRecorder, UniformUpload, UploadAllocatorId},
    render::GpuRenderer,
    scene_pass::{
        SceneMeshBuffers, ScenePage, ScenePipeline, SpaceShading, SurfaceShading, VolumeVariant,
        scene_scissor, scene_surface_uniform_spec, surface_uniforms,
    },
};

/// How far toward the eye a window's mark lies off the surface it is on, in
/// the surface's content pixels, so the surface's own depth does not hide
/// it.
const WINDOW_LIFT: f32 = 0.5;

/// A portal's window, ready to draw: its uniforms, the unit square's
/// buffers and the scene's scissor.
pub(super) struct PreparedWindow {
    portal: Arc<PortalDraw>,
    uniform: UniformUpload,
    buffers: SceneMeshBuffers,
    scissor: (u32, u32, u32, u32),
}

impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    /// The windows of the portals `draws` lie behind, and of the portals
    /// those lie behind, each once, in the order the scene met them.
    pub(super) fn prepare_windows(
        &mut self,
        draws: &[PreparedSurface<'_>],
        (scene, page, eye): (&crate::collect::SceneDraw, ScenePage, Vec3),
        device: &wgpu::Device,
    ) -> Vec<PreparedWindow> {
        let mut portals: Vec<Arc<PortalDraw>> = Vec::new();
        for draw in draws {
            let mut portal = draw.held.mesh.portal.as_ref();
            while let Some(shown) = portal {
                if portals.iter().any(|known| known.index == shown.index) {
                    break;
                }
                portals.push(Arc::clone(shown));
                portal = shown.parent.as_ref();
            }
        }
        let scissor = scene_scissor(scene, None, page);
        let Some(scissor) = scissor.filter(|_| !portals.is_empty()) else {
            return Vec::new();
        };
        portals.sort_by_key(|portal| portal.index);
        let window_mesh = Arc::clone(self.renderer.scene_pass.window_mesh());
        let (buffers, staged) = self
            .renderer
            .scene_pass
            .mesh_buffers((device, &mut *self.recorder), &window_mesh);
        self.renderer.frame_stats.record_command_stats(staged);
        let mut windows = Vec::with_capacity(portals.len());
        for portal in portals {
            let lifted =
                Mat4::translation(Vec3::new(0.0, 0.0, -WINDOW_LIFT)).then(&portal.window_to_scene);
            let Some(uniforms) = surface_uniforms(
                scene,
                &SurfaceShading {
                    transform: lifted,
                    material: &Material::Unlit,
                    alpha: 1.0,
                    source_region: [0.0; 4],
                    light_projection: None,
                    backdrop_region: [0.0; 4],
                    space: SpaceShading {
                        light: None,
                        clip: [0.0; 4],
                    },
                },
                (page, eye),
            ) else {
                continue;
            };
            let uniform = self.recorder.upload_uniform(
                UploadAllocatorId::SceneSurface,
                scene_surface_uniform_spec(),
                device,
                self.renderer.scene_uniform_layout(),
                bytemuck::bytes_of(&uniforms),
            );
            windows.push(PreparedWindow {
                portal,
                uniform,
                buffers: buffers.clone(),
                scissor,
            });
        }
        windows
    }
}

/// Draws the spaces the doorways of space `from` lead to, other than the
/// one `came_through`, inside those doorways, `depth` doorways deep from the
/// viewer's space: the doorways ahead into the spaces of portals on its
/// surfaces, and its own doorway back into the space it lies in. `texture`
/// binds in the textures' place, which the doorway draws do not read.
pub(super) fn encode_portals(
    pass: &mut wgpu::RenderPass<'_>,
    renderer: &GpuRenderer,
    (draws, windows): (&[PreparedSurface<'_>], &[PreparedWindow]),
    ((from, came_through), depth): ((Option<usize>, Option<usize>), u32),
    (device, texture): (&wgpu::Device, &wgpu::BindGroup),
) {
    let pipelines = [
        ScenePipeline::WindowMark,
        ScenePipeline::WindowClear,
        ScenePipeline::Portal,
        ScenePipeline::WindowRestore,
    ]
    .map(|kind| renderer.scene_surface_pipeline(kind));
    let [Some(mark), Some(clear), Some(surface), Some(restore)] = pipelines else {
        return;
    };
    let effects = &renderer.effect_renderer;
    for window in windows {
        let index = window.portal.index;
        let parent = window.portal.parent.as_ref().map(|shown| shown.index);
        let into = if parent == from && Some(index) != from {
            Some(index)
        } else if Some(index) == from {
            parent
        } else {
            continue;
        };
        if came_through == Some(index) {
            continue;
        }
        window.draw(pass, (mark, depth), texture);
        window.draw(pass, (clear, depth + 1), texture);
        for draw in draws.iter().filter(|draw| draw.space() == into) {
            let pipeline = match &draw.held.mesh.surface.material {
                Material::Volume(volume) => {
                    match renderer.scene_volume_pipeline(&volume.shader, VolumeVariant::Portal) {
                        Some(pipeline) => pipeline,
                        None => continue,
                    }
                }
                _ => surface,
            };
            pass.set_pipeline(pipeline);
            let (x, y, width, height) = draw.scissor;
            pass.set_scissor_rect(x, y, width, height);
            draw.bind(pass, (device, effects), None);
            pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
        }
        encode_portals(
            pass,
            renderer,
            (draws, windows),
            ((into, Some(index)), depth + 1),
            (device, texture),
        );
        window.draw(pass, (restore, depth + 1), texture);
    }
}

impl PreparedWindow {
    /// Draws the window with `pipeline` against the stencil `reference`.
    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (pipeline, reference): (&wgpu::RenderPipeline, u32),
        texture: &wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_stencil_reference(reference);
        let (x, y, width, height) = self.scissor;
        pass.set_scissor_rect(x, y, width, height);
        pass.set_bind_group(0, texture, &[]);
        pass.set_bind_group(3, texture, &[]);
        pass.set_bind_group(1, &self.uniform.bind_group, &[self.uniform.offset]);
        pass.set_vertex_buffer(0, self.buffers.vertices.slice(..));
        pass.set_index_buffer(self.buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.buffers.index_count, 0, 0..1);
    }
}
