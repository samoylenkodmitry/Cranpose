use cranpose_ui::text::{AnnotatedString, TextMotion};
use cranpose_ui_graphics::Color;

use super::*;

#[test]
fn text_bounds_preserve_logical_snapping_clipping_and_invalid_scale_rejection() {
    let mut draw = TextDraw {
        node_id: 1,
        rect: Rect {
            x: 10.25,
            y: 20.75,
            width: 30.125,
            height: 18.875,
        },
        snap_anchor: Some(SnapAnchor::rigid(Point::new(10.25, 20.75))),
        text: crate::scene::render_string_for(&Rc::new(AnnotatedString::from("bounds"))),
        color: Color::WHITE,
        text_style: Default::default(),
        font_size: 14.0,
        scale: 1.0,
        layout_options: Default::default(),
        clip: None,
    };
    let snapped = Rect {
        x: 10.5,
        y: 21.0,
        ..draw.rect
    };
    let clipped = Rect {
        x: 11.0,
        y: 21.5,
        width: 15.0,
        height: 8.0,
    };
    for motion in [TextMotion::Static, TextMotion::Animated] {
        draw.text_style.paragraph_style.text_motion = Some(motion);
        draw.clip = None;
        assert_eq!(text_draw_bounds(&draw, 2.0), Some(snapped));
        draw.clip = Some(clipped);
        assert_eq!(text_draw_bounds(&draw, 2.0), Some(clipped));
        draw.clip = Some(Rect {
            x: 100.0,
            ..clipped
        });
        assert_eq!(text_draw_bounds(&draw, 2.0), None);
    }
    draw.clip = None;
    draw.snap_anchor = None;
    assert_eq!(text_draw_bounds(&draw, 2.0), Some(draw.rect));
    for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(text_draw_bounds(&draw, invalid), None);
        draw.scale = invalid;
        assert_eq!(text_draw_bounds(&draw, 2.0), None);
        draw.scale = 1.0;
    }
    draw.text = crate::scene::render_string_for(&Rc::new(AnnotatedString::from("")));
    assert_eq!(text_draw_bounds(&draw, 2.0), None);
}
