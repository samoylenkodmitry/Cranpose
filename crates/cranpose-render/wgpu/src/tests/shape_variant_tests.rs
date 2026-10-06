use cranpose_ui_graphics::{
    BRUSH_KIND_LINEAR, BlendMode, FRAGMENT_KIND_FILL, RecordLane, RecordSegment,
};

use super::{RunTier, SegmentClip, ShapeDepth, ShapePipelineKey, ShapeTurns, ShapeVariant};
use crate::ablation::ShapeAblation;

fn fill_segment(gradient: bool, vertex_gradient: bool, interiors: bool) -> RecordSegment {
    RecordSegment {
        lane: RecordLane::Shapes,
        start: 0,
        count: 1,
        blend: BlendMode::SrcOver,
        gradient,
        vertex_gradient,
        brushes: if gradient || vertex_gradient {
            1 << BRUSH_KIND_LINEAR
        } else {
            1
        },
        kinds: 1 << FRAGMENT_KIND_FILL,
        band_class: 0,
        interiors,
        occluders: false,
        bare_interiors: interiors,
    }
}

fn variant(segment: RecordSegment, clipped: bool) -> ShapeVariant {
    let clip = if clipped {
        SegmentClip::Tested
    } else {
        SegmentClip::Untested
    };
    ShapeVariant::of_segment(
        &segment,
        clip,
        ShapeAblation::default(),
        false,
        (true, true),
    )
}

fn key(segment: RecordSegment) -> ShapePipelineKey {
    ShapePipelineKey {
        blend_mode: BlendMode::SrcOver,
        tier: RunTier::Arena,
        variant: variant(segment, false),
        turns: ShapeTurns::None,
        depth: ShapeDepth::Off,
    }
}

#[test]
fn vertex_gradients_draw_on_the_solid_fill_entries_with_the_dither() {
    let dithered = variant(fill_segment(false, true, false), false);
    assert_eq!(
        dithered.entries(true),
        ("vs_record_dithered_fill", "fs_dithered_fill")
    );
    assert_eq!(
        dithered.entries(false),
        ("vs_record_solid", "fs_solid"),
        "turned records read their position and dither from the solid vectors"
    );
    assert_eq!(
        variant(fill_segment(false, true, false), true).entries(true),
        ("vs_record_solid", "fs_solid"),
        "a clipped batch dithers on the solid vectors too"
    );
    assert_eq!(
        variant(fill_segment(false, false, false), false).entries(true),
        ("vs_record_plain_fill", "fs_plain_fill"),
        "a batch without them keeps the plain fill"
    );
    assert_eq!(
        variant(fill_segment(true, false, false), false).entries(true),
        ("vs_record_gradient_fill", "fs_gradient_fill"),
        "a gradient sampled per fragment keeps its own batch"
    );
}

#[test]
fn a_joined_key_adds_the_dither_to_a_solid_batch_and_the_interior_test_to_a_gradient_one() {
    let solid = key(fill_segment(false, false, false));
    let dithered = key(fill_segment(false, true, false));
    assert_ne!(solid, dithered);
    assert_eq!(solid.joined(), dithered);
    assert_eq!(dithered.joined(), dithered);
    let untested = key(fill_segment(true, false, false));
    let tested = key(fill_segment(true, false, true));
    assert_ne!(untested, tested);
    assert_eq!(untested.joined(), tested);
    assert_eq!(tested.joined(), tested);
}

#[test]
fn vertex_shading_requires_an_aligned_placement_and_viewport_without_turns() {
    use cranpose_ui_graphics::Point;

    use crate::{
        geometry::SegmentTransform,
        render::{GpuRenderer, ViewportUniformParams},
        scene::{Placement, SnapAnchor},
    };

    let segment = fill_segment(false, true, false);
    for snapped in [false, true] {
        for offset in [[0.0, 0.0], [3.0, -2.0], [0.13, 0.0]] {
            for turns in [ShapeTurns::None, ShapeTurns::All, ShapeTurns::Mixed] {
                let placement = Placement::at(
                    Point::default(),
                    snapped.then(|| SnapAnchor::rigid(Point::default())),
                    None,
                );
                let viewport = ViewportUniformParams {
                    width: 256,
                    height: 256,
                    offset,
                    transform: SegmentTransform::IDENTITY,
                    origin: [0.0; 2],
                    depth_base: 0.0,
                };
                let key = GpuRenderer::run_pipeline_key(
                    &segment,
                    &placement,
                    RunTier::Arena,
                    ShapeAblation::default(),
                    turns,
                    (false, SegmentClip::Untested),
                    viewport,
                );
                let vertex = snapped && offset[0].fract() == 0.0 && turns == ShapeTurns::None;
                assert_eq!(key.variant.solid, vertex, "{snapped} {offset:?} {turns:?}");
                assert_eq!(key.variant.dither, vertex);
                if !vertex {
                    assert_eq!(
                        key.variant.entries(true),
                        ("vs_record_gradient_fill", "fs_gradient_fill")
                    );
                }
            }
        }
    }
}
