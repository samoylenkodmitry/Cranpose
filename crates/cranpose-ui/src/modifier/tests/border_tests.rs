use cranpose_ui_graphics::{CornerRadii, DrawPrimitive, DrawScope, Rect, Size, Stroke};

use super::{Brush, Modifier, RoundedCornerShape};
use crate::{
    draw::{DrawCommand, command_draw_scope},
    modifier::Color,
    modifier_nodes::BorderElement,
};

fn drawn(element: &BorderElement, width: f32, height: f32) -> Vec<DrawPrimitive> {
    let DrawCommand::Overlay(draw) = element.command() else {
        panic!("a border draws over the content");
    };
    let mut scope = command_draw_scope(Size::new(width, height));
    draw(&mut scope);
    scope.into_primitives()
}

#[test]
fn a_border_strokes_its_shape_inside_the_bounds() {
    let element = BorderElement::new(
        2.0,
        Brush::solid(Color::BLACK),
        RoundedCornerShape::uniform(8.0),
    );
    assert_eq!(
        drawn(&element, 100.0, 40.0),
        [DrawPrimitive::RoundRect {
            rect: Rect {
                x: 1.0,
                y: 1.0,
                width: 98.0,
                height: 38.0,
            },
            brush: Brush::solid(Color::BLACK),
            radii: CornerRadii::uniform(7.0),
            stroke: Some(Stroke::new(2.0)),
        }],
        "the stroke's outer edge lies on the shape's, its radii taken in by half the width"
    );
}

#[test]
fn a_border_as_thick_as_half_the_node_fills_it() {
    let element = BorderElement::new(
        10.0,
        Brush::solid(Color::BLACK),
        RoundedCornerShape::uniform(0.0),
    );
    assert_eq!(
        drawn(&element, 40.0, 20.0),
        [DrawPrimitive::RoundRect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 20.0,
            },
            brush: Brush::solid(Color::BLACK),
            radii: CornerRadii::uniform(0.0),
            stroke: None,
        }]
    );
    assert!(
        drawn(
            &BorderElement::new(
                0.0,
                Brush::solid(Color::BLACK),
                RoundedCornerShape::uniform(0.0)
            ),
            40.0,
            20.0
        )
        .is_empty()
    );
}

#[test]
fn a_border_takes_a_colour_as_a_solid_brush() {
    let _app_context = crate::render_state::app_context_test_scope();
    let by_colour = Modifier::empty().border(1.0, Color::BLACK, RoundedCornerShape::uniform(4.0));
    let by_brush = Modifier::empty().border(
        1.0,
        Brush::solid(Color::BLACK),
        RoundedCornerShape::uniform(4.0),
    );
    assert!(by_colour == by_brush);
    assert!(
        by_colour != Modifier::empty().border(2.0, Color::BLACK, RoundedCornerShape::uniform(4.0))
    );
}
