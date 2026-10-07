//! Where the vertex stage reads storage, a text a frame draws again is drawn
//! from the glyphs an earlier frame kept on the GPU, each named by its place
//! there. It must cover the pixels the glyphs written whole each frame cover.

use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui::text::{SpanStyle, TextUnit};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 132;
const PAGE: Color = Color(1.0, 1.0, 1.0, 1.0);
const INK: Color = Color(0.1, 0.2, 0.8, 1.0);
/// Label boxes, `x`, `y` and height in points, each 110 points wide. Labels
/// 0, 2 and 4 show one text, on the pixel grid and off it. Label 4 is
/// shorter than its line, so its glyphs are cut to its box.
const LABELS: [[f32; 3]; 5] = [
    [4.0, 4.0, 24.0],
    [120.0, 4.0, 24.0],
    [4.3, 36.6, 24.0],
    [120.5, 36.0, 24.0],
    [4.0, 72.0, 9.0],
];
const LABEL_WIDTH: f32 = 110.0;
/// A paragraph long enough for a retained run's own draw where the vertex
/// stage reads no storage, wrapping past its box.
const PARAGRAPH: &str = "Quarterly totals for every region, rounded to the nearest unit, \
                         with the change from the last quarter beside each one.";
const PARAGRAPH_BOX: [f32; 4] = [4.3, 100.6, 230.0, 30.0];
const FRAMES: usize = 3;

#[composable]
fn LabelsPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, PAGE, Labels);
}

#[composable]
fn ParagraphPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, PAGE, || {
        Labels();
        Box(rect_modifier(PARAGRAPH_BOX), BoxSpec::default(), || {
            Text(
                PARAGRAPH,
                Modifier::empty(),
                TextStyle::from_span_style(SpanStyle {
                    color: Some(INK),
                    font_size: TextUnit::Sp(11.0),
                    ..Default::default()
                }),
            );
        });
    });
}

#[composable]
fn Labels() {
    for (index, [x, y, height]) in LABELS.into_iter().enumerate() {
        Box(
            rect_modifier([x, y, LABEL_WIDTH, height]),
            BoxSpec::default(),
            move || {
                Text(
                    format!("Total {} of 1,024", index % 2),
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
}

/// The first frames of `page` on a device that lacks `flags`.
fn frames(
    page: fn(),
    flags: wgpu::DownlevelFlags,
) -> Option<Vec<(RenderStatsSnapshot, CapturedFrame)>> {
    let (_lock, mut shell) = support::app_shell_without(flags, page, FRAME_WIDTH, FRAME_HEIGHT)?;
    Some(
        (0..FRAMES)
            .map(|_| support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT))
            .collect(),
    )
}

/// Asserts that `page` draws the same pixels on a device whose vertex stage
/// reads storage as on one where it does not, frame by frame, and hands back
/// both devices' frames.
fn assert_alike(page: fn()) -> Option<[Vec<(RenderStatsSnapshot, CapturedFrame)>; 2]> {
    let pulled = frames(page, wgpu::DownlevelFlags::empty())?;
    let whole = frames(page, wgpu::DownlevelFlags::VERTEX_STORAGE)
        .expect("headless WGPU init failed mid-suite");
    for (index, ((_, pulled), (_, whole))) in pulled.iter().zip(&whole).enumerate() {
        assert!(
            support::distinct_colors(&pulled.pixels) > 8,
            "frame {index} must draw the texts"
        );
        let differing = support::differing_pixels(FRAME_WIDTH, &pulled.pixels, &whole.pixels);
        assert!(
            differing.is_empty(),
            "frame {index}: pulled and whole glyphs differ at {}",
            support::describe_differing(&differing)
        );
    }
    Some([pulled, whole])
}

#[test]
fn text_drawn_from_its_kept_glyphs_covers_the_pixels_of_glyphs_written_whole() {
    let Some([pulled, whole]) = assert_alike(LabelsPage) else {
        return;
    };
    for (index, ((pulled, _), (whole, _))) in pulled.iter().zip(&whole).enumerate().skip(1) {
        // A first frame writes every glyph whole; a later one writes twenty
        // bytes a glyph, where a glyph written whole takes sixty-four.
        assert!(
            pulled.upload_bytes * 2 < whole.upload_bytes,
            "frame {index}: labels drawn again must name their kept glyphs: {} bytes \
             uploaded against {} written whole",
            pulled.upload_bytes,
            whole.upload_bytes
        );
    }
}

#[test]
fn a_paragraph_pulled_covers_the_pixels_of_its_retained_runs_own_draw() {
    assert_alike(ParagraphPage);
}
