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
