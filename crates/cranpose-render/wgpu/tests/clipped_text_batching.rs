use std::sync::atomic::{AtomicBool, Ordering};

use cranpose_ui::text::{SpanStyle, TextUnit};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 250;
const PAGE: Color = Color(1.0, 1.0, 1.0, 1.0);
const INK: Color = Color(0.1, 0.2, 0.8, 1.0);
/// Label boxes: `x`, `y` and height in points, all off the pixel grid, 70
/// points wide. Each is too short for the lines its label wraps into and
/// ends partway through the second one's glyphs; boxes lie 60 points apart,
/// so no label's lines reach another box even shown whole.
const LABELS: [[f32; 3]; 8] = [
    [6.3, 4.4, 23.6],
    [124.6, 4.4, 24.3],
    [6.3, 64.2, 25.1],
    [124.6, 64.2, 22.7],
    [6.3, 124.7, 23.4],
    [124.6, 124.7, 24.8],
    [6.3, 184.1, 24.2],
    [124.6, 184.1, 23.1],
];
const LABEL_WIDTH: f32 = 70.0;

/// Whether the labels are held to their boxes' heights, so their wrapped
/// lines run past them and they clip themselves, or given room to show whole.
static HELD: AtomicBool = AtomicBool::new(true);

#[composable]
fn LabelsPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, PAGE, || {
        let held = HELD.load(Ordering::Relaxed);
        for (index, [x, y, height]) in LABELS.into_iter().enumerate() {
            let height = if held { height } else { FRAME_HEIGHT as f32 };
            Box(
                rect_modifier([x, y, LABEL_WIDTH, height]),
                BoxSpec::default(),
                move || {
                    Text(
                        format!("label {index} wraps past its box"),
                        Modifier::empty(),
                        TextStyle::from_span_style(SpanStyle {
                            color: Some(INK),
                            font_size: TextUnit::Sp(13.0),
                            ..Default::default()
                        }),
                    );
                },
            );
        }
    });
}

fn capture(
    held: bool,
) -> Option<(
    cranpose_render_wgpu::CapturedFrame,
    cranpose_render_wgpu::RenderStatsSnapshot,
)> {
    HELD.store(held, Ordering::Relaxed);
    support::warm_app_frame(LabelsPage, FRAME_WIDTH, FRAME_HEIGHT)
}

/// Whether pixel (`x`, `y`) lies `margin` pixels or more inside a label's box
/// (a positive margin) or outside every box (a negative one, measured out).
fn within_labels(x: usize, y: usize, margin: f32) -> bool {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    LABELS.iter().any(|[left, top, height]| {
        x >= left + margin
            && x < left + LABEL_WIDTH - margin
            && y >= top + margin
            && y < top + height - margin
    })
}

#[test]
fn labels_their_boxes_clip_draw_together_as_their_scissors_drew_them() {
    let Some((held, held_stats)) = capture(true) else {
        return;
    };
    let (whole, _) = capture(false).expect("headless WGPU init failed mid-suite");
    let page = [255u8; 4];
    for (index, (held_pixel, whole_pixel)) in held
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(whole.pixels.as_chunks::<4>().0)
        .enumerate()
    {
        let (x, y) = (index % FRAME_WIDTH as usize, index / FRAME_WIDTH as usize);
        if within_labels(x, y, 2.0) {
            assert_eq!(
                held_pixel, whole_pixel,
                "({x}, {y}): inside its box a held label shows its lines as they are"
            );
        } else if !within_labels(x, y, -2.0) {
            assert_eq!(
                held_pixel, &page,
                "({x}, {y}): nothing of a held label shows past its box"
            );
        }
    }
    assert!(
        held.pixels != whole.pixels,
        "the held labels' lines ran past their boxes"
    );
    support::check_golden(
        "clipped_labels",
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/clipped_text/clipped_labels.png"),
        (FRAME_WIDTH, FRAME_HEIGHT),
        &held.pixels,
        "clipped text must cover the pixels its scissor passed, with the same texels",
    );
    assert!(
        held_stats.draw_calls < LABELS.len() as u32,
        "the clipped labels draw together, not one call each: {held_stats:?}"
    );
}
