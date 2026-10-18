//! The spaces a scene's eye sees through doorways, drawn inside them in the
//! scene's pass. A doorway is first marked in the stencil where it shows,
//! one count deeper than the space it opens from; the depth inside it is
//! cleared to the far plane; the surfaces of the space behind it draw where
//! the mark holds, past the doorway's plane and within its opening on
//! screen, and the doorways seen from there the same way, deeper; and the
//! doorway's own depth is written back, its mark taken back, so what the
//! scene draws after sees the doorway as the surface it lies on.

use cranpose_ui_graphics::space::{Mat4, Material, Vec3};

use super::{FrameExecutor, PreparedSurface};
use crate::{
    frame_graph::{FrameCommandRecorder, UniformUpload, UploadAllocatorId},
    render::GpuRenderer,
    scene_pass::{
        SceneMeshBuffers, ScenePage, ScenePipeline, SpaceShading, SurfaceShading, VolumeVariant,
        scene_scissor, scene_surface_uniform_spec, surface_uniforms,
    },
};

/// How far toward the eye a doorway's mark lies off the doorway's plane, in
/// units of the frame the doorway lies in, so the surface it opens in does
/// not hide it.
const WINDOW_LIFT: f32 = 0.5;

/// A doorway, ready to draw: the seen space behind it and the one it opens
/// from, its uniforms, the unit square's buffers and its opening's scissor.
pub(super) struct PreparedWindow {
    seen: usize,
    through: usize,
    uniform: UniformUpload,
    buffers: SceneMeshBuffers,
    scissor: (u32, u32, u32, u32),
}

impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    /// The doorways of the spaces `draws` lie behind, and of the spaces
    /// those are seen through, in the order the eye sees them.
    pub(super) fn prepare_windows(
        &mut self,
        draws: &[PreparedSurface<'_>],
        (scene, page, eye): (&crate::collect::SceneDraw, ScenePage, Vec3),
        device: &wgpu::Device,
    ) -> Vec<PreparedWindow> {
        // A doorway draws when a surface lies behind it, or behind one seen
        // through it; the eye sees parents before what lies behind them.
        let mut needed = vec![false; scene.seen.len()];
        for draw in draws {
            needed[draw.seen()] = true;
        }
        for (index, seen) in scene.seen.iter().enumerate().rev() {
            if let Some(through) = seen.through.filter(|_| needed[index]) {
                needed[through] = true;
            }
        }
        if !needed.iter().skip(1).any(|needed| *needed) {
            return Vec::new();
        }
        let window_mesh = std::sync::Arc::clone(self.renderer.scene_pass.window_mesh());
        let (buffers, staged) = self
            .renderer
            .scene_pass
            .mesh_buffers((device, &mut *self.recorder), &window_mesh);
        self.renderer.frame_stats.record_command_stats(staged);
        let mut windows = Vec::new();
        for (index, seen) in scene.seen.iter().enumerate() {
            let (Some(through), Some(door), true) = (seen.through, seen.door, needed[index]) else {
                continue;
            };
            let opening = door.opening.translate(scene.rect.x, scene.rect.y);
            let Some(scissor) = scene_scissor(scene, Some(opening), page) else {
                continue;
            };
            let lifted =
                Mat4::translation(Vec3::new(0.0, 0.0, -WINDOW_LIFT * door.away)).then(&door.quad);
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
                seen: index,
                through,
                uniform,
                buffers: buffers.clone(),
                scissor,
            });
        }
        windows
    }
}

/// Draws the spaces seen through the doorways of seen space `from`, which
/// shows `depth` doorways deep, inside those doorways, and the spaces seen
/// through theirs. `texture` binds in the textures' place, which the
/// doorway draws do not read.
pub(super) fn encode_portals(
    pass: &mut wgpu::RenderPass<'_>,
    renderer: &GpuRenderer,
    (draws, windows): (&[PreparedSurface<'_>], &[PreparedWindow]),
    (from, depth): (usize, u32),
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
    for window in windows.iter().filter(|window| window.through == from) {
        window.draw(pass, (mark, depth), texture);
        window.draw(pass, (clear, depth + 1), texture);
        for draw in draws.iter().filter(|draw| draw.seen() == window.seen) {
            let pipeline = match &draw.held.mesh.surface.material {
                Material::Volume(volume) => {
                    match renderer.scene_volume_pipeline(&volume.shader, VolumeVariant::Portal) {
                        Some(pipeline) => pipeline,
                        None => continue,
                    }
                }
                _ => surface,
            };
            let Some((x, y, width, height)) = within(draw.scissor, window.scissor) else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_scissor_rect(x, y, width, height);
            draw.bind(pass, (device, effects), None);
            pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
        }
        encode_portals(
            pass,
            renderer,
            (draws, windows),
            (window.seen, depth + 1),
            (device, texture),
        );
        window.draw(pass, (restore, depth + 1), texture);
    }
}

/// The part of scissor `rect` inside scissor `bounds`; `None` when none.
fn within(
    (x, y, width, height): (u32, u32, u32, u32),
    (bx, by, bwidth, bheight): (u32, u32, u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let (left, top) = (x.max(bx), y.max(by));
    let right = (x + width).min(bx + bwidth);
    let bottom = (y + height).min(by + bheight);
    (right > left && bottom > top).then_some((left, top, right - left, bottom - top))
}

impl PreparedWindow {
    /// Draws the doorway's square with `pipeline` against the stencil
    /// `reference`.
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
