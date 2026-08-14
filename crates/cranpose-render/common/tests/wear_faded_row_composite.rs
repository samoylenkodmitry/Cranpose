//! What a faded Wear row composites to, checked against the frame it has to
//! match.
//!
//! `ScalingLazyColumn` fades every off-centre row through a `graphicsLayer`,
//! and that is two quantisations, not none:
//!
//! 1. **The colour is eight bits before anything paints with it.**
//!    `androidx.compose.ui.graphics.Color` is a float-shaped API over a packed
//!    ARGB int — `ColorKt.Color(float, float, float, float, ColorSpace)` in the
//!    sRGB space is `coerceIn(0f, 1f)`, `* 255f`, `+ 0.5f`, `f2i`. So the row's
//!    fill is a whole channel value on the way into the layer buffer, with
//!    **ties up**.
//! 2. **The layer composites at a byte alpha, truncated.** HWUI isolates a
//!    `RenderNode` with `canvas->saveLayerAlpha(&bounds, (int)(properties
//!    .getAlpha() * 255))` (`RenderNodeDrawable.cpp`, `setViewProperties`), so
//!    the fraction below the byte is gone before a pixel is blended.
//!
//! Neither is cosmetic and neither is a rounding preference: this file pins
//! both against real Kotlin pixels.
//!
//! # Where the expected values come from
//!
//! `artifacts/sweeps/2026-08-14-192dp-post-gestures/settings-scrolled.kotlin.raw`
//! in the cranorbit port — one fixed frame of the Kotlin app on the Settings
//! screen, scrolled. Two of its rows are faded, and each row's alpha byte is
//! **read off the frame rather than assumed**: `onSurface` is the exact byte
//! triple (223, 246, 255), so a faded `onSurface` pixel's blue channel *is*
//! the composite's alpha byte, with nothing left free. Those two bytes are 224
//! and 171, and every other colour in the row then follows with no fitting at
//! all.
//!
//! The port's own ramp alpha for the same two rows is bracketed by what its
//! composed build drew, and both brackets sit inside the truncation window of
//! the byte Kotlin used — which is how the truncation is established here and
//! not merely quoted: rounding those same brackets gives 172 and would have
//! drawn Kotlin's row a level light.

use cranpose_render_common::layer_composition::{layer_composite_params, local_content_layer_for};
use cranpose_render_common::style_shared::apply_layer_to_color;
use cranpose_ui_graphics::{Color, CompositingStrategy, GraphicsLayer};

/// `surfaceContainer` = `mix(rail, background, 0.55)`. Green lands on an exact
/// half, which is the only reason any of this is visible.
const SURFACE_CONTAINER: [f32; 3] = [9.9, 22.5, 34.2];
/// `outline` = `mix(railAccent, background, 0.3)`.
const OUTLINE: [f32; 3] = [29.4, 77.0, 105.0];
/// `onSurface` — an exact byte triple, so a faded one reads back its own alpha.
const ON_SURFACE: [f32; 3] = [223.0, 246.0, 255.0];

/// A faded row of the Kotlin frame: the alpha byte its `onSurface` pixels
/// report, the window the port's ramp puts its own float alpha in, and what
/// Kotlin drew for each role in that row.
struct MeasuredRow {
    alpha_byte: u8,
    /// Half-open bracket on the port's own float alpha for this row, from the
    /// colours its composed build drew.
    port_alpha: (f32, f32),
    kotlin: [([f32; 3], [u8; 3]); 3],
}

impl MeasuredRow {
    /// The part of the bracket that **truncates** to the byte the frame
    /// reports, which is where the port's ramp alpha has to be.
    fn truncating_to_the_frames_byte(&self) -> (f32, f32) {
        (
            (self.alpha_byte as f32 / 255.0).max(self.port_alpha.0),
            ((self.alpha_byte as f32 + 1.0) / 255.0).min(self.port_alpha.1),
        )
    }

    /// The part that would have **rounded** to it instead.
    fn rounding_to_the_frames_byte(&self) -> (f32, f32) {
        (
            ((self.alpha_byte as f32 - 0.5) / 255.0).max(self.port_alpha.0),
            ((self.alpha_byte as f32 + 0.5) / 255.0).min(self.port_alpha.1),
        )
    }
}

const ROWS: [MeasuredRow; 2] = [
    MeasuredRow {
        alpha_byte: 224,
        port_alpha: (0.879_31, 0.880_95),
        kotlin: [
            (SURFACE_CONTAINER, [9, 20, 30]),
            (OUTLINE, [25, 68, 92]),
            (ON_SURFACE, [196, 216, 224]),
        ],
    },
    MeasuredRow {
        alpha_byte: 171,
        port_alpha: (0.672_764, 0.674_888),
        kotlin: [
            (SURFACE_CONTAINER, [7, 15, 23]),
            (OUTLINE, [19, 52, 70]),
            (ON_SURFACE, [150, 165, 171]),
        ],
    },
];

fn faded_layer(alpha: f32) -> GraphicsLayer {
    GraphicsLayer {
        alpha,
        compositing_strategy: CompositingStrategy::Auto,
        ..GraphicsLayer::default()
    }
}

/// How a colour attachment turns a float channel into the byte it stores.
///
/// **Which of these runs is not the renderer's to choose.** The conversion
/// happens in the hardware that writes the layer buffer, and the specs leave
/// the tie to the implementation, so a renderer that hands the converter an
/// exact half has already lost: the answer depends on the driver. Every test
/// here runs both, and the fix is what makes them agree.
#[derive(Clone, Copy, Debug)]
enum Converter {
    TiesUp,
    TiesToEven,
}

impl Converter {
    const BOTH: [Converter; 2] = [Converter::TiesUp, Converter::TiesToEven];

    fn to_byte(self, channel: f32) -> u32 {
        let scaled = channel.clamp(0.0, 1.0) * 255.0;
        match self {
            Converter::TiesUp => scaled.round() as u32,
            Converter::TiesToEven => round_half_to_even(scaled) as u32,
        }
    }
}

/// The colour the framework hands the layer buffer, before any conversion.
fn painted_into_layer(role_255: [f32; 3], alpha: f32) -> [f32; 3] {
    let content = local_content_layer_for(&faded_layer(alpha));
    let painted = apply_layer_to_color(
        Color(
            role_255[0] / 255.0,
            role_255[1] / 255.0,
            role_255[2] / 255.0,
            1.0,
        ),
        &content,
    );
    [painted.0, painted.1, painted.2]
}

/// One row colour all the way through the renderer's own layer arithmetic.
///
/// Every step is the framework's, not this file's: the content layer and the
/// composite alpha come from `layer_composition`, the painted colour from
/// `style_shared`. The only things modelled here are the two pieces that live
/// in the hardware — the conversion into the 8-bit offscreen and the byte-alpha
/// blend over black that composites it.
fn composited(role_255: [f32; 3], alpha: f32, converter: Converter) -> [u8; 3] {
    let (composite_alpha, _) =
        layer_composite_params(&faded_layer(alpha)).expect("a faded layer must be isolated");
    let alpha_byte = (composite_alpha * 255.0).round() as u32;
    painted_into_layer(role_255, alpha).map(|channel| {
        let in_layer = converter.to_byte(channel);
        // Skia's premultiplied 8-bit multiply: round(x * a / 255).
        ((in_layer * alpha_byte + 127) / 255) as u8
    })
}

/// What carrying the fraction to the framebuffer gives instead: the colour is
/// converted by whatever writes the layer buffer — a converter that breaks
/// ties to even, as a GPU's float-to-unorm does — and the float alpha
/// multiplies it.
fn fraction_carried(role_255: [f32; 3], alpha: f32) -> [u8; 3] {
    role_255.map(|channel| {
        let in_layer = round_half_to_even(channel);
        (in_layer * alpha + 0.5).floor() as u8
    })
}

fn round_half_to_even(value: f32) -> f32 {
    let low = value.floor();
    let fraction = value - low;
    if (fraction - 0.5).abs() < f32::EPSILON {
        if (low as i32) % 2 == 0 {
            low
        } else {
            low + 1.0
        }
    } else {
        value.round()
    }
}

/// `count` alphas spanning `[low, high)`.
fn sweep(low: f32, high: f32) -> impl Iterator<Item = f32> {
    (0..64).map(move |step| low + (high - low) * (step as f32 / 64.0))
}

#[test]
fn a_faded_row_composites_to_the_bytes_the_kotlin_frame_holds() {
    for row in &ROWS {
        let (low, high) = row.truncating_to_the_frames_byte();
        // Anywhere the port's ramp can put this row's alpha, so the result
        // cannot rest on one guessed float.
        for alpha in sweep(low, high) {
            for (role, want) in &row.kotlin {
                for converter in Converter::BOTH {
                    assert_eq!(
                        composited(*role, alpha, converter),
                        *want,
                        "role {role:?} at alpha {alpha} (byte {}) through {converter:?}",
                        row.alpha_byte
                    );
                }
            }
        }
    }
}

#[test]
fn the_layer_buffer_is_handed_a_whole_channel_value_with_no_tie_left_in_it() {
    // The defect this file exists for, stated as the invariant rather than as
    // the pixel: `surfaceContainer`'s green is exactly 22.5/255, and a
    // renderer that passes that on has handed the hardware a coin flip.
    for row in &ROWS {
        let (low, high) = row.truncating_to_the_frames_byte();
        for alpha in sweep(low, high) {
            for (role, _) in &row.kotlin {
                for channel in painted_into_layer(*role, alpha) {
                    let scaled = channel * 255.0;
                    assert!(
                        (scaled - scaled.round()).abs() < 1e-3,
                        "role {role:?} reaches the layer buffer at {scaled}, which is \
                         not a whole channel value — the tie is the hardware's to break"
                    );
                }
            }
        }
    }
}

#[test]
fn the_composite_step_cannot_land_on_a_tie_either() {
    // The second conversion, into the framebuffer, needs no help: the product
    // of a whole channel value and a byte alpha over 255 is never an exact
    // half, because that would need `x * a` to be `255n + 127.5` and it is an
    // integer. Swept over every pair rather than argued, and checked against
    // Skia's integer form at the same time.
    for channel in 0..=255u32 {
        for alpha_byte in 0..=255u32 {
            let product = channel * alpha_byte;
            let exact = product as f32 / 255.0;
            let skia = (product + 127) / 255;
            assert!(
                (exact - exact.floor() - 0.5).abs() > 1.0 / 512.0,
                "{channel} x {alpha_byte} lands on a tie at {exact}"
            );
            for converter in Converter::BOTH {
                assert_eq!(
                    converter.to_byte(exact / 255.0),
                    skia,
                    "{channel} x {alpha_byte} through {converter:?}"
                );
            }
        }
    }
}

#[test]
fn the_composite_alpha_is_the_byte_the_kotlin_frame_reports() {
    for row in &ROWS {
        let (low, high) = row.truncating_to_the_frames_byte();
        assert!(
            low < high,
            "no alpha in the port's bracket for this row truncates to the byte the \
             frame reports ({}), so the port's ramp and the composite rule cannot \
             both be right",
            row.alpha_byte
        );
        for alpha in sweep(low, high) {
            assert_eq!(
                (GraphicsLayer::composite_alpha_8bit(alpha) * 255.0).round() as u8,
                row.alpha_byte,
                "alpha {alpha} did not composite at the frame's byte"
            );
        }
    }

    // What makes the frame able to say *which* rule: on the second row the
    // port's own bracket sits entirely above the window that would round to
    // Kotlin's byte, so rounding cannot have produced it and truncation is the
    // only rule of the two left standing.
    let (low, high) = ROWS[1].rounding_to_the_frames_byte();
    assert!(
        low >= high,
        "rounding also reaches the frame's byte on this row, so it does not \
         discriminate and the truncation above is unsupported"
    );
    assert!(ROWS[1].truncating_to_the_frames_byte().0 < ROWS[1].truncating_to_the_frames_byte().1);
}

#[test]
fn carrying_the_fraction_instead_draws_what_the_composed_build_drew() {
    // The defect this file exists for, stated as the pixels it produced. Each
    // of these is a colour pair the composed build's parity run reported
    // against the same Kotlin frame.
    let cases = [
        // (role, port alpha, what Kotlin drew, what carrying the fraction drew)
        (SURFACE_CONTAINER, 0.880, [9, 20, 30], [9, 19, 30]),
        (OUTLINE, 0.880, [25, 68, 92], [26, 68, 92]),
        (OUTLINE, 0.6735, [19, 52, 70], [20, 52, 71]),
        (ON_SURFACE, 0.6735, [150, 165, 171], [150, 166, 172]),
    ];
    for (role, alpha, kotlin, reported) in cases {
        assert_eq!(
            fraction_carried(role, alpha),
            reported,
            "role {role:?} at alpha {alpha}: the old arithmetic no longer reproduces \
             the pixels that were measured, so this case has stopped testing anything"
        );
        for converter in Converter::BOTH {
            assert_eq!(
                composited(role, alpha, converter),
                kotlin,
                "role {role:?} at alpha {alpha} must now land on Kotlin's byte"
            );
        }
        assert_ne!(
            fraction_carried(role, alpha),
            composited(role, alpha, Converter::TiesToEven),
            "role {role:?} at alpha {alpha} is not discriminating"
        );
    }
}

#[test]
fn a_full_opacity_row_is_left_where_it_was() {
    // Snapping at full opacity must not move anything on screen: the frame
    // buffer holds the same byte either way, and the Wear roles are exact
    // bytes to begin with.
    let layer = GraphicsLayer::default();
    for role in [SURFACE_CONTAINER, OUTLINE, ON_SURFACE] {
        let painted = apply_layer_to_color(
            Color(role[0] / 255.0, role[1] / 255.0, role[2] / 255.0, 1.0),
            &layer,
        );
        assert_eq!(
            [painted.0, painted.1, painted.2].map(|c| (c * 255.0).round() as u8),
            role.map(|c| (c + 0.5).floor() as u8),
        );
    }
    assert!(layer_composite_params(&layer).is_none());
}
