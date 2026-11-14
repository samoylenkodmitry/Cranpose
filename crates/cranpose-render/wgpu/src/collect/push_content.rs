// How a layer's own primitives and draw runs enter the scene, placed by
// the layer's content context.

use cranpose_render_common::{
    graph::{DrawPrimitiveNode, DrawRunNode, TextPrimitiveNode},
    primitive_emit::{PrimitiveClipSpace, resolve_primitive_clip},
    scene_store::{ItemRef, ShapeRef},
};
use cranpose_ui_graphics::{Point, RecordLane};

use super::LayerScene;
use crate::{
    pipeline::{TextPlacement, push_draw_primitive, push_text_style_draws},
    scene::{
        CompositorScene, ContentContext, Placement as RunPlacement, RunDraw, ShadowDraw, SnapAnchor,
    },
};

pub(super) fn push_content(out: &mut LayerScene, node: ItemRef<'_>, content: &ContentContext<'_>) {
    match node {
        ItemRef::Draw(draw) => push_draw(out, draw, content),
        ItemRef::Text(text) => push_text(out, text, content),
        ItemRef::Run(run) => push_draw_run(out, run, content),
        ItemRef::Shape(shape) => push_shapes(out, shape, content),
        ItemRef::Layer(_) | ItemRef::Leaf(_) => {}
    }
}

fn push_draw(out: &mut LayerScene, draw: &DrawPrimitiveNode, content: &ContentContext<'_>) {
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        rounding,
        anchor: snap_anchor,
        ..
    } = *content;
    let counts = scene_counts(&out.scene);
    let clip = resolve_primitive_clip(
        draw.clip,
        layer_bounds,
        local_layer,
        visual_clip,
        PrimitiveClipSpace::Local,
    );
    if clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }
    push_draw_primitive(
        &draw.primitive,
        &ContentContext {
            visual_clip: clip,
            rounding: rounding.filter(|_| clip == visual_clip),
            ..*content
        },
        &mut out.scene,
        None,
    );
    assign_snap_anchor_since(&mut out.scene, counts, snap_anchor);
}

fn push_text(out: &mut LayerScene, text: &TextPrimitiveNode, content: &ContentContext<'_>) {
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        anchor: snap_anchor,
        ..
    } = *content;
    let text_rect = text.rect.translate(layer_bounds.x, layer_bounds.y);
    let text_clip = resolve_primitive_clip(
        text.clip,
        layer_bounds,
        local_layer,
        visual_clip,
        PrimitiveClipSpace::Local,
    );
    if text_clip.is_some_and(|clip| clip.is_empty()) {
        return;
    }
    push_text_style_draws(
        &mut out.scene,
        text,
        TextPlacement {
            bounds: layer_bounds,
            text_rect,
            layer: local_layer,
            clip: text_clip,
            snap_anchor,
        },
    );
}

/// Places a recorded run: every maximal stretch of shape segments becomes
/// one run draw under the layer's placement, and the primitives of the
/// other lane (text, images, shadows) take their own item paths between
/// them, in the order the command drew.
// Where a run of the layer's content draws: at the layer's origin, painted
// by its layer and cut by its visual clip.
fn run_placement(content: &ContentContext<'_>) -> RunPlacement {
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        rounding,
        anchor,
        ..
    } = *content;
    RunPlacement::painted(
        Point::new(layer_bounds.x, layer_bounds.y),
        anchor,
        visual_clip,
        local_layer,
    )
    .with_rounding(rounding)
}

// Places a shape item: its shapes draw as one run of the chunk they sit in.
fn push_shapes(out: &mut LayerScene, shape: ShapeRef<'_>, content: &ContentContext<'_>) {
    if shape.shape_count() > 0 {
        out.scene
            .push_run(RunDraw::of_shapes(shape, run_placement(content)));
    }
}

fn push_draw_run(out: &mut LayerScene, run: &DrawRunNode, content: &ContentContext<'_>) {
    if !run.summary.has_shapes && !run.summary.has_others {
        return;
    }
    let snap_anchor = content.anchor;
    let placement = run_placement(content);
    let recording = &*run.recording;
    // A run of shapes alone draws as one run, read without its segments.
    if run.summary.shapes_only() {
        out.scene.push_run(RunDraw::of(
            recording,
            run.command,
            run.segments.clone(),
            placement,
        ));
        return;
    }
    let counts = scene_counts(&out.scene);
    let mut shapes_from: Option<u32> = None;
    let flush_shapes = |out: &mut LayerScene, end: u32, from: &mut Option<u32>| {
        if let Some(start) = from.take() {
            out.scene.push_run(RunDraw::of(
                recording,
                run.command,
                start..end,
                placement.clone(),
            ));
        }
    };
    for (index, segment) in recording.segments_in(&run.segments).enumerate() {
        let position = run.segments.start + index as u32;
        match segment.lane {
            RecordLane::Shapes => {
                shapes_from.get_or_insert(position);
            }
            RecordLane::Content | RecordLane::GroupStart | RecordLane::GroupEnd => {}
            RecordLane::Others => {
                flush_shapes(out, position, &mut shapes_from);
                for primitive in &recording.others()[segment.range()] {
                    push_draw_primitive(primitive, content, &mut out.scene, None);
                }
            }
        }
    }
    flush_shapes(out, run.segments.end, &mut shapes_from);
    assign_snap_anchor_since(&mut out.scene, counts, snap_anchor);
}

#[derive(Clone, Copy)]
pub(crate) struct SceneCounts {
    images: usize,
    texts: usize,
    shadow_draws: usize,
    rrect_shadows: usize,
    effect_layers: usize,
}

pub(crate) fn scene_counts(scene: &CompositorScene) -> SceneCounts {
    SceneCounts {
        images: scene.images.len(),
        texts: scene.texts.len(),
        shadow_draws: scene.shadow_draws.len(),
        rrect_shadows: scene.rrect_shadows.len(),
        effect_layers: scene.effect_layers.len(),
    }
}

pub(crate) fn assign_snap_anchor_since(
    scene: &mut CompositorScene,
    counts: SceneCounts,
    snap_anchor: Option<SnapAnchor>,
) {
    let Some(anchor) = snap_anchor else {
        return;
    };
    for image in &mut scene.images[counts.images..] {
        image.snap_anchor = Some(anchor);
    }
    for text in &mut scene.texts[counts.texts..] {
        text.snap_anchor = Some(anchor);
    }
    anchor_shadows(&mut scene.shadow_draws[counts.shadow_draws..], anchor);
    assign_shadow_anchor(scene, counts.rrect_shadows, snap_anchor);
    for layer in &mut scene.effect_layers[counts.effect_layers..] {
        layer.snap_anchor = Some(anchor);
    }
}

/// Anchors the layer shadows pushed since `shadows_before` with
/// `snap_anchor`.
pub(super) fn assign_shadow_anchor(
    scene: &mut CompositorScene,
    shadows_before: usize,
    snap_anchor: Option<SnapAnchor>,
) {
    if let Some(anchor) = snap_anchor {
        for shadow in &mut scene.rrect_shadows[shadows_before..] {
            shadow.snap_anchor = Some(anchor);
        }
    }
}

fn anchor_shadows(shadows: &mut [ShadowDraw], anchor: SnapAnchor) {
    for shadow in shadows {
        for run in shadow
            .shapes
            .iter_mut()
            .chain(&mut shadow.post_blur_cutouts)
        {
            run.placement.snap_anchor = Some(anchor);
        }
        for text in &mut shadow.texts {
            text.snap_anchor = Some(anchor);
        }
    }
}
