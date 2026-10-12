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

#[test]
fn the_safe_area_reaches_content() {
    let seen = Rc::new(Cell::new(EdgeInsets::default()));
    let sink = Rc::clone(&seen);
    let mut app = Application::new(100, 100, 2.0, move || {
        sink.set(local_safe_area_insets().current());
    })
    .expect("valid surface");
    app.set_active(true);
    app.tick(1);
    assert_eq!(seen.get(), EdgeInsets::default());
    // The clock's edge; a broken edge counts as none.
    app.set_safe_area(0.0, 32.0, -4.0, f32::NAN);
    app.tick(16_000_001);
    assert_eq!(seen.get(), EdgeInsets::from_components(0.0, 32.0, 0.0, 0.0));
}

/// The sprites of the current picture, `SPRITE_FLOATS` numbers each.
fn sprite_records(app: &mut Application) -> Vec<Vec<f32>> {
    use cranpose::watchos::SPRITE_FLOATS;
    app.sprites()
        .sprites()
        .chunks_exact(SPRITE_FLOATS)
        .map(<[f32]>::to_vec)
        .collect()
}

fn kind_of(sprite: &[f32]) -> f32 {
    sprite[cranpose::watchos::field::KIND]
}

/// Shapes reach SpriteKit as numbers and text as a texture, which is uploaded
/// once, kept while the picture stays, and kept again when the text moves by
/// whole pixels: the quad moves and nothing is drawn again.
#[test]
fn sprites_carry_shapes_as_numbers_and_text_as_a_kept_texture() {
    use cranpose::watchos::field;
    const TEXTURE: f32 = 0.0;
    const FILL: f32 = 1.0;
    const ARC: f32 = 3.0;
    let shift_sink: Rc<Cell<Option<MutableState<f32>>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&shift_sink);
    let mut app = Application::new(120, 120, 2.0, move || {
        let shift = remember(|| mutableStateOf(0.0_f32)).with(|state| *state);
        sink.set(Some(shift));
        Box(Modifier::empty().fill_max_size(), BoxSpec::default(), move || {
            Canvas(Modifier::empty().fill_max_size(), |scope| {
                scope.draw_rect(Brush::solid(Color(0.1, 0.2, 0.3, 1.0)));
                scope.draw_arc(
                    Brush::solid(Color(1.0, 1.0, 1.0, 1.0)),
                    Point::new(30.0, 30.0),
                    20.0,
                    0.0,
                    1.0,
                    Stroke::new(4.0),
                );
            });
            Text(
                "Score",
                Modifier::empty().offset(shift.value(), 10.0),
                TextStyle::default(),
            );
        });
    })
    .expect("valid surface");
    app.set_active(true);
    assert!(app.tick(1));
    let first = sprite_records(&mut app);
    let kinds: Vec<f32> = first.iter().map(|sprite| kind_of(sprite)).collect();
    assert!(kinds.contains(&FILL), "the background is a fill: {kinds:?}");
    assert!(kinds.contains(&ARC), "the arc is an arc: {kinds:?}");
    assert_eq!(app.sprites().uploads().len(), 1, "the text is drawn into one texture");
    let text = first
        .iter()
        .position(|sprite| kind_of(sprite) == TEXTURE)
        .expect("the text is a texture sprite");

    assert!(!app.tick(16_000_001), "an unchanged picture presents nothing");
    assert!(app.sprites().uploads().is_empty());
    assert_eq!(sprite_records(&mut app), first);

    shift_sink.get().expect("composed").set(3.0);
    assert!(app.tick(32_000_001));
    let moved = sprite_records(&mut app);
    assert!(
        app.sprites().uploads().is_empty(),
        "text moved by whole pixels keeps its texture"
    );
    assert_eq!(moved[text][field::KIND + 1], first[text][field::KIND + 1]);
    assert_eq!(moved[text][field::FRAME], first[text][field::FRAME] + 6.0);
}

/// A texture no sprite shows any more is released once it has gone unused
/// for a while, so text that changes every frame keeps a bounded set.
#[test]
fn sprites_release_textures_nothing_shows() {
    let count_sink: Rc<Cell<Option<MutableState<u32>>>> = Rc::new(Cell::new(None));
    let sink = Rc::clone(&count_sink);
    let mut app = Application::new(120, 60, 1.0, move || {
        let count = remember(|| mutableStateOf(0_u32)).with(|state| *state);
        sink.set(Some(count));
        Text(format!("{}", count.value()), Modifier::empty(), TextStyle::default());
    })
    .expect("valid surface");
    app.set_active(true);
    let count = {
        app.tick(1);
        count_sink.get().expect("composed")
    };
    let mut uploaded = 0usize;
    let mut released = 0usize;
    for frame in 1..=200u32 {
        count.set(frame);
        app.tick(u64::from(frame) * 16_666_667 + 1);
        let sprites = app.sprites();
        uploaded += sprites.uploads().len();
        released += sprites.releases().len();
    }
    assert_eq!(uploaded, 200, "every new number is a new texture");
    assert!(released > 100, "old numbers are let go: {released} of {uploaded}");
    assert!(uploaded - released <= 64, "{} textures stay alive", uploaded - released);
}

/// A sprite is cut to the whole pixels its clip leaves, as the rasterizer
/// cuts the shape.
#[test]
fn sprites_are_cut_to_their_clip() {
    let mut app = Application::new(200, 200, 2.0, || {
        Box(
            Modifier::empty().width(20.0).height(20.0).clip_to_bounds(),
            BoxSpec::default(),
            || {
                Canvas(Modifier::empty().fill_max_size(), |scope| {
                    scope.draw_circle(
                        Brush::solid(Color(1.0, 0.0, 0.0, 1.0)),
                        Point::new(10.0, 10.0),
                        40.0,
                    );
                });
            },
        );
    })
    .expect("valid surface");
    app.set_active(true);
    app.tick(1);
    use cranpose::watchos::field;
    for sprite in sprite_records(&mut app) {
        let frame = &sprite[field::FRAME..field::FRAME + 4];
        assert!(
            frame[0] >= 0.0 && frame[1] >= 0.0 && frame[0] + frame[2] <= 40.0 && frame[1] + frame[3] <= 40.0,
            "a sprite reaches past its 40-pixel clip: {frame:?}"
        );
    }
}
