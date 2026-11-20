// A glTF model in the scene of a headset: each eye shows the model's texture upright, from its own
// place, and the model loads on a worker thread while the frames go on.

use super::*;
use crate::{SpatialGltfModel, SpatialGltfModelSource, rememberSpatialGltfModelState};

#[composable]
fn gltf_volume() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/models/checker_quad.gltf");
    let state = rememberSpatialGltfModelState(SpatialGltfModelSource::from_path(path));
    SpatialGltfModel(
        state,
        SubspaceModifier::new().size(300.0, 300.0).depth(300.0),
        |scope| {
            // A light with no shade keeps the texture's own colors
            scope.light(Light {
                direction: Vec3::new(0.0, 0.0, 1.0),
                ambient: 1.0,
                shadows: false,
                size: 0.0,
            });
        },
    );
}

// Frames of the host until the model has loaded and its volume stands in the space: the frames
// compose and run the tasks, which hand the loaded model to the composition.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for a worker thread, and a poll is the way to see it finish"
)]
fn wait_for_the_volume(headset: &mut Headset) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while headset.space.volume_count() == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the model never loaded"
        );
        headset.guest.shell().update();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

// The box of the pixels that the scene drew, as the first and last column and row.
fn drawn_box(pixels: &[u8]) -> Option<[u32; 4]> {
    let mut found: Option<[u32; 4]> = None;
    for (at, texel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if texel[3] == 0 {
            continue;
        }
        let (x, y) = (at as u32 % EYE, at as u32 / EYE);
        let [left, top, right, bottom] = found.get_or_insert([x, y, x, y]);
        (*left, *top, *right, *bottom) = (
            (*left).min(x),
            (*top).min(y),
            (*right).max(x),
            (*bottom).max(y),
        );
    }
    found
}

// The pixel at the quadrant of `bounds` whose middle is `across` and `down` of the box's width and
// height from its top left corner.
fn at_quarter(pixels: &[u8], bounds: [u32; 4], across: f32, down: f32) -> [u8; 3] {
    let x = bounds[0] as f32 + (bounds[2] - bounds[0] + 1) as f32 * across;
    let y = bounds[1] as f32 + (bounds[3] - bounds[1] + 1) as f32 * down;
    let at = ((y as u32 * EYE + x as u32) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

fn is_color(pixel: [u8; 3], strong: [bool; 3]) -> bool {
    pixel
        .iter()
        .zip(strong)
        .all(|(&value, strong)| if strong { value > 200 } else { value < 60 })
}

#[test]
fn each_eye_shows_the_models_texture_upright_from_its_own_place() {
    let mut headset = headset_of(Rc::new(|_, _| gltf_volume()));
    let mut pass = VolumePass::default();
    let (left, right) = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
    wait_for_the_volume(&mut headset);
    assert!(draw(&mut headset, &mut pass, &eye(-0.032), &left));
    assert!(draw(&mut headset, &mut pass, &eye(0.032), &right));

    let mut starts = Vec::new();
    for image in [&left, &right] {
        let pixels = read(&headset.gpu, &image.color, 4);
        let bounds = drawn_box(&pixels).expect("the eye shows the model");
        starts.push(bounds[0]);
        // The quad is 30 cm a meter ahead: 19 pixels wide in a view of 128 pixels for 90 degrees
        let width = bounds[2] - bounds[0] + 1;
        assert!(
            (17..=21).contains(&width),
            "the model is {width} pixels wide"
        );
        for ((across, down), strong, name) in [
            ((0.25, 0.25), [true, false, false], "red at the top left"),
            ((0.75, 0.25), [false, true, false], "green at the top right"),
            (
                (0.25, 0.75),
                [false, false, true],
                "blue at the bottom left",
            ),
            (
                (0.75, 0.75),
                [true, true, false],
                "yellow at the bottom right",
            ),
        ] {
            let pixel = at_quarter(&pixels, bounds, across, down);
            assert!(is_color(pixel, strong), "{name}: the pixel is {pixel:?}");
        }
    }
    // The eyes are 64 mm apart and the model is a meter away: the pictures differ by 4 pixels
    assert!(
        (3..=5).contains(&(starts[0] - starts[1])),
        "the left eye's model starts at column {}, the right eye's at {}",
        starts[0],
        starts[1]
    );
}

// A tower that swings: its block moves and turns over two seconds. `held` receives its state.
#[composable]
fn tower_volume(held: Rc<RefCell<Option<crate::SpatialGltfModelState>>>) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/models/tower.glb");
    let state = rememberSpatialGltfModelState(SpatialGltfModelSource::from_path(path));
    held.replace(Some(state.clone()));
    SpatialGltfModel(
        state,
        SubspaceModifier::new().size(300.0, 300.0).depth(300.0),
        |_| {},
    );
}

// One frame of the host, with both eyes drawn. Returns the left eye's picture.
fn frame(headset: &mut Headset, pass: &mut VolumePass, atlas: &Eye, eyes: &[Eye; 2]) -> Vec<u8> {
    headset
        .guest
        .draw(&atlas.color, &atlas.color_view)
        .expect("the atlas");
    for (x, image) in [(-0.032, &eyes[0]), (0.032, &eyes[1])] {
        let camera = EyeCamera::of(&eye(x), (EYE, EYE), FirstRow::Top).expect("a camera");
        pass.draw(&mut headset.guest, &headset.space, &camera, &image.image())
            .expect("a frame");
    }
    read(&headset.gpu, &eyes[0].color, 4)
}

#[test]
fn a_playing_animation_moves_the_model_and_a_stop_puts_it_back_at_rest() {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let mut headset = headset_of(Rc::new(move |_, _| tower_volume(Rc::clone(&keep))));
    let mut pass = VolumePass::default();
    let eyes = [Eye::new(&headset.gpu), Eye::new(&headset.gpu)];
    let atlas = Eye::new(&headset.gpu);
    wait_for_the_volume(&mut headset);
    assert!(draw(&mut headset, &mut pass, &eye(-0.032), &eyes[0]));
    let state = held.take().expect("the state");
    let swing = state.animations().remove(0);
    assert_eq!(swing.name(), Some("swing"));
    assert_eq!(swing.duration(), std::time::Duration::from_secs(2));
    assert_eq!(swing.animation_state(), crate::GltfAnimationState::Stopped);
    let rest = frame(&mut headset, &mut pass, &atlas, &eyes);

    // A speed of a thousand runs the two seconds in one frame, which a late frame cuts to a tenth
    // of a second: a few frames reach the end
    swing.set_speed(1000.0);
    swing.start();
    for _ in 0..8 {
        frame(&mut headset, &mut pass, &atlas, &eyes);
    }
    assert_eq!(swing.animation_state(), crate::GltfAnimationState::Stopped);
    let ended = frame(&mut headset, &mut pass, &atlas, &eyes);
    assert!(rest != ended, "the block stayed where it was");
    // A run that ended keeps its last pose, and a stop changes nothing then
    swing.stop();
    assert!(
        frame(&mut headset, &mut pass, &atlas, &eyes) == ended,
        "a stop after the end changed the pose"
    );

    // A start and a stop between two frames leave the state as it was and put the model at rest
    swing.start();
    swing.stop();
    frame(&mut headset, &mut pass, &atlas, &eyes);
    assert!(
        frame(&mut headset, &mut pass, &atlas, &eyes) == rest,
        "the model did not go back to rest"
    );
    state.stop_all_animations();
}

#[test]
fn a_looping_animation_goes_on_until_paused_and_goes_on_again_when_resumed() {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let mut headset = headset_of(Rc::new(move |_, _| tower_volume(Rc::clone(&keep))));
    let mut pass = VolumePass::default();
    let eyes = [Eye::new(&headset.gpu), Eye::new(&headset.gpu)];
    let atlas = Eye::new(&headset.gpu);
    wait_for_the_volume(&mut headset);
    assert!(draw(&mut headset, &mut pass, &eye(-0.032), &eyes[0]));
    let swing = held.take().expect("the state").animations().remove(0);
    swing.set_looping(true);
    // A tenth of a second of a frame, at a speed of 7.3, passes a run of 2 seconds in 3 frames
    // and a bit, and keeps the model moving
    swing.set_speed(7.3);
    swing.start();
    let mut seen = Vec::new();
    for _ in 0..12 {
        seen.push(frame(&mut headset, &mut pass, &atlas, &eyes));
        assert_eq!(swing.animation_state(), crate::GltfAnimationState::Playing);
    }
    assert!(
        seen.windows(2).any(|pair| pair[0] != pair[1]),
        "the model stood still while it played"
    );
    swing.pause();
    assert_eq!(swing.animation_state(), crate::GltfAnimationState::Paused);
    // A frame that follows the pause settles the pose, and the next frames keep it
    let held_pose = {
        frame(&mut headset, &mut pass, &atlas, &eyes);
        frame(&mut headset, &mut pass, &atlas, &eyes)
    };
    assert!(
        frame(&mut headset, &mut pass, &atlas, &eyes) == held_pose,
        "a paused animation moved the model"
    );
    swing.resume();
    assert_eq!(swing.animation_state(), crate::GltfAnimationState::Playing);
    let moved = (0..6).any(|_| frame(&mut headset, &mut pass, &atlas, &eyes) != held_pose);
    assert!(moved, "a resumed animation did not move the model");
}
