// Layers a runtime shader draws over their content in the pass beneath,
// and layers drawn by their shader alone.

use std::rc::Rc;

use cranpose_render_common::{
    graph::CachePolicy,
    raster_cache::{LayerRasterCacheKey, RasterScale},
};
use cranpose_ui_graphics::{BlendMode, Point, Rect, RenderEffect, RenderHash, RuntimeShader};

use super::{
    ChildLayer, CompositeSlot, CompositeSource, DeviceRect, FrameExecutor, MAX_SURFACE_PIXELS,
    ResolvedComposite, SourceContent, StandInPixels, SurfaceRender, TRANSPARENT_SOURCE,
    child_composite_support, child_surface_rect, draws_nothing, grid_rounded_mask,
    layer_pixel_rect, placeholder_mask,
};
use crate::{
    effect_renderer::{EffectReads, RoundedCompositeMask},
    frame_graph::FrameCommandRecorder,
    kept_pixels::disk_key,
    layer_cache::Retained,
    offscreen::OffscreenTarget,
};

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
        let place = ShaderPlace {
            z,
            dest,
            visible: support,
            layer_pixel_rect: layer_pixel_rect(child, surface_rect, scale),
            size: (width, height),
            scale,
            snap,
            rounded_mask: grid_rounded_mask(child, snap, scale),
        };
        let (layer_pixel_rect, rounded_mask) = (place.layer_pixel_rect, place.rounded_mask);
        if !self.effect_draws_now(effect, layer_pixel_rect, (width, height)) {
            return Some(self.shader_placeholder(child, shader, &place));
        }
        if child.cache_policy == CachePolicy::Offscreen
            && let Some(kept) = self.kept_shader_output(child, effect, shader, &place)
        {
            return Some(kept);
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

    // The placeholder of a layer its shader alone draws, over the layer's
    // place, or nothing for a shader without one.
    fn shader_placeholder(
        &mut self,
        child: &ChildLayer,
        shader: &RuntimeShader,
        place: &ShaderPlace,
    ) -> Option<ResolvedComposite> {
        let placeholder = shader.placeholder()?;
        let [x, y, width, height] = place.layer_pixel_rect;
        let layer = DeviceRect {
            x: place.dest.x + x,
            y: place.dest.y + y,
            width,
            height,
        };
        let mask = placeholder_mask(placeholder, layer, place.scale).or(place.rounded_mask);
        Some(self.placeholder_fill(
            placeholder.color,
            place.z,
            layer,
            place.visible,
            child.alpha,
            mask,
        ))
    }

    // The output of a layer its shader alone draws, which the app draws
    // offscreen: from the layer cache, or drawn into it from its first
    // frame while the frame's fills allow. Past them the layer shows its
    // placeholder, or nothing, until the next frame fills it. `None` when
    // the output cannot be kept and draws over the page.
    fn kept_shader_output(
        &mut self,
        child: &ChildLayer,
        effect: &RenderEffect,
        shader: &RuntimeShader,
        place: &ShaderPlace,
    ) -> Option<Option<ResolvedComposite>> {
        let (width, height) = place.size;
        let [x, y, w, h] = place.layer_pixel_rect;
        // keyed by what it draws, so layers that draw the same share it
        let key = LayerRasterCacheKey::layer_effect(
            None,
            NO_INPUT,
            effect.render_hash(),
            Rect {
                x,
                y,
                width: w,
                height: h,
            },
            (width, height),
            RasterScale::from_scale(place.scale),
        );
        let kept_slot = |texture: Rc<OffscreenTarget>| {
            CompositeSlot {
                z: place.z,
                source: CompositeSource {
                    texture,
                    content: SourceContent::retained(&key),
                },
                dest: place.dest,
                visible: place.visible,
            }
            .grid_child(child, None, place.snap, place.scale)
        };
        if let Some(cached) = self.renderer.layer_cache.get(&key) {
            self.renderer
                .frame_stats
                .record_layer_cache_hit(&key, width, height);
            return Some(Some(kept_slot(cached.texture)));
        }
        if !self.renderer.layer_cache.fits(width, height) {
            return None;
        }
        // an earlier launch's pixels upload in place of a fill
        let stored_key = disk_key(&key);
        let pixels = u64::from(width) * u64::from(height);
        let format = self.renderer.pipelines.composition_format;
        let stored = self
            .renderer
            .layer_cache
            .disk
            .holds(stored_key, place.size, format);
        if !stored && !self.renderer.layer_cache.fills.allows(pixels) {
            self.renderer
                .pipelines
                .note_deferred_fill(&self.renderer.frame_stats);
            return Some(self.shader_placeholder(child, shader, place));
        }
        self.renderer
            .frame_stats
            .record_layer_cache_miss(&key, width, height);
        let kept = Rc::new(self.renderer.acquire_retained_surface(width, height));
        let cache = &mut self.renderer.layer_cache;
        if stored && cache.disk.upload(&self.renderer.queue, stored_key, &kept) {
            let retained = Retained::surface(Rc::clone(&kept), StandInPixels::Final);
            cache.insert(key, retained, None);
            return Some(Some(kept_slot(kept)));
        }
        let source = self
            .renderer
            .transparent_source(self.recorder, width, height);
        let mark = self.renderer.stand_in_mark();
        self.apply_effect_into(
            &source,
            &kept,
            effect,
            place.layer_pixel_rect,
            EffectReads::default(),
        )
        .ok()?;
        self.renderer.layer_cache.fills.spend(pixels);
        let held = self.pixels_since(mark);
        let cache = &mut self.renderer.layer_cache;
        cache.insert(key, Retained::surface(Rc::clone(&kept), held), None);
        if held == StandInPixels::Final {
            cache.disk.keep(stored_key, Rc::clone(&kept));
        }
        Some(Some(kept_slot(kept)))
    }
}

// Where a layer that its shader alone draws goes.
struct ShaderPlace {
    z: usize,
    dest: DeviceRect,
    visible: DeviceRect,
    layer_pixel_rect: [f32; 4],
    size: (u32, u32),
    scale: f32,
    snap: Point,
    rounded_mask: Option<RoundedCompositeMask>,
}

// The input of a layer its shader alone draws: nothing.
const NO_INPUT: u64 = 0;
