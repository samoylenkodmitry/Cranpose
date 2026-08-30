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

const SIZE: u32 = 454;
const ROW: f32 = 52.0;
const ROWS: usize = 6;
const WIDGET_ROWS: usize = 9;

const PALETTE_FILL: (f32, f32, f32) = (15.0, 54.0, 78.0);

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

fn eight_bit_layer(channel_255: f32, alpha: f32) -> u8 {
    let quantised = channel_255.clamp(0.0, 255.0).round() as u32;
    let alpha_byte = (alpha.clamp(0.0, 1.0) * 255.0).floor() as u32;
    ((quantised * alpha_byte + 127) / 255) as u8
}

fn folded_alpha(channel_255: f32, alpha: f32) -> u8 {
    (channel_255 * alpha).round().clamp(0.0, 255.0) as u8
}

struct Probe {
    frame: CapturedFrame,
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

    let rows = expected_rows();

    let frame = shell
        .renderer()
        .capture_frame(SIZE, SIZE)
        .expect("frame capture");
    Some(Probe { frame, rows })
}

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

const MEASURED_CAPSULE: (f32, f32, f32) = (9.9, 22.5, 34.2);

#[test]
fn a_real_faded_row_capsule_composites_through_the_layer_too() {
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
    let reported = (9u8, 20u8, 31u8);
    let reachable_with_the_tie_down = (0..=10_000u32).any(|step| {
        let alpha = step as f32 / 10_000.0;
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
    assert_eq!(
        (
            eight_bit_layer(MEASURED_CAPSULE.0, 230.0 / 255.0),
            eight_bit_layer(MEASURED_CAPSULE.1, 230.0 / 255.0),
            eight_bit_layer(MEASURED_CAPSULE.2, 230.0 / 255.0),
        ),
        (9, 21, 31)
    );
}

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
