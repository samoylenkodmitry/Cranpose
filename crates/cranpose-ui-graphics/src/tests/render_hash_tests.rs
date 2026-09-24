#[test]
fn runtime_shader_overrides_change_the_render_hash() {
    let plain = crate::RuntimeShader::new("// hash-overrides");
    let mut raised = plain.clone();
    raised.set_override("FLAG", 1.0);
    assert_ne!(
        crate::RenderEffect::runtime_shader(plain).render_hash(),
        crate::RenderEffect::runtime_shader(raised).render_hash(),
        "a specialized pipeline renders through different code, so cached output keyed \
         without the overrides would serve the wrong program"
    );
}

use super::*;
use crate::render_effect::TileMode;

#[test]
fn color_render_hash_changes_with_channels() {
    assert_ne!(
        Color(1.0, 0.0, 0.0, 1.0).render_hash(),
        Color(0.0, 1.0, 0.0, 1.0).render_hash()
    );
}

#[test]
fn point_rect_and_corner_radii_render_hash_changes_with_geometry() {
    assert_ne!(
        Point::new(1.0, 2.0).render_hash(),
        Point::new(2.0, 1.0).render_hash()
    );
    assert_ne!(
        Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 20.0,
        }
        .render_hash(),
        Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        }
        .render_hash()
    );
    assert_ne!(
        CornerRadii::uniform(4.0).render_hash(),
        CornerRadii::uniform(6.0).render_hash()
    );
}

#[test]
fn layer_shape_render_hash_tracks_shape_kind_and_radii() {
    assert_ne!(
        LayerShape::Rectangle.render_hash(),
        LayerShape::Rounded(crate::RoundedCornerShape::uniform(8.0)).render_hash()
    );
    assert_ne!(
        LayerShape::Rounded(crate::RoundedCornerShape::uniform(4.0)).render_hash(),
        LayerShape::Rounded(crate::RoundedCornerShape::uniform(8.0)).render_hash()
    );
}

#[test]
fn brush_render_hash_tracks_gradient_structure() {
    let base = Brush::linear_gradient_with_tile_mode(
        vec![Color::RED, Color::BLUE],
        Point::new(0.0, 0.0),
        Point::new(10.0, 10.0),
        TileMode::Clamp,
    );
    let shifted = Brush::linear_gradient_with_tile_mode(
        vec![Color::RED, Color::BLUE],
        Point::new(1.0, 0.0),
        Point::new(10.0, 10.0),
        TileMode::Clamp,
    );

    assert_ne!(base.render_hash(), shifted.render_hash());
}

#[test]
fn color_filter_render_hash_tracks_variant_and_values() {
    assert_ne!(
        ColorFilter::Tint(Color::RED).render_hash(),
        ColorFilter::Modulate(Color::RED).render_hash()
    );
    assert_ne!(
        ColorFilter::Matrix([1.0; 20]).render_hash(),
        ColorFilter::Matrix([0.0; 20]).render_hash()
    );
}

#[test]
fn render_effect_render_hash_tracks_variant_parameters() {
    assert_ne!(
        RenderEffect::blur(4.0).render_hash(),
        RenderEffect::blur(6.0).render_hash()
    );
    assert_ne!(
        RenderEffect::offset(2.0, 1.0).render_hash(),
        RenderEffect::offset(1.0, 2.0).render_hash()
    );
}

#[test]
fn runtime_shader_render_hash_tracks_uniforms() {
    let mut base = RuntimeShader::new("// hash");
    base.set_float(0, 1.0);
    let same = base.clone();
    let mut changed = base.clone();
    changed.set_float(0, 2.0);
    assert_eq!(
        base.render_hash(),
        same.render_hash(),
        "equal source and uniforms hash equally"
    );
    assert_ne!(
        base.render_hash(),
        changed.render_hash(),
        "a uniform change is a pixel change: a surface that bakes this shader \
         must miss the layer cache, and the cache admits repeating keys only, \
         so per-frame uniforms never fill it with stale textures"
    );
}

#[test]
fn runtime_shader_render_hash_tracks_source() {
    let a = RuntimeShader::new("// shader A");
    let b = RuntimeShader::new("// shader B");
    assert_ne!(a.render_hash(), b.render_hash());
}

#[test]
fn draw_primitive_render_hash_tracks_nested_structure() {
    let base = DrawPrimitive::Blend {
        primitive: Box::new(DrawPrimitive::Rect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 8.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        }),
        blend_mode: crate::BlendMode::SrcOver,
    };
    let changed = DrawPrimitive::Blend {
        primitive: Box::new(DrawPrimitive::Rect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 12.0,
                height: 8.0,
            },
            brush: Brush::solid(Color::BLACK),
            stroke: None,
        }),
        blend_mode: crate::BlendMode::SrcOver,
    };
    assert_ne!(base.render_hash(), changed.render_hash());
}

use crate::{Stroke, StrokeCap, StrokeJoin};

fn stroked_rect(stroke: Option<Stroke>) -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 12.0,
            height: 8.0,
        },
        brush: Brush::solid(Color::WHITE),
        stroke,
    }
}

fn arc(
    radius: f32,
    start_angle: f32,
    sweep_angle: f32,
    stroke: Option<Stroke>,
    inner_radius: f32,
) -> DrawPrimitive {
    DrawPrimitive::Arc {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        },
        brush: Brush::solid(Color::WHITE),
        center: Point::new(20.0, 20.0),
        radius,
        start_angle,
        sweep_angle,
        stroke,
        inner_radius,
    }
}

#[test]
fn stroke_render_hash_tracks_width_cap_and_join() {
    let base = Stroke::new(2.0);
    assert_ne!(base.render_hash(), Stroke::new(3.0).render_hash());
    assert_ne!(
        base.render_hash(),
        base.with_cap(StrokeCap::Round).render_hash()
    );
    assert_ne!(
        base.render_hash(),
        base.with_join(StrokeJoin::Bevel).render_hash()
    );
    assert_eq!(base.render_hash(), Stroke::new(2.0).render_hash());
}

#[test]
fn rect_render_hash_separates_fill_from_stroke() {
    let fill = stroked_rect(None);
    let stroked = stroked_rect(Some(Stroke::new(2.0)));
    assert_ne!(fill.render_hash(), stroked.render_hash());
    assert_eq!(
        stroked.render_hash(),
        stroked_rect(Some(Stroke::new(2.0))).render_hash()
    );
}

#[test]
fn rect_render_hash_tracks_every_stroke_field() {
    let base = Stroke::new(2.0);
    let base_hash = stroked_rect(Some(base)).render_hash();
    assert_ne!(
        base_hash,
        stroked_rect(Some(base.with_width(2.5))).render_hash()
    );
    assert_ne!(
        base_hash,
        stroked_rect(Some(base.with_cap(StrokeCap::Square))).render_hash()
    );
    assert_ne!(
        base_hash,
        stroked_rect(Some(base.with_join(StrokeJoin::Round))).render_hash()
    );
}

#[test]
fn round_rect_render_hash_tracks_stroke() {
    let make = |stroke| DrawPrimitive::RoundRect {
        rect: Rect {
            x: 0.0,
            y: 0.0,
            width: 12.0,
            height: 8.0,
        },
        brush: Brush::solid(Color::WHITE),
        radii: CornerRadii::uniform(3.0),
        stroke,
    };
    assert_ne!(
        make(None).render_hash(),
        make(Some(Stroke::new(2.0))).render_hash()
    );
    assert_ne!(
        make(Some(Stroke::new(2.0))).render_hash(),
        make(Some(Stroke::new(2.0).with_join(StrokeJoin::Bevel))).render_hash()
    );
}

#[test]
fn arc_render_hash_tracks_angles_radii_and_stroke() {
    let stroke = Some(Stroke::new(4.0));
    let base = arc(10.0, 0.0, 1.0, stroke, 0.0);
    let base_hash = base.render_hash();

    assert_eq!(base_hash, arc(10.0, 0.0, 1.0, stroke, 0.0).render_hash());
    assert_ne!(base_hash, arc(11.0, 0.0, 1.0, stroke, 0.0).render_hash());
    assert_ne!(base_hash, arc(10.0, 0.5, 1.0, stroke, 0.0).render_hash());
    assert_ne!(base_hash, arc(10.0, 0.0, 1.5, stroke, 0.0).render_hash());
    assert_ne!(base_hash, arc(10.0, 0.0, -1.0, stroke, 0.0).render_hash());
    assert_ne!(base_hash, arc(10.0, 0.0, 1.0, stroke, 4.0).render_hash());
    assert_ne!(base_hash, arc(10.0, 0.0, 1.0, None, 0.0).render_hash());
    assert_ne!(
        base_hash,
        arc(
            10.0,
            0.0,
            1.0,
            Some(Stroke::new(4.0).with_cap(StrokeCap::Round)),
            0.0
        )
        .render_hash()
    );
}

#[test]
fn arc_render_hash_differs_from_rect_with_same_bounds() {
    assert_ne!(
        arc(10.0, 0.0, 1.0, None, 4.0).render_hash(),
        DrawPrimitive::Rect {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            brush: Brush::solid(Color::WHITE),
            stroke: None,
        }
        .render_hash()
    );
}

#[test]
fn a_shaders_declared_support_and_sample_domain_are_part_of_its_hash() {
    let plain = crate::RuntimeShader::new("fn effect_fs() {}");
    let rect = crate::Rect {
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
    };
    let mut supported = plain.clone();
    supported.set_output_support(Some(rect));
    let mut bounded = plain.clone();
    bounded.set_sample_domain(Some(rect));
    assert_ne!(plain.render_hash(), supported.render_hash());
    assert_ne!(plain.render_hash(), bounded.render_hash());
    assert_ne!(supported.render_hash(), bounded.render_hash());
}
