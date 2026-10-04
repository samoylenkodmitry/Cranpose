use cranpose_ui_graphics::{
    Color, LiquidGlassRect, LiquidGlassSpec, PlaceholderShape, Rect, ShaderPlaceholder,
    liquid_glass_effect,
};

/// While its pipelines compile, a glass is drawn as its tint over its own
/// rounded shape, at least frosted when the tint is clear.
#[test]
fn a_glass_compiling_shows_its_tint_over_its_rounded_shape() {
    let spec = LiquidGlassSpec {
        corner_radius: 10.0,
        ..LiquidGlassSpec::default()
    };
    let glass = |tint_color| {
        let rect = LiquidGlassRect {
            left: 20.0,
            top: 10.0,
            width: 60.0,
            height: 30.0,
            tint_color,
        };
        liquid_glass_effect(&rect, &spec, 100.0, 50.0)
            .placeholder()
            .expect("a glass has a placeholder")
    };
    let shape = Some(PlaceholderShape {
        bounds: Rect {
            x: 0.2,
            y: 0.2,
            width: 0.6,
            height: 0.6,
        },
        corner_radius: 10.0,
    });
    assert_eq!(
        glass(Color(0.2, 0.4, 0.6, 0.5)),
        ShaderPlaceholder {
            color: Color(0.1, 0.2, 0.3, 0.5),
            shape,
        }
    );
    assert_eq!(
        glass(Color(1.0, 1.0, 1.0, 0.0)).color,
        Color(0.18, 0.18, 0.18, 0.18),
        "a clear glass still reads as frosted"
    );
}
