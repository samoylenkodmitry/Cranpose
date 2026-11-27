// Layers a runtime shader draws over their content in the pass beneath,
// and layers drawn by their shader alone.

use cranpose_ui_graphics::{BlendMode, Point, RenderEffect, RuntimeShader};

use super::{
    ChildLayer, CompositeSlot, CompositeSource, DeviceRect, FrameExecutor, MAX_SURFACE_PIXELS,
    ResolvedComposite, SourceContent, SurfaceRender, TRANSPARENT_SOURCE, child_composite_support,
    child_surface_rect, draws_nothing, grid_rounded_mask, layer_pixel_rect, placeholder_mask,
};
use crate::frame_graph::FrameCommandRecorder;

impl<'r, 'c, C: FrameCommandRecorder> FrameExecutor<'r, 'c, C> {
    // Whether a child's runtime shader can draw in the final pass over the
    // child's content: the shader must apply the child's clip and alpha
    // itself unless the child has neither.
    fn shader_tail_composites(&mut self, child: &ChildLayer, shader: &RuntimeShader) -> bool {
        let plain = child.alpha >= 1.0 && child.rounded_clip.is_none();
        shader.substrates().is_empty()
            && child.blend_mode == BlendMode::SrcOver
            && (plain || shader.batched_source())
            && self
                .renderer
                .effect_renderer
                .shader_cache
                .position_independent(shader)
    }

    /// The child's shader drawn in the final pass over its surface: `None`
    /// when it does not draw that way, no composite while its pipeline
    /// compiles.
    pub(super) fn shader_tail_over_surface(
        &mut self,
        child: &ChildLayer,
        surface: &SurfaceRender,
        snap: Point,
        z: usize,
        scale: f32,
        visible: DeviceRect,
    ) -> Option<Option<ResolvedComposite>> {
        let effect = child.effect.as_ref()?;
        let RenderEffect::Shader { shader } = effect else {
            return None;
        };
        let dest = surface.grid_dest?;
        let visible = dest.intersect(visible)?;
        if !self.shader_tail_composites(child, shader) {
            return None;
        }
        let layer_pixel_rect = layer_pixel_rect(child, surface.rect, surface.scale);
        let source_size = (surface.source.texture.width, surface.source.texture.height);
        if !self.effect_draws_now(effect, layer_pixel_rect, source_size) {
            return Some(None);
        }
        let slot = CompositeSlot {
            z,
            source: surface.source.clone(),
            dest,
            visible,
        };
        Some(Some(slot.shader_tail(
            child,
            shader,
            layer_pixel_rect,
            grid_rounded_mask(child, snap, scale),
        )))
    }

    /// A child that draws only its shader, composited without a surface of
    /// its own: `None` when the child is not one, and no composite while its
    /// shader compiles without a placeholder.
    pub(super) fn shader_only_child(
        &mut self,
        child: &ChildLayer,
        z: usize,
        scale: f32,
        visible: DeviceRect,
        translation: Option<Point>,
        snap: Point,
    ) -> Option<Option<ResolvedComposite>> {
        let translation = translation?;
        let effect = child.effect.as_ref()?;
        let RenderEffect::Shader { shader } = effect else {
            return None;
        };
        let support =
            child_composite_support(child, shader.output_support(), snap, scale, visible)?;
        if !draws_nothing(&child.content) || !self.shader_tail_composites(child, shader) {
            return None;
        }
        let surface_logical = child_surface_rect(child, scale)?;
        let surface_rect = DeviceRect::from_logical(surface_logical, scale).snap_out();
        let (width, height) = surface_rect.pixel_size();
        if u64::from(width) * u64::from(height) > MAX_SURFACE_PIXELS {
            return None;
        }
        let dest = DeviceRect {
            x: (surface_rect.x + translation.x * scale).round(),
            y: (surface_rect.y + translation.y * scale).round(),
            width: surface_rect.width,
            height: surface_rect.height,
        };
        let layer_pixel_rect = layer_pixel_rect(child, surface_rect, scale);
        let rounded_mask = grid_rounded_mask(child, snap, scale);
        if !self.effect_draws_now(effect, layer_pixel_rect, (width, height)) {
            let [x, y, width, height] = layer_pixel_rect;
            let layer = DeviceRect {
                x: dest.x + x,
                y: dest.y + y,
                width,
                height,
            };
            return Some(shader.placeholder().map(|placeholder| {
                let mask = placeholder_mask(placeholder, layer, scale).or(rounded_mask);
                self.placeholder_fill(placeholder.color, z, layer, support, child.alpha, mask)
            }));
        }
        let source = self
            .renderer
            .transparent_source(self.recorder, width, height);
        let slot = CompositeSlot {
            z,
            source: CompositeSource {
                texture: source,
                content: SourceContent::retained(&TRANSPARENT_SOURCE),
            },
            dest,
            visible: support,
        };
        Some(Some(slot.shader_tail(
            child,
            shader,
            layer_pixel_rect,
            rounded_mask,
        )))
    }
}
