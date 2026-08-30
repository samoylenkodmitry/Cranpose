use cranpose_render_common::{
    layer_composition::{layer_composite_params, local_content_layer_for},
    style_shared::apply_layer_to_color,
};
use cranpose_ui_graphics::{Color, CompositingStrategy, GraphicsLayer};

const SURFACE_CONTAINER: [f32; 3] = [9.9, 22.5, 34.2];
const OUTLINE: [f32; 3] = [29.4, 77.0, 105.0];
const ON_SURFACE: [f32; 3] = [223.0, 246.0, 255.0];

struct MeasuredRow {
    alpha_byte: u8,
    port_alpha: (f32, f32),
    kotlin: [([f32; 3], [u8; 3]); 3],
}

impl MeasuredRow {
    fn truncating_to_the_frames_byte(&self) -> (f32, f32) {
        (
            (self.alpha_byte as f32 / 255.0).max(self.port_alpha.0),
            ((self.alpha_byte as f32 + 1.0) / 255.0).min(self.port_alpha.1),
        )
    }

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

fn composited(role_255: [f32; 3], alpha: f32, converter: Converter) -> [u8; 3] {
    let (composite_alpha, _) =
        layer_composite_params(&faded_layer(alpha)).expect("a faded layer must be isolated");
    let alpha_byte = (composite_alpha * 255.0).round() as u32;
    painted_into_layer(role_255, alpha).map(|channel| {
        let in_layer = converter.to_byte(channel);
        ((in_layer * alpha_byte + 127) / 255) as u8
    })
}

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

fn sweep(low: f32, high: f32) -> impl Iterator<Item = f32> {
    (0..64).map(move |step| low + (high - low) * (step as f32 / 64.0))
}

#[test]
fn a_faded_row_composites_to_the_bytes_the_kotlin_frame_holds() {
    for row in &ROWS {
        let (low, high) = row.truncating_to_the_frames_byte();
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
    let cases = [
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
