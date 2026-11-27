// Stroked text and the bold Compose synthesizes for a face that lacks one
// draw the pictures they drew through tiny-skia's raster pipeline: thin
// strokes that draw as hairlines, a hairline 1 px wide, and strokes wide
// enough to fill as outlines.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_macros::composable;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{
    Column, ColumnSpec, Modifier, SpanStyle, Text, TextStyle,
    text::{FontWeight, TextDrawStyle, TextUnit},
};

const WIDTH: u32 = 240;
const HEIGHT: u32 = 300;

// FNV-1a of the frame, drawn by the pixmap path before the switch to masks
// (c72066531).
const PINNED_FRAME: u64 = 7_832_325_334_350_055_234;

fn style(size: f32, stroke: Option<f32>, weight: Option<FontWeight>) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(size),
            draw_style: stroke.map(|width| TextDrawStyle::Stroke { width }),
            font_weight: weight,
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

#[composable]
fn Lines() {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        for width in [0.5, 1.0, 2.0] {
            Text(
                "Rag stroke",
                Modifier::empty(),
                style(18.0, Some(width), None),
            );
        }
        for size in [12.0, 14.0, 16.0, 18.0, 40.0] {
            Text(
                "Rag bold",
                Modifier::empty(),
                style(size, None, Some(FontWeight::BOLD)),
            );
        }
    });
}

fn frame() -> Vec<u8> {
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        || Lines(),
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    shell.update();
    let mut pixels = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw(&mut pixels, WIDTH, HEIGHT));
    pixels
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn stroked_and_fake_bold_text_draws_its_pinned_picture() {
    let pixels = frame();
    let colors = pixels.as_chunks::<4>().0;
    assert!(
        colors.iter().any(|pixel| *pixel != colors[0]),
        "the frame shows no text"
    );
    assert_eq!(
        fnv1a(&pixels),
        PINNED_FRAME,
        "stroked or fake-bold text drew other pixels"
    );
}
