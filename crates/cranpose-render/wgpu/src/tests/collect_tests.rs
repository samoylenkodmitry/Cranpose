use cranpose_ui_graphics::RoundedCornerShape;

use super::*;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn corners(width: f32, height: f32, radius: f32) -> RoundedClipCorners {
    RoundedClipCorners::of(LayerRoundedClip {
        rect: rect(0.0, 0.0, width, height),
        radii: [radius; 4],
    })
}

#[test]
fn content_clear_of_every_corner_square_is_admitted() {
    assert!(corners(200.0, 100.0, 20.0).admits(rect(20.0, 20.0, 160.0, 60.0)));
}

#[test]
fn content_inside_the_corner_circle_is_admitted() {
    assert!(corners(200.0, 100.0, 20.0).admits(rect(14.0, 14.0, 60.0, 60.0)));
}

#[test]
fn content_reaching_the_corner_cut_is_refused() {
    assert!(!corners(200.0, 100.0, 20.0).admits(rect(2.0, 2.0, 60.0, 60.0)));
}

#[test]
fn content_touching_the_edge_between_corners_is_admitted() {
    assert!(corners(200.0, 100.0, 20.0).admits(rect(40.0, 0.0, 100.0, 100.0)));
}

#[test]
fn a_rounded_layer_whose_content_enters_a_corner_isolates() {
    let mut layer = LayerNode {
        local_bounds: rect(0.0, 0.0, 200.0, 100.0),
        graphics_layer: GraphicsLayer {
            clip: true,
            shape: LayerShape::Rounded(RoundedCornerShape::uniform(20.0)),
            ..Default::default()
        },
        ..Default::default()
    };
    layer.children.push(RenderNode::DrawRun(DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![DrawPrimitive::Rect {
            rect: rect(0.0, 0.0, 200.0, 100.0),
            brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
            stroke: None,
        }],
    )));
    assert!(matches!(child_placement(&layer), Placement::Isolated));
    let RenderNode::DrawRun(run) = &mut layer.children[0] else {
        unreachable!()
    };
    *run = DrawRunNode::new(
        PrimitivePhase::BeforeChildren,
        vec![DrawPrimitive::Rect {
            rect: rect(14.0, 14.0, 172.0, 72.0),
            brush: cranpose_ui_graphics::Brush::solid(cranpose_ui_graphics::Color::WHITE),
            stroke: None,
        }],
    );
    assert!(matches!(child_placement(&layer), Placement::Direct(_)));
}

#[test]
fn an_animated_raster_scale_is_never_smaller_and_at_most_one_step_larger() {
    let step = 2f32.powf(1.0 / ANIMATED_RASTER_STEPS_PER_OCTAVE);
    for index in 0..400 {
        let scale = 0.25 + index as f32 * 0.01;
        let raster = animated_raster_scale(scale);
        assert!(raster >= scale, "{scale} rasterized at {raster}");
        assert!(
            raster <= scale * step * 1.0001,
            "{scale} rasterized at {raster}, more than one step up"
        );
    }
    assert_eq!(animated_raster_scale(0.9), animated_raster_scale(0.905));
    assert_eq!(animated_raster_scale(1.0), 1.0);
}

#[test]
fn layer_motion_steps_a_scale_only_while_it_changes_over_the_same_content() {
    let mut motion = LayerMotion::default();
    assert_eq!(
        motion.raster_scale(Some(1), 0.9, 7, true),
        0.9,
        "a layer seen for the first time rasterizes at its own scale"
    );
    motion.end_frame();
    let stepped = motion.raster_scale(Some(1), 0.91, 7, true);
    assert_eq!(stepped, animated_raster_scale(0.91));
    assert!(stepped > 0.91);
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.91, 7, true),
        0.91,
        "a scale that holds for a frame rasterizes exactly again"
    );
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.92, 8, true),
        0.92,
        "new content is drawn afresh anyway, so it rasterizes at its own scale"
    );
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.93, 8, false),
        0.93,
        "a layer the cache cannot hold gains nothing from a stepped scale"
    );
    assert_eq!(motion.raster_scale(None, 0.93, 8, true), 0.93);
    motion.end_frame();
    motion.end_frame();
    assert_eq!(
        motion.raster_scale(Some(1), 0.95, 8, true),
        0.95,
        "a layer missing from the last frame starts over"
    );
}
