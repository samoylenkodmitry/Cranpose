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

// The pixel `across` and `down` quarters of the way from the top left corner of `bounds`, which is
// a box of the first and last column and row.
fn at_quarter(pixels: &[u8], bounds: [u32; 4], across: u32, down: u32) -> [u8; 3] {
    let x = bounds[0] + (bounds[2] - bounds[0] + 1) * across / 4;
    let y = bounds[1] + (bounds[3] - bounds[1] + 1) * down / 4;
    let at = usize::try_from((y * EYE + x) * 4).expect("an offset");
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
            ((1, 1), [true, false, false], "red at the top left"),
            ((3, 1), [false, true, false], "green at the top right"),
            ((1, 3), [false, false, true], "blue at the bottom left"),
            ((3, 3), [true, true, false], "yellow at the bottom right"),
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
    let mut frames = 0;
    while swing.animation_state() != crate::GltfAnimationState::Stopped {
        frames += 1;
        assert!(frames < 100, "the animation never ended");
        frame(&mut headset, &mut pass, &atlas, &eyes);
    }
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

// A tower volume whose callback writes the input events of a ray into `events`.
#[composable]
fn pickable_tower(
    held: Rc<RefCell<Option<crate::SpatialGltfModelState>>>,
    events: Rc<RefCell<Vec<crate::InputEvent>>>,
) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/models/tower.glb");
    let state = rememberSpatialGltfModelState(SpatialGltfModelSource::from_path(path));
    held.replace(Some(state.clone()));
    SpatialGltfModel(
        state,
        SubspaceModifier::new().size(300.0, 300.0).depth(300.0),
        move |scope| {
            let events = Rc::clone(&events);
            scope.on_input_event(move |event| events.borrow_mut().push(event));
        },
    );
}

// A pointer over the tower volume once its model has loaded, and the model's state.
fn pointing_at_the_tower() -> (picking::Pointing, crate::SpatialGltfModelState) {
    let held = Rc::new(RefCell::new(None));
    let events = Rc::new(RefCell::new(Vec::new()));
    let (keep, sink) = (Rc::clone(&held), Rc::clone(&events));
    let mut headset = headset_of(Rc::new(move |_, _| {
        pickable_tower(Rc::clone(&keep), Rc::clone(&sink));
    }));
    wait_for_the_volume(&mut headset);
    let pointing = picking::Pointing::new(headset, events);
    (pointing, held.take().expect("the state"))
}

#[test]
fn a_ray_meets_a_model_and_the_state_names_the_gltf_node_that_it_hit() {
    let (mut pointing, state) = pointing_at_the_tower();
    // The block stands at the middle of the volume at rest, and the ray points at it
    let hit = pointing.point(0.0, 0.0)[0]
        .hit_info
        .expect("the ray meets the block");
    let node = state
        .node_of(&hit)
        .expect("the hit is on a node of the model");
    assert_eq!(node.index, 1);
    assert_eq!(node.name.as_deref(), Some("block"));
    // A ray far to the side meets nothing
    assert!(
        pointing
            .point(0.25, 0.0)
            .iter()
            .all(|event| event.hit_info.is_none())
    );
}

#[test]
fn a_ray_follows_the_node_that_an_animation_moves() {
    let (mut pointing, state) = pointing_at_the_tower();
    let atlas = Eye::new(&pointing.headset.gpu);
    let eyes = [
        Eye::new(&pointing.headset.gpu),
        Eye::new(&pointing.headset.gpu),
    ];
    let at_rest = pointing.point(0.0, 0.0)[0]
        .hit_info
        .expect("the ray meets the block at rest")
        .hit_position;

    // The animation runs to its end, which puts the block elsewhere
    let swing = state.animations().remove(0);
    swing.set_speed(1000.0);
    swing.start();
    let mut frames = 0;
    while swing.animation_state() != crate::GltfAnimationState::Stopped {
        frames += 1;
        assert!(frames < 100, "the animation never ended");
        frame(&mut pointing.headset, &mut pointing.pass, &atlas, &eyes);
    }
    frame(&mut pointing.headset, &mut pointing.pass, &atlas, &eyes);
    let after = pointing
        .point(0.0, 0.0)
        .iter()
        .rev()
        .find_map(|event| event.hit_info)
        .and_then(|hit| hit.hit_position);
    assert_ne!(
        after, at_rest,
        "the ray met the block where it stood at rest"
    );
}
