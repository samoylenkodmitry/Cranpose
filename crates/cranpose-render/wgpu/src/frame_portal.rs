//! The spaces a scene's eye sees through doorways, drawn inside them in the
//! scene's pass. A doorway is first marked in the stencil where it shows,
//! one count deeper than the space it opens from; the depth inside it is
//! cleared to the far plane; the surfaces of the space behind it draw where
//! the mark holds, past the doorway's plane and within its opening on
//! screen, and the doorways seen from there the same way, deeper; and the
//! doorway's own depth is written back, its mark taken back, so what the
//! scene draws after sees the doorway as the surface it lies on. Glow in a
//! space draws over the doorways seen from it. Glass draws in the pass
//! after the copy it reads, over the depth of each space drawn again inside
//! its doorway.

use cranpose_ui_graphics::space::{Mat4, Material, Vec3};

use super::{FrameExecutor, PreparedSurface, ScenePhase};
use crate::{
    frame_graph::{FrameCommandRecorder, UniformUpload, UploadAllocatorId},
    render::GpuRenderer,
    scene_pass::{
        SceneMeshBuffers, ScenePage, ScenePipeline, ShadedVariant, SpaceShading, SurfaceShading,
        VolumeVariant, scene_scissor, scene_surface_uniform_spec, surface_uniforms,
    },
};

/// How far toward the eye a doorway's mark lies off the doorway's plane, in
/// units of the frame the doorway lies in, so the surface it opens in does
/// not hide it.
const WINDOW_LIFT: f32 = 0.5;

/// The doorways of a scene, ready to draw, and the buffers of the unit
/// square they draw on.
pub(super) struct PreparedWindows {
    buffers: SceneMeshBuffers,
    windows: Vec<PreparedWindow>,
}

/// A doorway, ready to draw: the seen space behind it and the one it opens
/// from, its uniforms and its opening's scissor.
struct PreparedWindow {
    seen: usize,
    through: usize,
    uniform: UniformUpload,
    scissor: (u32, u32, u32, u32),
}

impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    /// The doorways of the spaces `draws` lie behind, and of the spaces
    /// those are seen through, in the order the eye sees them.
    pub(super) fn prepare_windows(
        &mut self,
        draws: &[PreparedSurface<'_>],
        (scene, page, eye): (&crate::collect::SceneDraw, ScenePage, Vec3),
    ) -> Option<PreparedWindows> {
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
            return None;
        }
        let (buffers, staged) = self
            .renderer
            .scene_pass
            .window_buffers((&self.renderer.device, &mut *self.recorder));
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
                &self.renderer.device,
                self.renderer.scene_uniform_layout(),
                bytemuck::bytes_of(&uniforms),
            );
            windows.push(PreparedWindow {
                seen: index,
                through,
                uniform,
                scissor,
            });
        }
        Some(PreparedWindows { buffers, windows })
    }
}

/// Which surfaces of the spaces seen through doorways a walk through the
/// doorways draws.
#[derive(Clone, Copy)]
pub(super) enum PortalPass {
    /// The surfaces that are neither glass nor glow, and the glow too when
    /// no pass for glass follows.
    Colour { glow: bool },
    /// For the pass after the copy glass reads: the depth of the surfaces
    /// that are not glass, again, then the glass, then the glow.
    Glass,
}

/// What a space's surfaces of one kind draw with inside its window.
#[derive(Clone, Copy)]
enum SpaceStep {
    /// The surfaces that are neither glass nor glow: volumes with the
    /// variant, shaded surfaces with the shaded variant, if any, and the
    /// others, shaded ones whose shader has not built among them, with the
    /// pipeline.
    Opaque(VolumeVariant, Option<ShadedVariant>, ScenePipeline),
    Glass,
    Glow,
}

/// Draws the spaces seen through the doorways of seen space `from`, which
/// shows `depth` doorways deep, inside those doorways, and the spaces seen
/// through theirs: the surfaces `portal_pass` names. `texture` binds in the
/// textures' place, which the doorway draws do not read; glass reads
/// `backdrop`.
pub(super) fn encode_portals(
    pass: &mut wgpu::RenderPass<'_>,
    renderer: &GpuRenderer,
    (draws, windows, portal_pass): (&[PreparedSurface<'_>], &PreparedWindows, PortalPass),
    (from, depth): (usize, u32),
    (device, texture, backdrop): (&wgpu::Device, &wgpu::BindGroup, Option<&wgpu::BindGroup>),
) {
    let pipelines = [
        ScenePipeline::WindowMark,
        ScenePipeline::WindowClear,
        ScenePipeline::WindowRestore,
    ]
    .map(|kind| renderer.scene_surface_pipeline(kind));
    let [Some(mark), Some(clear), Some(restore)] = pipelines else {
        return;
    };
    let colour = SpaceStep::Opaque(
        VolumeVariant::Portal,
        Some(ShadedVariant::Portal),
        ScenePipeline::Portal,
    );
    // What draws before the doorways seen from a space, and what draws over
    // what shows through them.
    let (before, after): (&[SpaceStep], &[SpaceStep]) = match portal_pass {
        PortalPass::Colour { glow: false } => (&[colour], &[]),
        PortalPass::Colour { glow: true } => (&[colour], &[SpaceStep::Glow]),
        PortalPass::Glass => (
            &[
                SpaceStep::Opaque(VolumeVariant::PortalDepth, None, ScenePipeline::PortalDepth),
                SpaceStep::Glass,
            ],
            &[SpaceStep::Glow],
        ),
    };
    let buffers = &windows.buffers;
    for window in windows
        .windows
        .iter()
        .filter(|window| window.through == from)
    {
        window.draw(pass, (mark, depth), (texture, buffers));
        window.draw(pass, (clear, depth + 1), (texture, buffers));
        for &step in before {
            draw_space(pass, renderer, step, (draws, window), (device, backdrop));
        }
        encode_portals(
            pass,
            renderer,
            (draws, windows, portal_pass),
            (window.seen, depth + 1),
            (device, texture, backdrop),
        );
        for &step in after {
            draw_space(pass, renderer, step, (draws, window), (device, backdrop));
        }
        window.draw(pass, (restore, depth + 1), (texture, buffers));
    }
}

/// Draws the surfaces of the space behind `window` that `step` takes,
/// inside the window; a step no surface takes asks for no pipeline.
fn draw_space(
    pass: &mut wgpu::RenderPass<'_>,
    renderer: &GpuRenderer,
    step: SpaceStep,
    (draws, window): (&[PreparedSurface<'_>], &PreparedWindow),
    (device, backdrop): (&wgpu::Device, Option<&wgpu::BindGroup>),
) {
    let (phase, kind) = match step {
        SpaceStep::Opaque(_, _, kind) => (ScenePhase::Portal, kind),
        SpaceStep::Glass => (ScenePhase::PortalGlass, ScenePipeline::PortalGlass),
        SpaceStep::Glow => (ScenePhase::PortalGlow, ScenePipeline::PortalGlow),
    };
    let mut space = draws
        .iter()
        .filter(|draw| draw.seen() == window.seen && draw.phase == phase)
        .peekable();
    if space.peek().is_none() {
        return;
    }
    let Some(fixed) = renderer.scene_surface_pipeline(kind) else {
        return;
    };
    let backdrop = backdrop.filter(|_| matches!(step, SpaceStep::Glass));
    let effects = &renderer.effect_renderer;
    for draw in space {
        let (pipeline, shaded) = match (&draw.held.mesh.surface.material, step) {
            (Material::Volume(volume), SpaceStep::Opaque(variant, _, _)) => {
                match renderer.scene_volume_pipeline(&volume.shader, variant) {
                    Some(pipeline) => (pipeline, false),
                    None => continue,
                }
            }
            (Material::Shaded(_), SpaceStep::Opaque(_, Some(variant), _)) => draw
                .shaded_pipeline(renderer, variant)
                .map_or((fixed, false), |pipeline| (pipeline, true)),
            _ => (fixed, false),
        };
        let Some((x, y, width, height)) = within(draw.scissor, window.scissor) else {
            continue;
        };
        pass.set_pipeline(pipeline);
        pass.set_scissor_rect(x, y, width, height);
        draw.bind(pass, (device, effects), backdrop);
        if shaded {
            draw.bind_shaded(pass);
        }
        pass.draw_indexed(0..draw.buffers.index_count, 0, 0..1);
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
    /// Draws the doorway's square, whose buffers are `buffers`, with
    /// `pipeline` against the stencil `reference`.
    fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (pipeline, reference): (&wgpu::RenderPipeline, u32),
        (texture, buffers): (&wgpu::BindGroup, &SceneMeshBuffers),
    ) {
        pass.set_pipeline(pipeline);
        pass.set_stencil_reference(reference);
        let (x, y, width, height) = self.scissor;
        pass.set_scissor_rect(x, y, width, height);
        pass.set_bind_group(0, texture, &[]);
        pass.set_bind_group(3, texture, &[]);
        pass.set_bind_group(1, &self.uniform.bind_group, &[self.uniform.offset]);
        pass.set_vertex_buffer(0, buffers.vertices.slice(..));
        pass.set_index_buffer(buffers.indices.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..buffers.index_count, 0, 0..1);
    }
}
