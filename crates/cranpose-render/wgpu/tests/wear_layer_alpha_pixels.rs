//! What a faded Wear row's pixels actually come out as.
//!
//! `ScalingLazyColumn` fades every off-centre row through a `graphicsLayer`,
//! and Skia renders a layer below full opacity into an **8-bit** offscreen and
//! composites that. So the row's colours are quantised to whole channel values
//! *before* the alpha multiplies them. Folding the alpha into a float colour
//! instead lands up to one level away on every pixel of every faded row —
//! measured on the Kotlin build, that single level covered thirty per cent of
//! the scrolled Settings screen.
//!
//! This runs the whole path an app runs — compose, `AppShell::update`, scene
//! build, wgpu, framebuffer readback — and reads the composited pixels back.
//!
//! **The answer is that this renderer already gets it right**, and these tests
//! exist to keep it that way rather than to drive a fix. The reason it is right
//! is structural, not lucky: `CompositingStrategy::Auto` with alpha below one
//! raises `SurfaceRequirement::GroupOpacity` (`surface_plan.rs`), which
//! allocates a real offscreen target from the 8-bit `OffscreenPool`, and
//! `layer_for_content` draws the row into it at alpha 1 and moves the alpha to
//! the composite. The quantisation therefore happens where Skia's does — on the
//! way into the layer buffer — instead of being modelled.
//!
//! What would break it is the `ModulateAlpha` strategy, which folds the alpha
//! into each primitive in float and is one call away
//! (`WearScalingLazyColumnSpec::compositing_strategy`). The discriminating test
//! below is what would notice.

mod support;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::round_scaling_list::CentreAnchor;
use cranpose_ui::widgets::wear::{
    rememberWearScalingListState, WearScalingLazyColumn, WearScalingLazyColumnSpec,
};
use cranpose_ui::widgets::{Box, BoxSpec, Spacer};
use cranpose_ui::{Color, Modifier, Size};
use std::cell::Cell;

/// A 454x454 watch, one framebuffer pixel per layout point.
const SIZE: u32 = 454;
/// `MIN_HEIGHT` for a Wear row.
const ROW: f32 = 52.0;
const ROWS: usize = 6;

/// `primaryContainer` in the theme the widget spec measured — the fill of a
/// checked `SwitchButton`'s capsule. Exact 8-bit values, as every colour in the
/// Wear palette is.
const PALETTE_FILL: (f32, f32, f32) = (15.0, 54.0, 78.0);

/// A fill deliberately BETWEEN 8-bit values, which is the only way to tell the
/// two candidate models apart.
///
/// Wear's own roles are all exact bytes, so quantising them is a no-op and both
/// models agree — the palette case below can confirm the composite is sane but
/// can never prove which rule produced it. A theme that lerps, as the Kotlin
/// app's `mix(c, background, t)` does, lands between bytes: `surfaceContainer`
/// is `mix(rail, bg, 0.55)` = (9.9, 22.5, 34.2). These three channels are the
/// same shape, nudged off `.5` so neither model depends on which way a
/// half rounds.
const BETWEEN_FILL: (f32, f32, f32) = (22.6, 45.6, 78.6);

thread_local! {
    static FILL_UNDER_TEST: Cell<(f32, f32, f32)> = const { Cell::new((15.0, 54.0, 78.0)) };
}

fn pixel(frame: &CapturedFrame, x: u32, y: u32) -> (u8, u8, u8) {
    let index = ((y * SIZE + x) * 4) as usize;
    (
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
    )
}

/// What Skia gives: the layer is an 8-bit buffer, so the channel is a whole
/// value BEFORE the alpha multiplies it, and the composite rounds once more.
fn eight_bit_layer(channel_255: f32, alpha: f32) -> u8 {
    let quantised = channel_255.clamp(0.0, 255.0).round();
    (quantised * alpha).round().clamp(0.0, 255.0) as u8
}

/// What folding the alpha into each primitive in float gives instead: one
/// rounding, at the end. Up to a level away from [`eight_bit_layer`].
fn folded_alpha(channel_255: f32, alpha: f32) -> u8 {
    (channel_255 * alpha).round().clamp(0.0, 255.0) as u8
}

struct Probe {
    frame: CapturedFrame,
    /// `(top, height, scale, alpha)` per row, from the ramp itself.
    rows: Vec<(f32, f32, f32, f32)>,
}

fn render_faded_rows(fill: (f32, f32, f32)) -> Option<Probe> {
    FILL_UNDER_TEST.with(|cell| cell.set(fill));
    let (_lock, renderer) = match support::headless_renderer_parts_unencoded() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping the Wear layer-alpha probe: headless WGPU init failed: {err}");
            return None;
        }
    };

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, || {
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty()
                .fill_max_size()
                .background(Color::from_rgb_u8(0, 0, 0)),
            state,
            WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
            |scope| {
                scope.items(ROWS, |_| {
                    // A flat fill and nothing else: what is under test is the
                    // composite, so the row must have no antialiased edge, no
                    // rounded corner and no glyph anywhere near the sample.
                    let fill = FILL_UNDER_TEST.with(Cell::get);
                    Box(
                        Modifier::empty()
                            .fill_max_width()
                            .height(ROW)
                            .background(Color(fill.0 / 255.0, fill.1 / 255.0, fill.2 / 255.0, 1.0)),
                        BoxSpec::default(),
                        || {
                            Spacer(Size {
                                width: 0.0,
                                height: ROW,
                            });
                        },
                    );
                });
            },
        );
    });
    shell.set_viewport(SIZE as f32, SIZE as f32);
    shell.set_buffer_size(SIZE, SIZE);
    shell.update();

    // The ramp's own numbers for this list, recomputed rather than read back so
    // the assertions below do not depend on the renderer to say what alpha it
    // was given.
    let rows = expected_rows();

    let frame = shell
        .renderer()
        .capture_frame(SIZE, SIZE)
        .expect("frame capture");
    Some(Probe { frame, rows })
}

/// `(top, height, scale, alpha)` for each row, straight from the scaling ramp.
fn expected_rows() -> Vec<(f32, f32, f32, f32)> {
    use cranpose_ui::round_scaling_list::{centre_offset, place_row, stack_into, Slot};
    let mut slots: Vec<Slot> = Vec::new();
    stack_into(std::iter::repeat_n(ROW, ROWS), 4.0, &mut slots);
    let viewport = SIZE as f32;
    let offset = centre_offset(&slots, viewport, CentreAnchor::default(), 1.0);
    slots
        .iter()
        .map(|slot| {
            let top = slot.top + offset;
            let row = place_row(viewport, top, slot.height, 1.0).unwrap_or(
                cranpose_ui::round_scaling_list::PlacedRow {
                    top,
                    height: slot.height,
                    scale: 1.0,
                    alpha: 1.0,
                },
            );
            (row.top, row.height, row.scale, row.alpha)
        })
        .collect()
}

#[test]
fn a_faded_row_composites_the_way_skia_composites_an_eight_bit_layer() {
    // Wear's own palette: exact bytes, so quantising is a no-op and this can
    // only confirm the composite is sane, not which rule made it.
    let Some(probe) = render_faded_rows(PALETTE_FILL) else {
        return;
    };
    let report = compare(&probe, PALETTE_FILL);
    assert!(report.sampled >= 4, "only {} rows sampled", report.sampled);
    assert_eq!(
        report.wrong, 0,
        "{} of {} rows missed the 8-bit layer value: {:?}",
        report.wrong, report.sampled, report.misses
    );
}

#[test]
fn a_faded_row_quantises_to_eight_bits_before_the_alpha_not_after() {
    // The discriminating case. With a fill between 8-bit values the two models
    // disagree by a level on some rows, and the framebuffer has to land on the
    // one Skia produces: quantise, THEN multiply.
    let Some(probe) = render_faded_rows(BETWEEN_FILL) else {
        return;
    };
    let report = compare(&probe, BETWEEN_FILL);
    assert!(
        report.discriminating >= 1,
        "no sampled row told the two models apart, so this test proves nothing; \
         {} rows sampled",
        report.sampled
    );
    assert_eq!(
        report.wrong, 0,
        "{} of {} rows ({} of them discriminating) composited to the folded-alpha \
         value rather than the 8-bit layer's: {:?}",
        report.wrong, report.sampled, report.discriminating, report.misses
    );
}

#[derive(Default)]
struct Report {
    sampled: usize,
    wrong: usize,
    /// Rows where the 8-bit layer and the folded-alpha model disagree, so the
    /// framebuffer can actually say which one ran.
    discriminating: usize,
    misses: Vec<String>,
}

fn compare(probe: &Probe, fill: (f32, f32, f32)) -> Report {
    let mut report = Report::default();
    for (index, (top, height, scale, alpha)) in probe.rows.iter().enumerate() {
        // The middle of the row, well clear of any antialiased edge.
        let y = (top + height * 0.5).round();
        if !(4.0..(SIZE as f32 - 4.0)).contains(&y) || *height < 8.0 {
            continue;
        }
        let got = pixel(&probe.frame, SIZE / 2, y as u32);
        let layer = (
            eight_bit_layer(fill.0, *alpha),
            eight_bit_layer(fill.1, *alpha),
            eight_bit_layer(fill.2, *alpha),
        );
        let folded = (
            folded_alpha(fill.0, *alpha),
            folded_alpha(fill.1, *alpha),
            folded_alpha(fill.2, *alpha),
        );
        report.sampled += 1;
        if layer != folded {
            report.discriminating += 1;
        }
        if got != layer {
            report.wrong += 1;
            report.misses.push(format!(
                "row {index} (scale {scale:.4}, alpha {alpha:.4}) drew {got:?}, \
                 8-bit layer {layer:?}, folded alpha {folded:?}"
            ));
        }
    }
    eprintln!(
        "fill {fill:?}: sampled {}, discriminating {}, wrong {}",
        report.sampled, report.discriminating, report.wrong
    );
    report
}
