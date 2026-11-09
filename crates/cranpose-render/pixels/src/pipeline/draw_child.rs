// How a layer's own primitives, draw runs and text become raster items.

use cranpose_render_common::{
    graph::{DrawPrimitiveNode, TextPrimitiveNode},
    layer_transform::apply_layer_to_rect,
    primitive_emit::{
        DrawPrimitiveSink, ImageDrawParams, PrimitiveClipSpace, ShapeDrawParams, TextDrawParams,
        draw_shape_params_for_primitive, emit_draw_primitive, resolve_primitive_clip,
    },
    scene_store::ItemRef,
    text_raster::GlyphInk,
};
use cranpose_ui_graphics::{BlendMode, DrawPrimitive, GraphicsLayer, Rect};

use super::{
    PrimitiveRenderContext, RasterLayerBounds, TextPlacement,
    layer_shadow::{
        blur_samples, expanded_shape_rect, push_blurred_shape_samples, scale_brush_alpha,
    },
    push_text_style_draws,
};
use crate::scene::{ImageDraw, RasterScene, TextDraw};

pub(super) fn render_draw_child(
    scene: &mut RasterScene,
    child: ItemRef<'_>,
    context: PrimitiveRenderContext<'_>,
) {
    // The items the child pushes snap as its layer does.
    scene.snap_anchor = context.layer_snap_anchor;
    match child {
        ItemRef::Draw(draw) => render_graph_draw(scene, draw, context),
        ItemRef::Text(text) => render_graph_text_item(scene, text, context),
        ItemRef::Run(run) => {
            let rect = context.layer_bounds.raster_rect();
            run.for_each_primitive(|primitive| {
                push_run_primitive(scene, &primitive, rect, context);
            });
        }
        ItemRef::Shape(shape) => {
            let rect = context.layer_bounds.raster_rect();
            for primitive in shape.primitives() {
                push_run_primitive(scene, &primitive, rect, context);
            }
        }
        ItemRef::Layer(_) | ItemRef::Leaf(_) => {}
    }
    scene.snap_anchor = None;
}

// Pushes a primitive of a draw run or a shape item, which the layer's
// visual clip cuts.
fn push_run_primitive(
    scene: &mut RasterScene,
    primitive: &DrawPrimitive,
    rect: Rect,
    context: PrimitiveRenderContext<'_>,
) {
    push_draw_primitive(
        primitive,
        rect,
        context.node_layer,
        context.visual_clip,
        scene,
        None,
        context.motion_context_animated || context.content_offset_translation,
    );
}

fn render_graph_draw(
    scene: &mut RasterScene,
    draw: &DrawPrimitiveNode,
    context: PrimitiveRenderContext<'_>,
) {
    let rect = context.layer_bounds.raster_rect();
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

fn render_graph_text_item(
    scene: &mut RasterScene,
    text: &TextPrimitiveNode,
    context: PrimitiveRenderContext<'_>,
) {
    render_graph_text(
        scene,
        text,
        context.layer_bounds,
        context.node_layer,
        context.visual_clip,
    );
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
                text: std::sync::Arc::clone(params.text.render_string()),
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
