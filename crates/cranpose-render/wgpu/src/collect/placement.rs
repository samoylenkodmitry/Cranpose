use cranpose_render_common::{
    graph::{LayerNode, ProjectiveTransform},
    layer_composition::layer_requires_isolation,
};
use cranpose_ui_graphics::{BlendMode, CompositingStrategy, Point, Rect};

use super::{
    AFFINE_TOLERANCE, LayerScene, RasterScale, content_admits_rounded_clip,
    content_takes_rounded_clip, rounded_clip_for_layer,
};
use crate::scene::RunDraw;

pub(crate) fn direct_translation(transform: ProjectiveTransform) -> Option<Point> {
    uniform_scale_translation(transform)
        .filter(|(scale, _)| (scale - 1.0).abs() <= AFFINE_TOLERANCE)
        .map(|(_, translation)| translation)
}

pub(crate) fn uniform_scale_translation(transform: ProjectiveTransform) -> Option<(f32, Point)> {
    let matrix = transform.matrix();
    let scale = matrix[0][0];
    if scale <= 0.0
        || (matrix[1][1] - scale).abs() > AFFINE_TOLERANCE
        || matrix[0][1].abs() > AFFINE_TOLERANCE
        || matrix[1][0].abs() > AFFINE_TOLERANCE
        || matrix[2][0].abs() > AFFINE_TOLERANCE
        || matrix[2][1].abs() > AFFINE_TOLERANCE
        || (matrix[2][2] - 1.0).abs() > AFFINE_TOLERANCE
    {
        return None;
    }
    Some((scale, Point::new(matrix[0][2], matrix[1][2])))
}

/// Whether the layer's own composition needs its content on a separate
/// texture, before any rounded clip or transform is considered.
pub(super) fn child_needs_surface(layer: &LayerNode) -> bool {
    let graphics = &layer.graphics_layer;
    layer.isolation.explicit_offscreen
        || layer.isolation.effect
        || layer.isolation.blend_mode
        || layer.isolation.group_opacity
        || graphics.compositing_strategy == CompositingStrategy::Offscreen
        || layer_requires_isolation(graphics)
}

/// The scale of a `transform` that only scales evenly, turns and moves:
/// affine, with a linear part that is an orthonormal one times the scale.
/// Content drawn at that scale under the rest of the transform resamples
/// nothing.
pub(crate) fn similarity_scale(transform: ProjectiveTransform) -> Option<f32> {
    let [[a, b, _], [c, d, _], perspective] = transform.matrix();
    let near = |value: f32, target: f32| (value - target).abs() <= AFFINE_TOLERANCE;
    let squared = a * a + c * c;
    (near(perspective[0], 0.0)
        && near(perspective[1], 0.0)
        && near(perspective[2], 1.0)
        && squared.is_normal()
        && near((b * b + d * d) / squared, 1.0)
        && near((a * b + c * d) / squared, 0.0))
    .then(|| squared.sqrt())
}

/// The scale a layer whose transform scales by `scale` draws its content at
/// in place, against its parent's, for a layer rasterized at `raster_scale`:
/// one that keeps content's size draws it at the parent's scale, and one
/// that scales at the scale its surface would raster. `None` while its scale
/// moves within a raster it holds, which it keeps compositing.
pub(crate) fn in_place_content_scale(scale: f32, raster_scale: f32) -> Option<f32> {
    if (scale - 1.0).abs() <= AFFINE_TOLERANCE {
        Some(1.0)
    } else {
        ((scale - raster_scale).abs() <= AFFINE_TOLERANCE).then_some(raster_scale)
    }
}

/// Whether an isolated layer can draw its content straight into its parent's
/// pass: its transform only scales evenly at its raster scale, turns and
/// moves it, nothing else about it needs a surface, and its content draws
/// the same under a transform.
pub(super) fn can_draw_in_place(
    layer: &LayerNode,
    transform: ProjectiveTransform,
    raster_scale: f32,
    content: &LayerScene,
) -> bool {
    similarity_scale(transform)
        .and_then(|scale| in_place_content_scale(scale, raster_scale))
        .is_some()
        && !child_needs_surface(layer)
        && layer.backdrop().is_none()
        && rounded_clip_for_layer(layer).is_none()
        && content_draws_in_place(content)
}

/// Whether every part of a layer's content draws the same straight into a
/// transformed pass as into a surface of its own: no backdrop, effect range
/// or shadow that resolves into a texture, nothing that blends other than
/// source-over (it would reach the pixels beneath the layer), no image (its
/// quad's edges are not anti-aliased, where a turned surface filters them),
/// shadow texts and elevation shadows their clips leave whole (a clip turned
/// off the pixel grid is no scissor; a text's own glyphs are cut to its clip
/// before the turn), no rounding a clip cuts into (only a scissor cuts it),
/// and children that draw in place unclipped.
fn content_draws_in_place(content: &LayerScene) -> bool {
    let scene = &content.scene;
    let whole =
        |rect: Rect, clip: Option<Rect>| clip.is_none_or(|clip| clip.intersect(rect) == Some(rect));
    let run_draws_alike = |run: &RunDraw| {
        !run.placement.rounding_cut()
            && run
                .segment_records()
                .all(|segment| segment.blend == BlendMode::SrcOver)
    };
    scene.backdrop_layers.is_empty()
        && scene.effect_layers.is_empty()
        && scene.runs.iter().all(run_draws_alike)
        && scene.shadow_draws.iter().all(|shadow| {
            !shadow.requires_surface()
                && shadow.texts.iter().all(|text| whole(text.rect, text.clip))
        })
        && scene
            .rrect_shadows
            .iter()
            .all(|shadow| whole(shadow.shadow.bounds, shadow.clip))
        && scene.images.is_empty()
        && content
            .children
            .iter()
            .all(|child| child.in_place && child.clip.is_none())
}

#[derive(Clone, Copy)]
pub(super) enum Placement {
    Direct(Point),
    /// Drawn in place under its own clip rounded at the radius, every
    /// shape in the clip's corners taking the clip's coverage.
    DirectRounded(Point, f32),
    Isolated,
}

pub(super) fn child_placement(layer: &LayerNode, raster_scale: RasterScale) -> Placement {
    let Some(translation) = direct_translation(layer.transform_to_parent) else {
        return Placement::Isolated;
    };
    if child_needs_surface(layer) {
        return Placement::Isolated;
    }
    match rounded_clip_for_layer(layer) {
        Some(clip) if !content_admits_rounded_clip(layer, clip, raster_scale) => {
            content_takes_rounded_clip(layer, clip, raster_scale)
                .map_or(Placement::Isolated, |radius| {
                    Placement::DirectRounded(translation, radius)
                })
        }
        _ => Placement::Direct(translation),
    }
}
