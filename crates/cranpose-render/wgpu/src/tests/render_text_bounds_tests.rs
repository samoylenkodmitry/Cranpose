use cranpose_ui::text::TextMotion;
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
        text: cranpose_ui::text::shared_plain_render_string("bounds"),
        color: Color::WHITE,
        text_style: Default::default(),
        style_hash: 0,
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
        std::sync::Arc::make_mut(&mut draw.text_style)
            .paragraph_style
            .text_motion = Some(motion);
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
    draw.text = cranpose_ui::text::shared_plain_render_string("");
    assert_eq!(text_draw_bounds(&draw, 2.0), None);
}

fn glyph(rect: [f32; 4], uv: [f32; 4]) -> GlyphInstance {
    GlyphInstance {
        rect,
        uv,
        uv_bounds: uv,
        color: [1.0; 4],
    }
}

#[test]
fn a_glyph_quad_cut_by_a_clip_keeps_the_atlas_texels_it_still_shows() {
    let quad = glyph([10.0, 20.0, 30.0, 60.0], [0.5, 0.25, 0.75, 0.75]);
    let cut = quad
        .clipped_to([15.0, 0.0, 100.0, 40.0])
        .expect("the clip keeps part of the quad");
    assert_eq!(cut.rect, [15.0, 20.0, 30.0, 40.0]);
    assert_eq!(
        cut.uv,
        [0.5625, 0.25, 0.75, 0.5],
        "a quarter of the width and half the height go, with their texels"
    );
    assert_eq!(
        cut.uv_bounds, quad.uv_bounds,
        "sampling stays inside the glyph"
    );
    assert_eq!(cut.color, quad.color);
}

#[test]
fn a_glyph_quad_inside_its_clip_is_left_alone_and_one_outside_it_is_dropped() {
    let quad = glyph([10.0, 20.0, 30.0, 60.0], [0.5, 0.25, 0.75, 0.75]);
    assert_eq!(quad.clipped_to([0.0, 0.0, 100.0, 100.0]), Some(quad));
    assert_eq!(quad.clipped_to([30.0, 0.0, 100.0, 100.0]), None);
    assert_eq!(quad.clipped_to([0.0, 60.0, 100.0, 100.0]), None);
}

#[test]
fn glyph_clip_edges_are_the_clip_in_device_pixels() {
    let clip = Rect {
        x: 1.5,
        y: 2.0,
        width: 10.0,
        height: 4.25,
    };
    assert_eq!(glyph_clip_edges(clip, 2.0), [3.0, 4.0, 23.0, 12.5]);
}

fn cut_viewport(transform: SegmentTransform) -> ViewportUniformParams {
    ViewportUniformParams {
        width: 200,
        height: 100,
        offset: [3.0, 5.0],
        transform,
        origin: [0.0, 0.0],
        depth_base: 0.0,
    }
}

#[test]
fn a_text_its_clip_leaves_whole_is_cut_at_its_own_scissor() {
    // Its scissor is its draw rect: a last line's descenders past it are cut
    // as the scissor would cut them, so the text needs no scissor to draw.
    let viewport = cut_viewport(SegmentTransform::IDENTITY);
    for clip in [
        None,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        }),
    ] {
        assert_eq!(
            glyph_cut_edges(clip, (7, 5, 40, 20), viewport, 1.0),
            Some([10.0, 10.0, 50.0, 30.0])
        );
    }
}

#[test]
fn an_unturned_text_is_cut_at_its_scissor_s_pixel_edges() {
    let clip = Rect {
        x: 10.0,
        y: 10.0,
        width: 40.0,
        height: 12.3,
    };
    assert_eq!(
        glyph_cut_edges(
            Some(clip),
            (7, 5, 40, 13),
            cut_viewport(SegmentTransform::IDENTITY),
            1.0
        ),
        Some([10.0, 10.0, 50.0, 23.0]),
        "the scissor's whole pixels, back in the quads' space by the viewport's offset"
    );
}

#[test]
fn a_turned_text_is_cut_at_its_clip_before_the_turn() {
    let clip = Rect {
        x: 10.0,
        y: 10.0,
        width: 40.0,
        height: 12.25,
    };
    let turn = SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [0.0, 0.0]).expect("a quarter turn");
    assert_eq!(
        glyph_cut_edges(Some(clip), (0, 0, 1, 1), cut_viewport(turn), 2.0),
        Some(glyph_clip_edges(clip, 2.0))
    );
}
