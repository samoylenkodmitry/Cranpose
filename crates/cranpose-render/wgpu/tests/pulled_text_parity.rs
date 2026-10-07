//! Where the vertex stage reads storage, a text a frame draws again is drawn
//! from the glyphs an earlier frame kept on the GPU, each named by its place
//! there. It must cover the pixels the glyphs written whole each frame cover.

use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot};
use cranpose_ui::text::{SpanStyle, TextUnit};
use support::page::*;

use crate::support;

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 100;
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
const FRAMES: usize = 3;

#[composable]
fn LabelsPage() {
    FramePage(FRAME_WIDTH, FRAME_HEIGHT, PAGE, || {
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
    });
}

/// The first frames of the labels page on a device that lacks `flags`.
fn frames(flags: wgpu::DownlevelFlags) -> Option<Vec<(RenderStatsSnapshot, CapturedFrame)>> {
    let (_lock, mut shell) =
        support::app_shell_without(flags, LabelsPage, FRAME_WIDTH, FRAME_HEIGHT)?;
    Some(
        (0..FRAMES)
            .map(|_| support::update_and_capture(&mut shell, FRAME_WIDTH, FRAME_HEIGHT))
            .collect(),
    )
}

#[test]
fn text_drawn_from_its_kept_glyphs_covers_the_pixels_of_glyphs_written_whole() {
    let Some(pulled) = frames(wgpu::DownlevelFlags::empty()) else {
        return;
    };
    let whole =
        frames(wgpu::DownlevelFlags::VERTEX_STORAGE).expect("headless WGPU init failed mid-suite");
    for (index, ((pulled_stats, pulled), (whole_stats, whole))) in
        pulled.iter().zip(&whole).enumerate()
    {
        assert!(
            support::distinct_colors(&pulled.pixels) > 8,
            "frame {index} must draw the labels"
        );
        let differing = support::differing_pixels(FRAME_WIDTH, &pulled.pixels, &whole.pixels);
        assert!(
            differing.is_empty(),
            "frame {index}: pulled and whole glyphs differ at {}",
            support::describe_differing(&differing)
        );
        // A first frame writes every glyph whole; a later one writes twenty
        // bytes a glyph, where a glyph written whole takes sixty-four.
        assert!(
            index == 0 || pulled_stats.upload_bytes * 2 < whole_stats.upload_bytes,
            "frame {index}: labels drawn again must name their kept glyphs: {} bytes \
             uploaded against {} written whole",
            pulled_stats.upload_bytes,
            whole_stats.upload_bytes
        );
    }
}
