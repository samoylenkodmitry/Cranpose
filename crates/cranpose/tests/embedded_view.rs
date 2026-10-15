use std::time::{Duration, Instant};

use cranpose::{
    AppLauncher,
    embedded_view::{EmbeddedView, EmbeddedViewError},
    prelude::*,
};

/// Draws until the view has nothing left to draw. A frame skips a shape
/// whose pipeline still compiles, and the view draws again once it lands.
fn draw_settled(view: &mut EmbeddedView, pixels: &mut Vec<u8>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    assert!(view.draw(pixels).expect("first frame"));
    while view.draw(pixels).expect("follow-up frame") {
        assert!(Instant::now() < deadline, "the view kept drawing");
    }
}

#[test]
fn resizing_a_small_host_matches_a_component_created_at_its_final_size() {
    fn content() {
        Column(
            Modifier::empty()
                .fill_max_size()
                .background(Color(1.0, 0.0, 0.0, 1.0))
                .padding(4.0),
            ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
            || {
                Spacer(
                    Modifier::empty()
                        .width(16.0)
                        .height(8.0)
                        .background(Color(0.0, 1.0, 0.0, 1.0)),
                );
                Spacer(
                    Modifier::empty()
                        .width(16.0)
                        .height(8.0)
                        .background(Color(0.0, 0.0, 1.0, 1.0)),
                );
            },
        );
    }
    let mut resized = AppLauncher::new()
        .create_embedded_view(1, 1, 1.0, content)
        .expect("small");
    resized.resize(64, 48, 2.0).expect("resize");
    let mut fresh = AppLauncher::new()
        .create_embedded_view(64, 48, 2.0, content)
        .expect("fresh");
    let mut before = Vec::new();
    let mut after = Vec::new();
    draw_settled(&mut resized, &mut before);
    draw_settled(&mut fresh, &mut after);
    assert_eq!(before, after);
    assert_eq!(&after[..4], &[255, 0, 0, 255]);
    let green = (10 * 64 + 10) * 4;
    assert_eq!(&after[green..green + 4], &[0, 255, 0, 255]);
}

#[test]
fn host_driven_frames_resize_suspend_and_reject_invalid_dimensions() {
    let mut view = AppLauncher::new()
        .create_embedded_view(64, 48, 2.0, || {
            Spacer(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(1.0, 0.0, 0.0, 1.0)),
            );
        })
        .expect("GPU embedded component");
    let mut pixels = Vec::new();
    draw_settled(&mut view, &mut pixels);
    assert_eq!(pixels.len(), 64 * 48 * 4);
    assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    assert!(!view.draw(&mut pixels).expect("unchanged component"));
    view.resize(80, 60, 2.0).expect("native resize");
    assert!(view.draw(&mut pixels).expect("resized frame"));
    assert_eq!(pixels.len(), 80 * 60 * 4);
    assert_eq!(view.shell().viewport_size(), (40.0, 30.0));
    view.set_visible(false);
    assert!(!view.draw(&mut pixels).expect("hidden view"));
    view.set_visible(true);
    assert!(view.draw(&mut pixels).expect("resumed view"));
    assert!(matches!(
        view.resize(0, 60, 1.0),
        Err(EmbeddedViewError::InvalidSize { .. })
    ));
    assert!(matches!(
        view.resize(80, 60, f32::NAN),
        Err(EmbeddedViewError::InvalidSize { .. })
    ));
    assert_eq!(view.pixel_size(), (80, 60));
}
