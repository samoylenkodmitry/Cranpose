use std::time::Instant;

use cranpose_ui_graphics::{
    Brush, Color, DrawScope, DrawScopeDefault, Point, Size, Stroke, StrokeCap,
};

/// A frame the size of a watch game's: bricks as annular sectors and rails
/// and outlines as stroked, round-capped arcs, at the counts Megaboss records.
const SECTORS: usize = 11_000;
const ARCS: usize = 5_000;

fn record_frame(frame: usize) -> usize {
    let mut scope = DrawScopeDefault::new(Size::new(450.0, 450.0));
    let center = Point::new(225.0, 225.0);
    let breathing = 1.0 + (frame as f32 * 0.37).sin() * 0.012;
    for index in 0..SECTORS {
        let ring = (index % 40) as f32;
        let slot = (index / 40) as f32;
        let inner = (60.0 + ring * 4.0) * breathing;
        let color = Color(
            0.2 + (index % 7) as f32 * 0.1,
            0.4,
            0.9 - (index % 5) as f32 * 0.1,
            1.0,
        );
        scope.draw_annular_sector(
            Brush::solid(color),
            center,
            inner,
            inner + 3.2,
            slot * 0.023 + ring * 0.001,
            0.021,
        );
    }
    for index in 0..ARCS {
        let radius = (40.0 + (index % 90) as f32 * 2.0) * breathing;
        scope.draw_arc(
            Brush::solid(Color(1.0, 0.8, 0.2, 0.9)),
            center,
            radius,
            index as f32 * 0.011,
            0.4 + (index % 11) as f32 * 0.05,
            Stroke::new(1.5).with_cap(StrokeCap::Round),
        );
    }
    let recording = scope.finish();
    recording.len()
}

/// Nanoseconds per recorded shape, printed; run with
/// `cargo test --release -p cranpose-ui-graphics --test integration arc_recording_speed -- --ignored --nocapture`.
#[test]
#[ignore = "a timing probe, not a correctness check"]
fn arc_recording_speed() {
    let frames = 200;
    let mut shapes = 0;
    for frame in 0..20 {
        shapes = record_frame(frame);
    }
    let started = Instant::now();
    for frame in 0..frames {
        shapes = record_frame(frame);
    }
    let elapsed = started.elapsed().as_nanos() as f64;
    println!(
        "{shapes} shapes a frame: {:.1} ns per shape, {:.2} ms per frame",
        elapsed / (frames * (SECTORS + ARCS)) as f64,
        elapsed / frames as f64 / 1e6
    );
}
