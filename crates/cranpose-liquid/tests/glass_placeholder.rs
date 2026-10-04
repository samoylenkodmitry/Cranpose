use cranpose_liquid::{Glass, GlassDynamics, LiquidColors, LiquidShape};
use cranpose_ui_graphics::{Color, PlaceholderShape, Rect, ShaderPlaceholder};

fn placeholder(glass: &Glass, dynamics: GlassDynamics) -> ShaderPlaceholder {
    let colors = LiquidColors::dark(Color(0.0, 0.48, 1.0, 1.0));
    glass
        .backdrop_effect(&colors, 2.0, dynamics)
        .placeholder()
        .expect("a glass has a placeholder")
}

const WHOLE: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 1.0,
    height: 1.0,
};

/// While its pipelines compile, a glass fills its own shape, so a capsule
/// that clips in its shader does not show a square.
#[test]
fn a_compiling_glass_fills_its_own_shape() {
    let capsule = Glass::regular().shape(LiquidShape::Capsule);
    assert_eq!(
        placeholder(&capsule, GlassDynamics::default()).shape,
        Some(PlaceholderShape {
            bounds: WHOLE,
            corner_radius: f32::MAX,
        })
    );
    let rounded = Glass::regular().shape(LiquidShape::RoundedRect(12.0));
    assert_eq!(
        placeholder(&rounded, GlassDynamics::default()).shape,
        Some(PlaceholderShape {
            bounds: WHOLE,
            corner_radius: 12.0,
        })
    );
}

/// A glass crossfading its tint shows the tint it is passing through: a
/// selected chip blue, the others their own glass.
#[test]
fn a_compiling_glass_shows_the_tint_it_crossfades_through() {
    let glass = Glass::regular().shape(LiquidShape::Capsule);
    let settled = placeholder(&glass, GlassDynamics::default());
    let crossfading = |progress| {
        placeholder(
            &glass,
            GlassDynamics {
                tint_crossfade: Some((Color(0.0, 0.0, 0.0, 0.0), progress)),
                ..GlassDynamics::default()
            },
        )
    };
    assert_eq!(
        crossfading(1.0),
        settled,
        "a finished crossfade is the tint"
    );
    assert_eq!(
        crossfading(0.0).color,
        Color(0.0, 0.0, 0.0, 0.18),
        "from a clear tint the glass still reads as frosted"
    );
}
