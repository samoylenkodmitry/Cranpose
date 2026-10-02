use cranpose::{AppLauncher, embedded_view::EmbeddedViewError, prelude::*};

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
    resized.draw(&mut before).expect("resized");
    fresh.draw(&mut after).expect("fresh");
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
    assert!(view.draw(&mut pixels).expect("first frame"));
    assert_eq!(pixels.len(), 64 * 48 * 4);
    assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    for _ in 0..12 {
        view.draw(&mut pixels).expect("pipeline warmup");
    }
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
