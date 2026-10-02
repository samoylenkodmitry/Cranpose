use std::{cell::Cell, rc::Rc};

use cranpose::{prelude::*, watchos::Application};

#[test]
fn text_rasterization_uses_the_app_render_context() {
    let mut app = Application::new(200, 100, 2.0, || {
        Text("Watch text", Modifier::empty(), TextStyle::default());
    })
    .expect("valid surface");
    app.set_active(true);
    assert!(app.tick(1));
    assert!(
        app.pixels()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|pixel| *pixel != [18, 18, 24, 255])
    );
}

#[test]
fn shared_content_renders_and_idle_frames_do_not_present() {
    let mut app = Application::new(80, 60, 1.0, || {
        Spacer(
            Modifier::empty()
                .fill_max_size()
                .background(Color(1.0, 0.0, 0.0, 1.0)),
        );
    })
    .expect("valid surface");
    assert!(!app.tick(0));
    app.set_active(true);
    assert!(app.tick(1));
    assert_eq!(app.pixels().len(), 80 * 60 * 4);
    assert_eq!(&app.pixels()[4 * (30 * 80 + 40)..][..4], &[255, 0, 0, 255]);
    assert!(!app.tick(16_000_001));
    app.resize(100, 70, 2.0).expect("resize");
    assert!(app.tick(32_000_001));
    assert_eq!(app.pixels().len(), 100 * 70 * 4);
    assert_eq!(&app.pixels()[4 * (69 * 100 + 99)..][..4], &[255, 0, 0, 255]);
}

#[test]
fn touch_points_and_crown_reach_shared_modifiers() {
    let clicked = Rc::new(Cell::new(0));
    let delta = Rc::new(Cell::new(0.0));
    let click_sink = Rc::clone(&clicked);
    let crown_sink = Rc::clone(&delta);
    let mut app = Application::new(160, 120, 2.0, move || {
        let click_sink = Rc::clone(&click_sink);
        let crown_sink = Rc::clone(&crown_sink);
        Box(
            Modifier::empty()
                .fill_max_size()
                .on_rotary_scroll_event(move |event| {
                    crown_sink.set(event.vertical_scroll_pixels);
                    true
                }),
            BoxSpec::default(),
            move || {
                let click_sink = Rc::clone(&click_sink);
                Spacer(
                    Modifier::empty()
                        .offset(50.0, 40.0)
                        .width(20.0)
                        .height(20.0)
                        .background(Color(1.0, 0.0, 0.0, 1.0))
                        .clickable(move |_| click_sink.set(click_sink.get() + 1)),
                );
            },
        );
    })
    .expect("valid surface");
    app.set_active(true);
    app.tick(1);
    assert_eq!(
        &app.pixels()[4 * (118 * 160 + 138)..][..4],
        &[255, 0, 0, 255]
    );
    assert_ne!(&app.pixels()[4 * (50 * 160 + 60)..][..4], &[255, 0, 0, 255]);
    assert!(app.crown(1.0, 20));
    assert!(delta.get() < 0.0);
    app.touch(0, 60.0, 50.0);
    app.touch(2, 60.0, 50.0);
    app.tick(32_000_001);
    assert_eq!(clicked.get(), 1);
    app.set_active(false);
    assert!(!app.tick(48_000_001));
    assert!(!app.crown(1.0, 50));
    app.touch(0, 70.0, 50.0);
    app.touch(2, 70.0, 50.0);
    app.set_active(true);
    assert!(app.tick(64_000_001));
    assert_eq!(clicked.get(), 1);
}

#[test]
fn invalid_surface_and_input_are_rejected() {
    assert!(Application::new(0, 10, 1.0, || {}).is_err());
    assert!(Application::new(10, 10, f32::NAN, || {}).is_err());
    assert!(Application::new(u32::MAX, u32::MAX, 1.0, || {}).is_err());
    let mut app = Application::new(10, 10, 1.0, || {}).expect("surface");
    assert!(app.resize(0, 10, 1.0).is_err());
    assert_eq!(app.width(), 10);
    assert!(!app.crown(f32::NAN, 0));
    app.touch(99, 0.0, 0.0);
}
