// How a frame draws a shadow that needs a surface: it blurs the shadow's
// casters into a texture, or takes the texture an earlier frame kept, and
// queues its composite.

use std::rc::Rc;

use cranpose_render_common::geometry::blur_reach;
use cranpose_ui_graphics::{BlendMode, Rect, TileMode};

use super::{
    GpuRenderer, anchored_rect_to_device, expand_rect, intersect_device_rects,
    shadow_cache::{
        device_pixel_bounds, shadow_composite_mask, shadow_draw_bounds, shadow_owner, shadow_scene,
        shadow_surface_cache_key, skip_shadow_draws,
    },
};
use crate::{
    draw_pass::{
        PassSegment, PassTarget, ResolvedComposite, ResolvedCompositeKind, SegmentLayout,
        SourceContent,
    },
    effect_renderer::{BlurDest, BlurSpec, CompositeSampleMode},
    frame::{DeviceRect, FrameRecording, Window},
    frame_graph::FrameCommandRecorder,
    geometry::{DevicePixelBounds, translation_stable_anchored_device_pixel_bounds},
    offscreen::OffscreenTarget,
    scene::{RunDraw, ShadowDraw},
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "blurred shadows in their file"
)]
impl GpuRenderer {
    /// Resolves a blurred shadow at `z` into a texture and queues its
    /// composites. The shadow's shapes and texts render into a source the
    /// size of their blur footprint, blur in place and take the post-blur
    /// cutouts; the source is then blitted over the rect the shadow covers.
    /// A surface over the whole footprint lives in the shadow cache, keyed
    /// by its content and device placement, so a still label or a card
    /// scrolled by whole pixels re-blits its cached blur.
    /// The blurred shadow texture and whether the cache held it; a cut
    /// surface renders every frame.
    fn blurred_shadow_source<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        shadow: &ShadowDraw,
        surface: SurfaceBounds,
        pixel_radius: f32,
        root_scale: f32,
    ) -> Option<(Rc<OffscreenTarget>, bool, SourceContent)> {
        let source_device = surface.bounds();
        let key = matches!(surface, SurfaceBounds::Whole(_))
            .then(|| shadow_surface_cache_key(shadow, source_device, pixel_radius, root_scale));
        let owner = shadow_owner(shadow);
        let content = key.map_or(SourceContent::Transient, |key| {
            SourceContent::retained(&key)
        });
        if let Some(target) = key.and_then(|key| self.shadow_surfaces.get(&key, owner)) {
            return Some((Rc::clone(target), true, content));
        }
        // A shadow whose blur compiles is left out until the blur lands.
        if !self.effect_renderer.blur_draws_now(
            &self.device,
            TileMode::Decal,
            (source_device.width, source_device.height),
            pixel_radius,
            pixel_radius,
        ) {
            self.pipelines.note_placeholder(&self.frame_stats);
            return None;
        }
        let shape_only = shadow.texts.is_empty();
        if !shape_only {
            self.frame_stats.record_shadow_text_blur_fallback();
        }
        let source = self.render_shadow_source(
            frame,
            shadow,
            source_device,
            pixel_radius,
            root_scale,
            key.is_some(),
        )?;
        let Some(key) = key else {
            return Some((source, false, content));
        };
        if shape_only {
            self.frame_stats
                .record_shadow_shape_cache_miss(source_device.width, source_device.height);
            self.frame_stats.maybe_print_shadow_shape_cache_miss(
                source_device,
                key.content_hash,
                pixel_radius,
                shadow.shapes.as_ref().map_or(0, RunDraw::record_count) as usize,
                shadow.clip,
            );
        }
        if let Some(replaced) = self.shadow_surfaces.insert(key, Rc::clone(&source), owner)
            && let Ok(target) = Rc::try_unwrap(replaced)
        {
            self.defer_offscreen_release(target);
        }
        Some((source, false, content))
    }

    pub(crate) fn resolve_blurred_shadow<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        shadow: &ShadowDraw,
        z: usize,
        root_scale: f32,
        window: &Window,
        resolved: &mut Vec<ResolvedComposite>,
    ) {
        if !shadow.requires_surface()
            || skip_shadow_draws()
            || !root_scale.is_finite()
            || root_scale <= 0.0
        {
            return;
        }
        let Some(surface) =
            shadow_surface_bounds(shadow, root_scale, window, self.max_texture_dim())
        else {
            return;
        };
        let source_device = surface.bounds();
        let anchor = shadow
            .shapes
            .as_ref()
            .and_then(|run| run.placement.snap_anchor);
        let pixel_radius = shadow.blur_radius * root_scale;
        let Some((source, hit, content)) =
            self.blurred_shadow_source(frame, shadow, surface, pixel_radius, root_scale)
        else {
            return;
        };
        let dest = (
            source_device.x,
            source_device.y,
            source_device.width as f32,
            source_device.height as f32,
        );
        let mut coverage = window
            .cut(DeviceRect {
                x: dest.0,
                y: dest.1,
                width: dest.2,
                height: dest.3,
            })
            .map(|cut| (cut.x, cut.y, cut.width, cut.height));
        if let Some(clip) = shadow.clip {
            coverage = coverage.and_then(|coverage| {
                intersect_device_rects(coverage, anchored_rect_to_device(clip, anchor, root_scale))
            });
        }
        let Some(coverage) = coverage else {
            return;
        };
        if hit && shadow.texts.is_empty() {
            self.frame_stats.record_shadow_shape_cache_hit(
                (coverage.2 as u64).saturating_mul(coverage.3 as u64),
            );
        }
        let rounded_mask = shadow_composite_mask(shadow, anchor, root_scale);
        let downscaled =
            (source.width, source.height) != (source_device.width, source_device.height);
        let (sample_mode, source_viewport) = if downscaled {
            (
                CompositeSampleMode::Linear,
                Some((0.0, 0.0, source.width as f32, source.height as f32)),
            )
        } else {
            (CompositeSampleMode::Nearest, None)
        };
        resolved.push(ResolvedComposite {
            z_index: z,
            source,
            content,
            dest,
            scissor: Some(coverage),
            kind: ResolvedCompositeKind::Blit {
                alpha: 1.0,
                blend_mode: BlendMode::SrcOver,
                rounded_mask,
                sample_mode,
                source_viewport,
            },
        });
    }

    /// Draws a shadow's shapes and texts into a surface covering `bounds`
    /// and blurs it. A wide blur runs at its scratch size and its result
    /// stays there, read bilinearly by the composite; a post-blur cutout
    /// needs the surface's full size, so the blurred result is interpolated
    /// back into it first and the cutout drawn at that size. A retained
    /// result feeds the shadow cache; a transient one is registered with
    /// the frame's transients and released with them. `None` when the
    /// shadow draws nothing.
    fn render_shadow_source<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        shadow: &ShadowDraw,
        bounds: DevicePixelBounds,
        pixel_radius: f32,
        root_scale: f32,
        retained: bool,
    ) -> Option<Rc<OffscreenTarget>> {
        let (width, height) = (bounds.width, bounds.height);
        let (scratch_width, scratch_height) =
            crate::effect_renderer::blur_scratch_size(pixel_radius, pixel_radius, width, height);
        let full_size_result = shadow.post_blur_cutouts.is_some()
            || (scratch_width, scratch_height) == (width, height);
        let (result_width, result_height) = if full_size_result {
            (width, height)
        } else {
            (scratch_width, scratch_height)
        };
        let result = if retained {
            Rc::new(self.acquire_retained_surface(result_width, result_height))
        } else {
            self.shadow_transient(frame, "Shadow Result", result_width, result_height)
        };
        let source = if full_size_result {
            Rc::clone(&result)
        } else {
            self.shadow_transient(frame, "Shadow Source", width, height)
        };
        let offset = [bounds.x, bounds.y];
        let target = PassTarget {
            view: &source.view,
            width,
            height,
        };
        let scene = shadow_scene(
            shadow.shapes.as_ref(),
            &shadow.texts,
            blur_reach(shadow.blur_radius, root_scale),
        );
        let segment = PassSegment::plain(&scene, offset, root_scale);
        let drew = self.encode_pass(
            frame.recorder,
            target,
            (std::slice::from_ref(&segment), SegmentLayout::Stacked),
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            "Shadow Source Pass",
        );
        match drew {
            Ok(true) => {}
            Ok(false) => {
                drop(source);
                if retained && let Ok(target) = Rc::try_unwrap(result) {
                    self.defer_offscreen_release(target);
                }
                return None;
            }
            Err(error) => {
                log::error!("shadow source pass failed: {error}");
                return None;
            }
        }
        if pixel_radius > 0.0 {
            self.blur_shadow_surface(frame, &source, &result, pixel_radius);
        }
        if let Some(cutout_run) = &shadow.post_blur_cutouts {
            let cutouts = shadow_scene(Some(cutout_run), &[], 0.0);
            let segment = PassSegment::plain(&cutouts, offset, root_scale);
            if let Err(error) = self.encode_pass(
                frame.recorder,
                target,
                (std::slice::from_ref(&segment), SegmentLayout::Stacked),
                wgpu::LoadOp::Load,
                "Shadow Cutout Pass",
            ) {
                log::error!("shadow cutout pass failed: {error}");
            }
        }
        Some(result)
    }

    // Blurs `source` into `result`. A result the size of the source that
    // the blur's scratch is smaller than takes the blur interpolated back
    // up from the scratch size.
    fn blur_shadow_surface<C: FrameCommandRecorder>(
        &mut self,
        frame: &mut FrameRecording<'_, C>,
        source: &OffscreenTarget,
        result: &OffscreenTarget,
        pixel_radius: f32,
    ) {
        let (width, height) = (source.width, source.height);
        let (scratch_width, scratch_height) =
            crate::effect_renderer::blur_scratch_size(pixel_radius, pixel_radius, width, height);
        let (result_width, result_height) = (result.width, result.height);
        let full_size_result = (result_width, result_height) == (width, height);
        let scratch_descriptor = self.transient_offscreen_descriptor(
            "Shadow Blur Scratch",
            scratch_width,
            scratch_height,
        );
        let scratch = frame
            .recorder
            .acquire_transient_offscreen(&self.device, scratch_descriptor);
        let blurred = if full_size_result && (scratch_width, scratch_height) != (width, height) {
            Some(self.shadow_transient(frame, "Shadow Blur Result", scratch_width, scratch_height))
        } else {
            None
        };
        let (view, size) = match &blurred {
            Some(blurred) => (&blurred.view, (scratch_width, scratch_height)),
            None => (&result.view, (result_width, result_height)),
        };
        let passes = self.effect_renderer.encode_blur_scissored_ping_pong_passes(
            frame.recorder,
            &self.device,
            source,
            &scratch,
            BlurDest {
                view,
                size,
                scissor: None,
            },
            BlurSpec::uniform(pixel_radius, TileMode::Decal),
        );
        frame.recorder.record_passes(passes);
        self.effect_renderer.record_blur_pass();
        frame
            .recorder
            .release_transient_offscreen(scratch_descriptor, scratch);
        if let Some(blurred) = &blurred {
            self.effect_renderer.encode_upscale_pass(
                frame.recorder,
                &self.device,
                blurred,
                &result.view,
            );
            frame.recorder.record_pass();
        }
    }

    /// A transient surface of a shadow's frame, released with the frame's
    /// transients.
    fn shadow_transient<C: FrameCommandRecorder>(
        &self,
        frame: &mut FrameRecording<'_, C>,
        label: &'static str,
        width: u32,
        height: u32,
    ) -> Rc<OffscreenTarget> {
        let descriptor = self.transient_offscreen_descriptor(label, width, height);
        let target = Rc::new(
            frame
                .recorder
                .acquire_transient_offscreen(&self.device, descriptor),
        );
        frame.transients.push((descriptor, Rc::clone(&target)));
        target
    }
}

// The device pixels a shadow's surface covers.
#[derive(Clone, Copy)]
enum SurfaceBounds {
    // The shadow's whole footprint, which frames keep: it stays the same
    // while the shadow draws the same casters in the same place, whatever
    // part of the page a frame draws.
    Whole(DevicePixelBounds),
    // The part near what the frame draws of a footprint no texture holds,
    // drawn for this frame alone.
    Cut(DevicePixelBounds),
}

impl SurfaceBounds {
    fn bounds(self) -> DevicePixelBounds {
        match self {
            Self::Whole(bounds) | Self::Cut(bounds) => bounds,
        }
    }
}

// The surface of `shadow`: its whole footprint within its clip when a
// texture holds it, a shape-only one placed so that a translation by whole
// pixels keeps it, and otherwise the part of the footprint near what the
// frame draws. `None` when the frame draws none of it.
fn shadow_surface_bounds(
    shadow: &ShadowDraw,
    root_scale: f32,
    window: &Window,
    max_texture_dim: u32,
) -> Option<SurfaceBounds> {
    let margin = blur_reach(shadow.blur_radius, root_scale);
    let footprint = expand_rect(shadow_draw_bounds(shadow)?, margin, margin);
    let visible = match shadow.clip {
        Some(clip) => footprint.intersect(expand_rect(clip, margin, margin))?,
        None => footprint,
    };
    let drawn = window.cut_near(
        DeviceRect::from_logical(visible, root_scale),
        margin * root_scale,
    )?;
    let whole = if shadow.texts.is_empty() {
        let anchor = shadow
            .shapes
            .as_ref()
            .and_then(|run| run.placement.snap_anchor);
        translation_stable_anchored_device_pixel_bounds(
            footprint,
            anchor,
            root_scale,
            max_texture_dim,
        )
    } else {
        device_pixel_bounds(visible, root_scale, max_texture_dim)
    };
    if let Some(whole) = whole {
        return Some(SurfaceBounds::Whole(whole));
    }
    let drawn = Rect {
        x: drawn.x / root_scale,
        y: drawn.y / root_scale,
        width: drawn.width / root_scale,
        height: drawn.height / root_scale,
    };
    device_pixel_bounds(drawn, root_scale, max_texture_dim).map(SurfaceBounds::Cut)
}
