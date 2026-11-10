// A translucent stroke too long for the slicer keeps its place on a surface
// wider than one coverage mask: a chart's dimmed curve on a wide window.

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::{Box, BoxSpec, Modifier};
use cranpose_ui_graphics::{Brush, Color, DrawStyle, Path, Point, Stroke};

const WIDTH: u32 = 3000;
const HEIGHT: u32 = 40;
// Where the zigzag runs; the stroke is level before it.
const ZIGZAG: std::ops::Range<u32> = 2400..2900;
const TEETH: u32 = 600;

// A white surface with a half transparent blue stroke: level at mid height
// up to the zigzag, then up and down over the whole height.
fn wide_stroke() -> Vec<u8> {
    let mut shell = AppShell::new_with_size(
        PixelsRenderer::new(),
        location_key(file!(), line!(), column!()),
        || {
            Box(
                Modifier::empty().fill_max_size().draw_behind(|scope| {
                    scope.draw_rect(Brush::solid(Color::WHITE));
                    let mut path = Path::new();
                    path.move_to(Point::new(10.0, 20.0));
                    path.line_to(Point::new(ZIGZAG.start as f32, 20.0));
                    let span = (ZIGZAG.end - ZIGZAG.start) as f32;
                    for tooth in 0..TEETH {
                        let x = ZIGZAG.start as f32 + span * tooth as f32 / TEETH as f32;
                        let y = if tooth % 2 == 0 { 5.0 } else { 35.0 };
                        path.line_to(Point::new(x, y));
                    }
                    scope.draw_path(
                        &path,
                        Brush::solid(Color(0.0, 0.0, 1.0, 0.5)),
                        DrawStyle::Stroke(Stroke::new(1.5)),
                    );
                }),
                BoxSpec::default(),
                || {},
            );
        },
        (WIDTH, HEIGHT),
        (WIDTH as f32, HEIGHT as f32),
    );
    shell.update();
    let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    shell
        .primary()
        .with_renderer(|renderer| renderer.draw(&mut frame, WIDTH, HEIGHT));
    frame
}

// How many pixels of row `y` the stroke painted between columns `columns`.
fn painted(frame: &[u8], y: u32, columns: std::ops::Range<u32>) -> usize {
    columns
        .filter(|x| {
            let at = ((y * WIDTH + x) * 4) as usize;
            frame[at] < 250
        })
        .count()
}

#[test]
fn a_wide_translucent_stroke_keeps_its_place() {
    let frame = wide_stroke();
    assert!(
        painted(&frame, 10, 2500..2800) > 100,
        "the zigzag is missing from its place"
    );
    assert_eq!(
        painted(&frame, 10, 100..2300),
        0,
        "the zigzag was drawn before its place"
    );
    assert!(
        painted(&frame, 20, 100..2300) > 2000,
        "the level part is missing from its place"
    );
    assert_eq!(
        painted(&frame, 20, 2000..2100),
        100,
        "the level part breaks where two masks meet"
    );
}
