// Where a scene's pass draws. A scene of one sample in each pixel draws onto
// the page, over what it holds. A multisampled scene cannot: a pass loads
// what an attachment holds only into an attachment as many samples deep,
// while the page holds one. It draws into targets of its own, several
// samples deep, cleared to transparent. Each pass averages them into a
// texture, which a composite then lays over the page where the scene shows.
// Premultiplied colour laid over transparent and then over the page is the
// colour laid over the page. Glow, which adds colour and keeps alpha, adds
// the same way. The targets take the scene's size, within the page's, so a
// scene that scrolls finds the same targets each frame. They lie over the
// part of the scene the page shows.
//
// A scene that covers a page which still holds only its transparent clear
// needs no composite: its one pass clears the samples and averages them
// into the page, which that pass replaces. The samples are a transient
// attachment that the pass drops, so a tiled GPU never writes them out: on
// GLES with `GL_EXT_multisampled_render_to_texture` the driver averages in
// tile memory as it writes the page, and on Vulkan the pass resolves in its
// subpass.

use std::rc::Rc;

use cranpose_ui_graphics::{
    BlendMode,
    space::{Material, Multisample},
};

use super::super::{
    FrameExecutor, LayerPass, composite_z, draw_op_z, filtered_ops_in_range, is_transparent_clear,
};
use crate::{
    collect::SceneDraw,
    draw_pass::{ResolvedComposite, ResolvedCompositeKind, SourceContent},
    effect_renderer::CompositeSampleMode,
    frame_graph::FrameCommandRecorder,
    offscreen::OffscreenTarget,
    scene_pass::{ScenePage, scene_scissor},
};

// The colour, depth and stencil a scene's passes draw into, the page as its
// shaders see it, and the samples they take.
pub(super) struct SceneTarget {
    pub(super) page: ScenePage,
    pub(super) samples: Multisample,
    depth: wgpu::TextureView,
    // Whether the passes leave their depth in `depth`, which a host reads after the frame
    keeps_depth: bool,
    colour: SceneColour,
}

enum SceneColour {
    // The page itself, which the passes draw over.
    Page(Rc<OffscreenTarget>),
    // Colour several samples deep, cleared at each pass, whose average each
    // pass lays over the page at `z`, where `shown`, the pixels of the
    // targets the page shows, lie.
    Sampled {
        view: wgpu::TextureView,
        z: usize,
        shown: (u32, u32, u32, u32),
    },
    // Colour several samples deep, cleared, that the pass averages into the
    // page whole, which held nothing but its transparent clear.
    Resolving {
        view: wgpu::TextureView,
        page: Rc<OffscreenTarget>,
    },
    // The colour of a pass that writes only depth: cleared, and never stored.
    Placeholder(wgpu::TextureView),
}

impl SceneTarget {
    // The z below which a capture of the page sees what the scene's passes
    // have drawn, for a scene whose first surface lies at `z`.
    pub(super) fn drawn_below(&self, z: usize) -> usize {
        match self.colour {
            SceneColour::Page(_) | SceneColour::Resolving { .. } | SceneColour::Placeholder(_) => z,
            SceneColour::Sampled { .. } => z + 1,
        }
    }

    // The depth and stencil the passes draw against, and how they store the depth: a host's
    // view keeps it.
    pub(super) fn depth(&self) -> (&wgpu::TextureView, wgpu::StoreOp) {
        let store = if self.keeps_depth {
            wgpu::StoreOp::Store
        } else {
            wgpu::StoreOp::Discard
        };
        (&self.depth, store)
    }

    // The colour a pass draws into: the page, which keeps what it holds, or
    // the samples, cleared, then averaged into `resolved` and dropped.
    pub(super) fn colour<'a>(
        &'a self,
        resolved: Option<&'a OffscreenTarget>,
    ) -> wgpu::RenderPassColorAttachment<'a> {
        let (view, resolve_target, ops) = match &self.colour {
            SceneColour::Page(page) => (
                &page.view,
                None,
                wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            ),
            SceneColour::Sampled { view, .. } => (
                view,
                resolved.map(|resolved| &resolved.view),
                cleared_and_dropped(),
            ),
            SceneColour::Resolving { view, page } => {
                (view, Some(&page.view), cleared_and_dropped())
            }
            SceneColour::Placeholder(view) => (view, None, cleared_and_dropped()),
        };
        wgpu::RenderPassColorAttachment {
            view,
            resolve_target,
            depth_slice: None,
            ops,
        }
    }

    // The composite that lays `resolved`, a pass's averaged samples, over the
    // page where the scene shows.
    pub(super) fn composite(
        &self,
        resolved: Option<Rc<OffscreenTarget>>,
    ) -> Option<ResolvedComposite> {
        let (&SceneColour::Sampled { z, shown, .. }, Some(source)) = (&self.colour, resolved)
        else {
            return None;
        };
        let (x, y, width, height) = (
            shown.0 as f32,
            shown.1 as f32,
            shown.2 as f32,
            shown.3 as f32,
        );
        Some(ResolvedComposite {
            z_index: z,
            source,
            content: SourceContent::Transient,
            dest: (
                self.page.offset[0] + x,
                self.page.offset[1] + y,
                width,
                height,
            ),
            scissor: None,
            kind: ResolvedCompositeKind::Blit {
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask: None,
                sample_mode: CompositeSampleMode::Nearest,
                source_viewport: Some((x, y, width, height)),
            },
        })
    }
}

impl LayerPass<'_> {
    // Whether `scene`, whose first surface lies at `z`, can average its samples straight into the
    // page: it covers the page whole, the page still holds its transparent clear, and nothing
    // below `z` has drawn or waits to draw. Glass reads the page its scene drew, so a pass of its
    // own follows the first and needs the page the first left. Where a window of the page is
    // drawn again, the rest keeps the frame before's pixels.
    pub(super) fn resolves_into_page(&self, scene: &SceneDraw, z: usize) -> bool {
        let glass = self
            .meshes
            .iter()
            .any(|held| matches!(held.mesh.surface.material, Material::Glass(_)));
        let texture = &self.page.texture;
        let size = (texture.width, texture.height);
        let page = ScenePage {
            scale: self.scale,
            offset: self.page.offset,
            size,
        };
        scene.multisample != Multisample::Off
            && !glass
            && self.page.window.is_none()
            && is_transparent_clear(self.load_op)
            && scene_scissor(scene, None, page) == Some((0, 0, size.0, size.1))
            && self.deferred.iter().all(|op| draw_op_z(op) >= z)
            && self
                .pending
                .iter()
                .all(|composite| composite_z(composite) >= z)
            && self
                .in_place
                .iter()
                .all(|&index| self.layer.children[index].z_index >= z)
            && filtered_ops_in_range(&self.layer.scene.draw_ops, self.drawn_z, z, &self.excluded)
                .is_empty()
    }
}

fn cleared_and_dropped() -> wgpu::Operations<wgpu::Color> {
    wgpu::Operations {
        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        store: wgpu::StoreOp::Discard,
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "3D steps in their files")]
impl<C: FrameCommandRecorder> FrameExecutor<'_, '_, C> {
    // The target the passes of `scene`, whose first surface lies at `z` in
    // `pass`, draw into; `None` when a multisampled scene covers no pixel of
    // the page.
    pub(super) fn scene_target(
        &mut self,
        pass: &LayerPass<'_>,
        scene: &SceneDraw,
        (z, into_page): (usize, bool),
    ) -> Option<SceneTarget> {
        let texture = &pass.page.texture;
        let page = ScenePage {
            scale: pass.scale,
            offset: pass.page.offset,
            size: (texture.width, texture.height),
        };
        let renderer = &mut *self.renderer;
        let samples = scene.multisample;
        if samples == Multisample::Off {
            let (depth, keeps_depth) = renderer.depth_targets.scene(&renderer.device, page.size);
            return Some(SceneTarget {
                page,
                samples,
                depth,
                keeps_depth,
                colour: SceneColour::Page(Rc::clone(texture)),
            });
        }
        if into_page {
            let format = renderer.pipelines.composition_format;
            let (view, depth) = renderer
                .sampled_targets
                .scene(&renderer.device, (page.size, format, samples));
            return Some(SceneTarget {
                page,
                samples,
                depth,
                keeps_depth: false,
                colour: SceneColour::Resolving {
                    view,
                    page: Rc::clone(texture),
                },
            });
        }
        let (left, top, width, height) = scene_scissor(scene, None, page)?;
        // A pixel more than the scene's size holds it at any offset.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "an extent and the scale are lengths, and the page's size clamps what the saturating cast gives"
        )]
        let fit = |extent: f32, page: u32| ((extent * pass.scale).ceil() as u32 + 1).min(page);
        let size = (
            fit(scene.rect.width, page.size.0),
            fit(scene.rect.height, page.size.1),
        );
        let origin = (
            left.min(page.size.0 - size.0),
            top.min(page.size.1 - size.1),
        );
        let format = renderer.pipelines.composition_format;
        let (view, depth) = renderer
            .sampled_targets
            .scene(&renderer.device, (size, format, samples));
        Some(SceneTarget {
            page: ScenePage {
                scale: page.scale,
                offset: [
                    page.offset[0] + origin.0 as f32,
                    page.offset[1] + origin.1 as f32,
                ],
                size,
            },
            samples,
            depth,
            keeps_depth: false,
            colour: SceneColour::Sampled {
                view,
                z,
                shown: (left - origin.0, top - origin.1, width, height),
            },
        })
    }

    // The texture the next pass into `target` averages its samples into;
    // none for a target that is the page.
    pub(super) fn scene_resolved(&mut self, target: &SceneTarget) -> Option<Rc<OffscreenTarget>> {
        match target.colour {
            SceneColour::Page(_) | SceneColour::Resolving { .. } | SceneColour::Placeholder(_) => {
                None
            }
            SceneColour::Sampled { .. } => Some(self.acquire_transient(
                "Scene Samples Averaged",
                target.page.size.0,
                target.page.size.1,
            )),
        }
    }

    // The target of the pass that writes the host's depth for a scene of several samples: the
    // host's view, cleared and stored, behind a colour that the pass clears and drops, with one
    // sample in each pixel. `None` where the host gave no view of the page's size.
    pub(super) fn host_depth_target(&mut self, page: ScenePage) -> Option<SceneTarget> {
        let renderer = &mut *self.renderer;
        let depth = renderer.depth_targets.host(page.size)?;
        let format = renderer.pipelines.composition_format;
        let colour = renderer
            .depth_targets
            .placeholder(&renderer.device, page.size, format);
        Some(SceneTarget {
            page,
            samples: Multisample::Off,
            depth,
            keeps_depth: true,
            colour: SceneColour::Placeholder(colour),
        })
    }
}
