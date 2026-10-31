// How a layer's own primitives, draw runs and text become raster items.

use cranpose_render_common::{
    graph::{PrimitiveEntry, PrimitiveNode, RenderNode, TextPrimitiveNode},
    layer_transform::apply_layer_to_rect,
    primitive_emit::{
        DrawPrimitiveSink, ImageDrawParams, PrimitiveClipSpace, ShapeDrawParams, TextDrawParams,
        draw_shape_params_for_primitive, emit_draw_primitive, resolve_primitive_clip,
    },
    text_raster::GlyphInk,
};
use cranpose_ui_graphics::{BlendMode, DrawPrimitive, GraphicsLayer, Rect};

use super::{
    PrimitiveRenderContext, RasterLayerBounds, TextPlacement, assign_snap_anchor_since,
    layer_shadow::{
        blur_samples, expanded_shape_rect, push_blurred_shape_samples, scale_brush_alpha,
    },
    push_text_style_draws,
};
use crate::scene::{ImageDraw, RasterScene, TextDraw};

/// Pushes the items of `child`, a primitive or draw run of a layer.
pub(super) fn render_draw_child(
    scene: &mut RasterScene,
    child: &RenderNode,
    context: PrimitiveRenderContext<'_>,
) {
    match child {
        RenderNode::Primitive(primitive) => render_graph_primitive(scene, primitive, context),
        RenderNode::DrawRun(run) => render_graph_draw_run(scene, run, context),
        RenderNode::Layer(_) => {}
    }
}

fn render_graph_draw_run(
    scene: &mut RasterScene,
    run: &cranpose_render_common::graph::DrawRunNode,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
    let counts_before = scene.counts();
    run.for_each_primitive(|primitive| {
        push_draw_primitive(
            &primitive,
            rect,
            context.node_layer,
            context.visual_clip,
            scene,
            None,
            context.motion_context_animated || context.content_offset_translation,
        );
    });
    assign_snap_anchor_since(scene, counts_before, context.layer_snap_anchor);
}

fn render_graph_primitive(
    scene: &mut RasterScene,
    primitive: &PrimitiveEntry,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
    let counts_before = scene.counts();
    match &primitive.node {
        PrimitiveNode::Draw(draw) => {
            let effective_clip = resolve_primitive_clip(
                draw.clip,
                rect,
                context.node_layer,
                context.visual_clip,
                PrimitiveClipSpace::LayerTransformed,
            );
            if effective_clip.is_some_and(|clip| clip.is_empty()) {
                return;
            }
            push_draw_primitive(
                &draw.primitive,
                rect,
                context.node_layer,
                effective_clip,
                scene,
                None,
                context.motion_context_animated || context.content_offset_translation,
            );
        }
        PrimitiveNode::Text(text) => {
            render_graph_text(
                scene,
                text,
                context.layer_bounds,
                context.node_layer,
                context.visual_clip,
            );
        }
    }
    assign_snap_anchor_since(scene, counts_before, context.layer_snap_anchor);
}

// Out of line for the walk's stack, as `push_layer_shadow_of` tells.
#[inline(never)]
fn render_graph_text(
    scene: &mut RasterScene,
    text: &TextPrimitiveNode,
    layer_bounds: RasterLayerBounds,
    node_layer: &GraphicsLayer,
    visual_clip: Option<Rect>,
) {
    let rect = layer_bounds.raster_rect();
    let text_rect = Rect {
        x: rect.x + text.rect.x,
        y: rect.y + text.rect.y,
        width: text.rect.width,
        height: text.rect.height,
    };
    let text_clip = resolve_primitive_clip(
        text.clip,
        rect,
        node_layer,
        visual_clip,
        PrimitiveClipSpace::LayerTransformed,
    );
    if text_clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }

    let placement = TextPlacement {
        bounds: rect,
        text_rect,
        layer: node_layer,
        clip: text_clip,
    };
    push_text_style_draws(scene, text, placement);
}

pub(crate) fn push_draw_primitive(
    primitive: &DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
    blend_mode: Option<BlendMode>,
    motion_context_animated: bool,
) {
    struct SceneEmitter<'a> {
        scene: &'a mut RasterScene,
    }

    impl DrawPrimitiveSink for SceneEmitter<'_> {
        fn push_shape(&mut self, params: ShapeDrawParams) {
            self.scene.push_shape_params(params);
        }

        fn push_image(&mut self, params: ImageDrawParams) {
            self.scene.push_image(ImageDraw {
                rect: params.rect,
                snap_anchor: None,
                image: params.image,
                alpha: params.alpha,
                color_filter: params.color_filter,
                sampling: params.sampling,
                clip: params.clip,
                blend_mode: params.blend_mode,
                src_rect: params.src_rect,
                transform: None,
            });
        }

        fn push_shadow(
            &mut self,
            shadow_primitive: &cranpose_ui_graphics::ShadowPrimitive,
            layer_bounds: Rect,
            layer: &GraphicsLayer,
            clip: Option<Rect>,
        ) {
            push_shadow_primitive(shadow_primitive, layer_bounds, layer, clip, self.scene);
        }

        fn push_text(&mut self, params: TextDrawParams) {
            self.scene.push_text(TextDraw {
                rect: params.rect,
                snap_anchor: None,
                text: params.text,
                layout: params.layout,
                ink: GlyphInk::Painted,
                color: params.color,
                text_style: params.text_style,
                font_size: params.font_size,
                scale: params.scale,
                clip: params.clip,
            });
        }
    }

    let mut emitter = SceneEmitter { scene };
    emit_draw_primitive(
        primitive,
        layer_bounds,
        layer,
        clip,
        &mut emitter,
        blend_mode,
        motion_context_animated,
    );
}

pub(super) fn push_shadow_primitive(
    shadow_prim: &cranpose_ui_graphics::ShadowPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    clip: Option<Rect>,
    scene: &mut RasterScene,
) {
    match shadow_prim {
        cranpose_ui_graphics::ShadowPrimitive::Drop {
            shape,
            cutout,
            blur_radius,
            blend_mode,
        } => {
            let Some(shape_pair) =
                shape_pair_for_primitive(shape, layer_bounds, layer, *blend_mode)
            else {
                return;
            };
            let cutout_pair = match cutout {
                Some(cutout) => {
                    let Some(pair) =
                        shape_pair_for_primitive(cutout, layer_bounds, layer, BlendMode::DstOut)
                    else {
                        return;
                    };
                    Some(pair)
                }
                None => None,
            };
            let Some(cutout_pair) = cutout_pair else {
                push_blurred_shape_samples(scene, &shape_pair.0, shape_pair.1, clip, *blur_radius);
                return;
            };
            push_cut_shape_samples(scene, (shape_pair, cutout_pair), clip, *blur_radius);
        }
        cranpose_ui_graphics::ShadowPrimitive::Inner {
            fill,
            cutout,
            blur_radius,
            blend_mode,
            clip_rect,
        } => {
            let Some(fill_pair) = shape_pair_for_primitive(fill, layer_bounds, layer, *blend_mode)
            else {
                return;
            };
            let Some(cutout_pair) =
                shape_pair_for_primitive(cutout, layer_bounds, layer, BlendMode::DstOut)
            else {
                return;
            };
            let abs_clip = Rect {
                x: clip_rect.x + layer_bounds.x,
                y: clip_rect.y + layer_bounds.y,
                width: clip_rect.width,
                height: clip_rect.height,
            };
            let transformed_clip = apply_layer_to_rect(abs_clip, layer_bounds, layer);
            let effective_clip = clip.map_or(Some(transformed_clip), |parent_clip| {
                parent_clip.intersect(transformed_clip)
            });
            push_cut_shape_samples(
                scene,
                (fill_pair, cutout_pair),
                effective_clip,
                *blur_radius,
            );
        }
    }
}

fn shape_pair_for_primitive(
    prim: &DrawPrimitive,
    layer_bounds: Rect,
    layer: &GraphicsLayer,
    blend_mode: BlendMode,
) -> Option<(crate::scene::DrawShape, BlendMode)> {
    let params = draw_shape_params_for_primitive(prim, layer_bounds, layer, None, blend_mode)?;
    let blend_mode = params.blend_mode;
    Some((crate::scene::DrawShape::of_params(params), blend_mode))
}

// Pushes `shape` and then `cutout`, which cuts it, under `clip`: widened and
// faded by each blur sample of `blur_radius`, outermost first, or as they
// are where the radius gives no samples.
fn push_cut_shape_samples(
    scene: &mut RasterScene,
    (shape, cutout): (
        (crate::scene::DrawShape, BlendMode),
        (crate::scene::DrawShape, BlendMode),
    ),
    clip: Option<Rect>,
    blur_radius: f32,
) {
    let samples = blur_samples(blur_radius.max(1.0));
    if samples.is_empty() {
        scene.push_shape(
            shape.0.rect,
            shape.0.brush,
            shape.0.outline.rounded(),
            clip,
            shape.1,
        );
        scene.push_shape(
            cutout.0.rect,
            cutout.0.brush,
            cutout.0.outline.rounded(),
            clip,
            cutout.1,
        );
        return;
    }
    for sample in samples.iter().rev() {
        scene.push_shape(
            expanded_shape_rect(&shape.0, sample.expansion),
            scale_brush_alpha(shape.0.brush.clone(), sample.weight),
            shape.0.outline.rounded(),
            clip,
            shape.1,
        );
        scene.push_shape(
            expanded_shape_rect(&cutout.0, sample.expansion),
            scale_brush_alpha(cutout.0.brush.clone(), sample.weight),
            cutout.0.outline.rounded(),
            clip,
            cutout.1,
        );
    }
}
