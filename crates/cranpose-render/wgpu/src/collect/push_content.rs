// How a layer's own primitives and draw runs enter the scene, placed by
// the layer's content context.

use cranpose_render_common::{
    graph::{DrawRunNode, PrimitiveEntry, PrimitiveNode, RenderNode},
    primitive_emit::{PrimitiveClipSpace, resolve_primitive_clip},
};
use cranpose_ui_graphics::{Point, RecordLane};

use super::LayerScene;
use crate::{
    pipeline::{TextPlacement, push_draw_primitive, push_text_style_draws},
    scene::{
        CompositorScene, ContentContext, Placement as RunPlacement, RunDraw, ShadowDraw, SnapAnchor,
    },
};

pub(super) fn push_content(out: &mut LayerScene, node: &RenderNode, content: &ContentContext<'_>) {
    match node {
        RenderNode::Primitive(entry) => push_primitive(out, entry, content),
        RenderNode::DrawRun(run) => push_draw_run(out, run, content),
        RenderNode::Layer(_) => {}
    }
}

fn push_primitive(out: &mut LayerScene, entry: &PrimitiveEntry, content: &ContentContext<'_>) {
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        rounding,
        anchor: snap_anchor,
        ..
    } = *content;
    match &entry.node {
        PrimitiveNode::Draw(draw) => {
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
        PrimitiveNode::Text(text) => {
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
    }
}

/// Places a recorded run: every maximal stretch of shape segments becomes
/// one run draw under the layer's placement, and the primitives of the
/// other lane (text, images, shadows) take their own item paths between
/// them, in the order the command drew.
fn push_draw_run(out: &mut LayerScene, run: &DrawRunNode, content: &ContentContext<'_>) {
    if !run.summary.has_shapes && !run.summary.has_others {
        return;
    }
    let ContentContext {
        layer_bounds,
        local_layer,
        visual_clip,
        rounding,
        anchor: snap_anchor,
        ..
    } = *content;
    let placement = RunPlacement::painted(
        Point::new(layer_bounds.x, layer_bounds.y),
        snap_anchor,
        visual_clip,
        local_layer,
    )
    .with_rounding(rounding);
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
