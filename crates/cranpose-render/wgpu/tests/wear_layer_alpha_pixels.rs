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
//! The structure is right and was always right: `CompositingStrategy::Auto`
//! with alpha below one raises `SurfaceRequirement::GroupOpacity`
//! (`surface_plan.rs`), which allocates a real offscreen target from the 8-bit
//! `OffscreenPool`, and `layer_for_content` draws the row into it at alpha 1
//! and moves the alpha to the composite. The quantisation therefore happens
//! where Skia's does — on the way into the layer buffer — instead of being
//! modelled.
//!
//! What would break the structure is the `ModulateAlpha` strategy, which folds
//! the alpha into each primitive in float and is one call away
//! (`WearScalingLazyColumnSpec::compositing_strategy`). The discriminating
//! tests below are what would notice.
//!
//! # Structure was not enough, and this file said it was
//!
//! Having the right shape left two arithmetic questions to whoever performed
//! each step, and both were answered wrong. See
//! `cranpose-render-common/tests/wear_faded_row_composite.rs`, which pins both
//! against real Kotlin pixels; in short:
//!
//! * The colour reaching the layer buffer was still a fraction, so the
//!   conversion into that buffer had a tie to break and broke it in whatever
//!   direction the hardware prefers. `Color::srgb_8bit` now snaps it first, as
//!   `androidx.compose.ui.graphics.Color` does at construction, leaving no tie.
//! * The composite ran at a float alpha where HWUI truncates it to a byte
//!   (`saveLayerAlpha(&bounds, (int)(alpha * 255))`). `composite_alpha_8bit`
//!   now does.
//!
//! An earlier version of this file recorded a parity report of the composed
//! Settings capsule as `(9, 20, 31)` against Kotlin's `(9, 21, 31)`, proved
//! that value unreachable through an 8-bit layer at any alpha, and concluded
//! that the fix was not in this file's subject. The proof was sound and the
//! conclusion was wrong: it swept only the alpha, and the free variable was the
//! *tie*. `the_capsule_the_composed_build_drew_is_a_tie_broken_the_other_way`
//! below is that sweep done properly.
//!
//! The software `pixels` backend has no offscreen at all
//! (`pipeline.rs::is_render_effect_supported` returns `false` unconditionally),
//! so a `CompositingStrategy::Auto` layer below full opacity folds instead of
//! isolating. It folds `local_content_layer`'s alpha, which is the composite's
//! byte, so the two backends land on the same pixel wherever a flat subtree
//! makes the fold exact at all.

mod support;

use std::cell::Cell;

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::lazy::LazyItems;
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Color, Modifier, Size,
    round_scaling_list::CentreAnchor,
    widgets::{
        Box, BoxSpec, Spacer,
        wear::{
            SwitchButton, SwitchButtonSpec, WearColors, WearScalingLazyColumn,
            WearScalingLazyColumnSpec, rememberWearScalingListState,
        },
    },
};

/// A 454x454 watch, one framebuffer pixel per layout point.
const SIZE: u32 = 454;
/// `MIN_HEIGHT` for a Wear row.
const ROW: f32 = 52.0;
const ROWS: usize = 6;
/// Enough rows that the ones at the ends are well down the fade ramp.
const WIDGET_ROWS: usize = 9;

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
/// app's `mix(c, background, t)` does, lands between bytes.
///
/// This is [`MEASURED_CAPSULE`] itself rather than a stand-in for it, because
/// whether a fill discriminates depends on the alphas the ramp happens to hand
/// the sampled rows, and a synthetic triple that separated the two models under
/// one set of row positions stopped separating them under another. The real
/// `surfaceContainer` separates them on every scaled row, and it is the colour
/// the parity report was about.
const BETWEEN_FILL: (f32, f32, f32) = MEASURED_CAPSULE;

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
/// value BEFORE the alpha multiplies it — and the alpha is a whole value too,
/// truncated, because that is what `saveLayerAlpha` takes.
fn eight_bit_layer(channel_255: f32, alpha: f32) -> u8 {
    let quantised = channel_255.clamp(0.0, 255.0).round() as u32;
    let alpha_byte = (alpha.clamp(0.0, 1.0) * 255.0).floor() as u32;
    ((quantised * alpha_byte + 127) / 255) as u8
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
                scope.items(
                    LazyItems::new(ROWS).key(|index: usize| index as u64),
                    |_| {
                        // A flat fill and nothing else: what is under test is the
                        // composite, so the row must have no antialiased edge, no
                        // rounded corner and no glyph anywhere near the sample.
                        let fill = FILL_UNDER_TEST.with(Cell::get);
                        Box(
                            Modifier::empty()
                                .fill_max_width()
                                .height(ROW)
                                .background(Color(
                                    fill.0 / 255.0,
                                    fill.1 / 255.0,
                                    fill.2 / 255.0,
                                    1.0,
                                )),
                            BoxSpec::default(),
                            || {
                                Spacer(Size {
                                    width: 0.0,
                                    height: ROW,
                                });
                            },
                        );
                    },
                );
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
    rows_for(ROWS)
}

fn rows_for(count: usize) -> Vec<(f32, f32, f32, f32)> {
    use cranpose_ui::round_scaling_list::{RowRun, Slot, centre_offset, place_rows, stack_into};
    let mut slots: Vec<Slot> = Vec::new();
    stack_into(std::iter::repeat_n(ROW, count), 4.0, &mut slots);
    let viewport = SIZE as f32;
    let anchor = CentreAnchor::default();
    let offset = centre_offset(&slots, viewport, anchor, 1.0);
    // The list walks OUTWARD from the anchored row and advances its cursor by
    // each row's reported (scaled) height plus the gap, which is what
    // `ScalingLazyListState.layoutInfo` does. Stacking full heights instead
    // parts company from the second row out and puts these samples on the wrong
    // pixels.
    let index = anchor.index.min(count.saturating_sub(1));
    let mut rows = Vec::new();
    place_rows(
        RowRun {
            viewport,
            anchor: index,
            anchor_top: slots[index].top + offset,
            gap: 4.0,
            density: 1.0,
        },
        &vec![ROW; count],
        &mut rows,
    );
    rows.into_iter()
        .map(|row| (row.top, row.height, row.scale, row.alpha))
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

/// `surfaceContainer` = `mix(rail, background, 0.55)`, the exact capsule the
/// parity report measured, in 0..255.
const MEASURED_CAPSULE: (f32, f32, f32) = (9.9, 22.5, 34.2);

#[test]
fn a_real_faded_row_capsule_composites_through_the_layer_too() {
    // The tests above draw a flat `Box`. The parity report was taken on a real
    // row, whose capsule is a rounded rect the widget draws and whose layer
    // holds a label and a switch as well — so this runs the same probe over an
    // actual `SwitchButton` before concluding anything about the report.
    let (_lock, renderer) = match support::headless_renderer_parts_unencoded() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping the Wear widget layer-alpha probe: {err}");
            return;
        }
    };
    let colors = WearColors {
        surface_container: Color(
            MEASURED_CAPSULE.0 / 255.0,
            MEASURED_CAPSULE.1 / 255.0,
            MEASURED_CAPSULE.2 / 255.0,
            1.0,
        ),
        ..WearColors::default()
    };
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(renderer, root_key, move || {
        let state = rememberWearScalingListState(CentreAnchor::default());
        WearScalingLazyColumn(
            Modifier::empty()
                .fill_max_size()
                .background(Color::from_rgb_u8(0, 0, 0)),
            state,
            WearScalingLazyColumnSpec::default().content_padding(18.0, 34.0),
            move |scope| {
                scope.items(
                    LazyItems::new(WIDGET_ROWS).key(|index: usize| index as u64),
                    move |index| {
                        SwitchButton(
                            Modifier::empty().fill_max_width(),
                            SwitchButtonSpec::default().colors(colors).progress(0.0),
                            false,
                            format!("Row {index}"),
                            None,
                            |_| {},
                        );
                    },
                );
            },
        );
    });
    shell.set_viewport(SIZE as f32, SIZE as f32);
    shell.set_buffer_size(SIZE, SIZE);
    shell.update();
    let frame = shell
        .renderer()
        .capture_frame(SIZE, SIZE)
        .expect("frame capture");

    let mut sampled = 0usize;
    let mut discriminating = 0usize;
    let mut misses = Vec::new();
    let offset = rows_for(WIDGET_ROWS)
        .iter()
        .find_map(|(top, height, _, alpha)| {
            if *alpha < 0.999 {
                return None;
            }
            let y = (top + height * 0.22).round();
            if !(2.0..(SIZE as f32 - 2.0)).contains(&y) {
                return None;
            }
            let got = modal_row_colour(&frame, y as u32)?;
            let expected = (
                eight_bit_layer(MEASURED_CAPSULE.0, *alpha),
                eight_bit_layer(MEASURED_CAPSULE.1, *alpha),
                eight_bit_layer(MEASURED_CAPSULE.2, *alpha),
            );
            Some([
                got.0 as i16 - expected.0 as i16,
                got.1 as i16 - expected.1 as i16,
                got.2 as i16 - expected.2 as i16,
            ])
        })
        .unwrap_or([0; 3]);
    assert!(
        offset.into_iter().all(|channel| channel.abs() <= 1),
        "opaque calibration drift exceeded one channel level: {offset:?}"
    );
    for (index, (top, height, scale, alpha)) in rows_for(WIDGET_ROWS).iter().enumerate() {
        // A fifth of the way down the capsule: inside the fill, above the label
        // and clear of the switch. Take the modal colour along that scan line so
        // the sample is the capsule and not an antialiased corner.
        let y = (top + height * 0.22).round();
        if !(2.0..(SIZE as f32 - 2.0)).contains(&y) {
            continue;
        }
        let Some(got) = modal_row_colour(&frame, y as u32) else {
            continue;
        };
        let layer = (
            eight_bit_layer(MEASURED_CAPSULE.0, *alpha),
            eight_bit_layer(MEASURED_CAPSULE.1, *alpha),
            eight_bit_layer(MEASURED_CAPSULE.2, *alpha),
        );
        let folded = (
            folded_alpha(MEASURED_CAPSULE.0, *alpha),
            folded_alpha(MEASURED_CAPSULE.1, *alpha),
            folded_alpha(MEASURED_CAPSULE.2, *alpha),
        );
        sampled += 1;
        let model_difference = [layer.0, layer.1, layer.2]
            .into_iter()
            .zip([folded.0, folded.1, folded.2])
            .filter(|(layer, folded)| layer != folded)
            .count();
        if model_difference >= 2 {
            discriminating += 1;
        }
        let expected = (
            (layer.0 as i16 + offset[0]).clamp(0, 255) as u8,
            (layer.1 as i16 + offset[1]).clamp(0, 255) as u8,
            (layer.2 as i16 + offset[2]).clamp(0, 255) as u8,
        );
        if model_difference >= 2 && got != expected {
            misses.push(format!(
                "row {index} (scale {scale:.4}, alpha {alpha:.4}) drew {got:?}, \
                 8-bit layer {layer:?}, adjusted {expected:?}, folded alpha {folded:?}"
            ));
        }
    }
    assert!(
        discriminating >= 1,
        "no sampled row told the two models apart, so this test proves nothing; \
         {sampled} rows sampled"
    );
    assert!(
        misses.is_empty(),
        "{} of {sampled} widget rows ({discriminating} discriminating) missed the \
         8-bit layer value: {misses:?}",
        misses.len()
    );
}

#[test]
fn the_capsule_the_composed_build_drew_is_a_tie_broken_the_other_way() {
    // The parity report had the composed capsule at (9, 20, 31) where Kotlin
    // drew (9, 21, 31). Sweeping the alpha with the tie held fixed says that
    // value is unreachable, which is true and was the wrong sweep: green 22.5
    // is an exact half, and the free variable is which way it goes. Sweep both
    // and the reported pixel appears immediately.
    let reported = (9u8, 20u8, 31u8);
    let reachable_with_the_tie_down = (0..=10_000u32).any(|step| {
        let alpha = step as f32 / 10_000.0;
        // The same composite with the layer's green taken DOWN to 22 instead of
        // up to 23 — the answer a converter that breaks ties to even gives.
        let tie_down = |channel: f32| {
            let quantised = if (channel - channel.floor() - 0.5).abs() < 1e-6 {
                channel.floor()
            } else {
                channel.round()
            } as u32;
            let alpha_byte = (alpha * 255.0).floor() as u32;
            ((quantised * alpha_byte + 127) / 255) as u8
        };
        (
            tie_down(MEASURED_CAPSULE.0),
            tie_down(MEASURED_CAPSULE.1),
            tie_down(MEASURED_CAPSULE.2),
        ) == reported
    });
    assert!(
        reachable_with_the_tie_down,
        "the reported composed capsule is not reachable even with the tie taken \
         down, so the diagnosis this file records is wrong"
    );
    let reachable_with_the_tie_up = (0..=10_000u32).any(|step| {
        let alpha = step as f32 / 10_000.0;
        (
            eight_bit_layer(MEASURED_CAPSULE.0, alpha),
            eight_bit_layer(MEASURED_CAPSULE.1, alpha),
            eight_bit_layer(MEASURED_CAPSULE.2, alpha),
        ) == reported
    });
    assert!(
        !reachable_with_the_tie_up,
        "with the tie taken up this composite can reach the reported capsule at \
         some alpha, so the tie is not what distinguished it"
    );
    // And Kotlin's own value is what the fixed composite gives, at a byte alpha
    // it actually uses.
    assert_eq!(
        (
            eight_bit_layer(MEASURED_CAPSULE.0, 230.0 / 255.0),
            eight_bit_layer(MEASURED_CAPSULE.1, 230.0 / 255.0),
            eight_bit_layer(MEASURED_CAPSULE.2, 230.0 / 255.0),
        ),
        (9, 21, 31)
    );
}

/// The most common non-black colour along a scan line — the capsule's fill.
fn modal_row_colour(frame: &CapturedFrame, y: u32) -> Option<(u8, u8, u8)> {
    let mut counts: std::collections::HashMap<(u8, u8, u8), usize> =
        std::collections::HashMap::new();
    for x in 0..SIZE {
        let got = pixel(frame, x, y);
        if got != (0, 0, 0) {
            *counts.entry(got).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .filter(|(_, n)| *n > 32)
        .map(|(colour, _)| colour)
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
    let offset = probe
        .rows
        .iter()
        .filter(|(_, _, _, alpha)| *alpha >= 0.999)
        .find_map(|(top, height, _, alpha)| {
            let y = (top + height * 0.5).round();
            if !(4.0..(SIZE as f32 - 4.0)).contains(&y) || *height < 8.0 {
                return None;
            }
            let got = pixel(&probe.frame, SIZE / 2, y as u32);
            let expected = (
                eight_bit_layer(fill.0, *alpha),
                eight_bit_layer(fill.1, *alpha),
                eight_bit_layer(fill.2, *alpha),
            );
            Some([
                got.0 as i16 - expected.0 as i16,
                got.1 as i16 - expected.1 as i16,
                got.2 as i16 - expected.2 as i16,
            ])
        })
        .unwrap_or([0; 3]);
    assert!(
        offset.into_iter().all(|channel| channel.abs() <= 1),
        "opaque calibration drift exceeded one channel level: {offset:?}"
    );
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
        let expected = (
            (layer.0 as i16 + offset[0]).clamp(0, 255) as u8,
            (layer.1 as i16 + offset[1]).clamp(0, 255) as u8,
            (layer.2 as i16 + offset[2]).clamp(0, 255) as u8,
        );
        let model_difference = [layer.0, layer.1, layer.2]
            .into_iter()
            .zip([folded.0, folded.1, folded.2])
            .filter(|(layer, folded)| layer != folded)
            .count();
        if model_difference >= 2 && got != expected {
            report.wrong += 1;
            report.misses.push(format!(
                "row {index} (scale {scale:.4}, alpha {alpha:.4}) drew {got:?}, \
                 8-bit layer {layer:?}, adjusted {expected:?}, folded alpha {folded:?}"
            ));
        }
    }
    eprintln!(
        "fill {fill:?}: sampled {}, discriminating {}, wrong {}",
        report.sampled, report.discriminating, report.wrong
    );
    report
}
