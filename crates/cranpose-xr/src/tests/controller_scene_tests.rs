// The controllers' models in the scene of a headset, on the GPU of the machine: a controller in use
// shows a model at its grip in both eyes with depth, and the choices of the app and of the runtime
// decide which model it is.

use super::*;
use crate::{
    Controller, ControllerModel, Hand, InteractionProfile, TrackedPose, controller_look::Offer,
    controller_states::ControllerStates, pose::turn_degrees, volumes::scene_count,
};

#[cfg(feature = "gltf")]
#[path = "controller_gltf_tests.rs"]
mod gltf;

// A headset whose app has no volume: only the controllers' models can stand in its scene.
fn headset_without_volumes() -> Headset {
    let headset = headset_of(Rc::new(|_, _| {}));
    // A runtime that offers no model, such as Android XR
    for hand in Hand::BOTH {
        headset
            .space
            .volumes()
            .controllers()
            .set_offer(hand, Offer::Absent);
    }
    headset
}

// The turn that lays a controller across the view: its handle, along the grip's z, runs to the
// room's x, and its tip, along -z, points to the room's -x, which is the user's left.
fn across() -> [f32; 4] {
    turn_degrees(0.0, 90.0, 0.0)
}

// A controller that the runtime tracks at `position`, held in the hand that the profile names.
fn held(position: [f32; 3]) -> Controller {
    let pose = TrackedPose {
        pose: Pose {
            position,
            orientation: across(),
        },
        linear_velocity: None,
        angular_velocity: None,
    };
    Controller {
        profile: Some(InteractionProfile::MetaTouchPlus),
        grip: Some(pose),
        aim: Some(pose),
        ..Controller::default()
    }
}

// Runs the host's frames until `done`, which the composition and the worker threads of the loads
// must make true.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for a worker thread, and a poll is the way to see it finish"
)]
fn settle(headset: &mut Headset, done: impl Fn(&Headset) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !done(headset) {
        assert!(
            std::time::Instant::now() < deadline,
            "the scene never settled"
        );
        headset.guest.shell().update();
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    headset.guest.shell().update();
}

// What `controllers` of a frame leave in the scene: composed, and drawn from `view` into `eye`.
// Returns the first and last column and row of the pixels that the scene drew.
fn shown_in(headset: &mut Headset, view: &RenderViewpoint, eye: &Eye) -> Option<[u32; 4]> {
    let mut pass = VolumePass::default();
    draw(headset, &mut pass, view, eye).then(|| drawn_box(&read(&headset.gpu, &eye.color, 4)))?
}

// The pixel column and row at which an eye `x` meters to the right of the head sees the room's point
// `point`, in the 90 degree view of 128 pixels that `eye` gives.
fn projected(x: f32, point: [f32; 3]) -> (f32, f32) {
    let scale = EYE as f32 / 2.0;
    let distance = -point[2];
    (
        scale + scale * (point[0] - x) / distance,
        scale - scale * point[1] / distance,
    )
}

// The columns of the pixels that are mostly blue: the plain controller's tip, which marks its front.
fn blue_columns(pixels: &[u8]) -> Vec<u32> {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, texel)| {
            texel[3] == 255 && texel[2] > 120 && u16::from(texel[2]) > u16::from(texel[0]) + 60
        })
        .map(|(at, _)| u32::try_from(at).expect("a pixel") % EYE)
        .collect()
}

#[test]
fn a_controller_in_use_shows_the_plain_model_at_its_grip_in_both_eyes_with_depth() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    let grip = [0.0, 0.0, -0.5];
    states.track([&held(grip), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 1);

    let (left, right) = (Eye::new(&headset.gpu), Eye::new(&headset.gpu));
    let in_left = shown_in(&mut headset, &eye(-0.032), &left).expect("the left eye sees it");
    let in_right = shown_in(&mut headset, &eye(0.032), &right).expect("the right eye sees it");

    // The model is 16 cm long and stands half a meter from the eyes: 20 pixels of 128 in a view of
    // 90 degrees, and no more than 30 with its tip and its trigger
    for (bounds, x) in [(in_left, -0.032), (in_right, 0.032)] {
        let (column, row) = projected(x, grip);
        assert!(
            (bounds[0] as f32 - 1.0..=bounds[2] as f32 + 1.0).contains(&column)
                && (bounds[1] as f32 - 1.0..=bounds[3] as f32 + 1.0).contains(&row),
            "the grip is at ({column}, {row}) and the model covers {bounds:?}"
        );
        let width = bounds[2] - bounds[0] + 1;
        assert!(
            (14..=30).contains(&width),
            "the model is {width} pixels wide"
        );
    }
    // The nearer an object, the farther apart the eyes put it: 64 mm at half a meter is 8 pixels
    let (column_left, column_right) = (projected(-0.032, grip).0, projected(0.032, grip).0);
    assert!(((column_left - column_right) - 8.2).abs() < 0.5);
    let shift = i64::from(in_left[0]) - i64::from(in_right[0]);
    assert!(
        (6..=10).contains(&shift),
        "the eyes' pictures differ by {shift} pixels"
    );

    // The tip marks the front, which the turn put to the user's left of the grip: 8.6 cm, which is
    // 11 pixels, so the blue lies left of the grip in the picture
    let blue = blue_columns(&read(&headset.gpu, &left.color, 4));
    assert!(!blue.is_empty(), "the tip does not show");
    let (grip_column, _) = projected(-0.032, grip);
    assert!(
        blue.iter()
            .all(|column| (*column as f32) < grip_column - 4.0),
        "the tip lies at columns {blue:?}, and the grip at {grip_column}"
    );

    // The scene's depth where the model stands is that of its distance from the eye, half a meter
    // give or take the model's own depth
    let near = EyeCamera::of(&eye(0.0), (EYE, EYE), FirstRow::Top)
        .expect("a camera")
        .camera
        .near();
    let depth = read_depth(&headset.gpu, &left.depth);
    let (column, row) = projected(-0.032, grip);
    let value = depth[(row as u32 * EYE + column as u32) as usize];
    assert!(
        (1.0 - near / 420.0..=1.0 - near / 580.0).contains(&value),
        "the depth at the grip is {value}"
    );
    // The right hand holds none, so nothing stands on its side of the picture
    assert!(in_left[2] < EYE - 20);
}

#[test]
fn a_controller_that_moves_draws_again_where_it_stands_without_composing() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.track([&held([0.0, 0.0, -0.5]), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 1);
    let (view, image, atlas) = (eye(0.0), Eye::new(&headset.gpu), Eye::new(&headset.gpu));
    let before = shown_in(&mut headset, &view, &image).expect("the model shows");
    let draw_atlas = |headset: &mut Headset| {
        headset
            .guest
            .draw(&atlas.color, &atlas.color_view)
            .expect("the atlas")
    };
    for _ in 0..200 {
        if !draw_atlas(&mut headset) {
            break;
        }
    }
    assert!(!draw_atlas(&mut headset), "the atlas keeps drawing");
    // The hand moves 10 cm to the right in each of three frames: only the scene draws
    for step in 1..=3 {
        states.track([
            &held([0.1 * step as f32, 0.0, -0.5]),
            &Controller::default(),
        ]);
        assert!(
            !draw_atlas(&mut headset),
            "the atlas drew for a move of a controller"
        );
    }
    let after = shown_in(&mut headset, &view, &image).expect("the model shows");
    // 30 cm at half a meter is 38 pixels in this view
    let moved = i64::from(after[0]) - i64::from(before[0]);
    assert!(
        (36..=41).contains(&moved),
        "the model moved by {moved} pixels"
    );
}

#[test]
fn a_hand_that_cameras_track_shows_no_model_and_leaves_the_scene_empty() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    let hand = Controller {
        profile: Some(InteractionProfile::ExtHand),
        ..held([0.0, 0.0, -0.5])
    };
    states.track([&hand, &hand]);
    headset.guest.shell().update();
    assert_eq!(scene_count(&headset.space), 0);
    let mut pass = VolumePass::default();
    let eye_image = Eye::new(&headset.gpu);
    assert!(!draw(&mut headset, &mut pass, &eye(0.0), &eye_image));
}

#[test]
fn turned_off_no_model_shows_and_turned_on_again_it_comes_back() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    let controller = held([0.0, 0.0, -0.5]);
    states.track([&controller, &controller]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 2);

    states.handle().set_enabled(false);
    states.track([&controller, &controller]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 0);
    let mut pass = VolumePass::default();
    let image = Eye::new(&headset.gpu);
    assert!(!draw(&mut headset, &mut pass, &eye(0.0), &image));
    assert!(!states.handle().is_enabled());

    states.handle().set_enabled(true);
    states.track([&controller, &controller]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 2);
    assert!(shown_in(&mut headset, &eye(0.0), &image).is_some());
}

#[test]
fn a_hand_waits_for_the_runtimes_answer_and_then_shows_the_plain_model_if_it_has_none() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.set_offer(Hand::Left, Offer::Pending);
    states.track([&held([0.0, 0.0, -0.5]), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 1);
    let image = Eye::new(&headset.gpu);
    // The scene stands for the hand, and it holds nothing yet
    assert_eq!(shown_in(&mut headset, &eye(0.0), &image), None);

    states.set_offer(Hand::Left, Offer::Absent);
    settle(&mut headset, |_| true);
    assert!(shown_in(&mut headset, &eye(0.0), &image).is_some());
}

#[test]
fn the_plain_choice_shows_the_plain_model_where_the_runtime_has_a_model() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.set_offer(Hand::Left, Offer::Bytes(Arc::from(&b"glTF"[..])));
    states
        .handle()
        .set_model(Hand::Left, ControllerModel::Plain);
    states.track([&held([0.0, 0.0, -0.5]), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 1);
    let image = Eye::new(&headset.gpu);
    assert!(shown_in(&mut headset, &eye(0.0), &image).is_some());
    assert!(!blue_columns(&read(&headset.gpu, &image.color, 4)).is_empty());
}

#[test]
fn the_state_of_the_models_is_one_per_composition_and_the_app_reads_it() {
    let headset = headset_without_volumes();
    let one: ControllerStates = headset.space.volumes().controllers();
    let handle = one.handle();
    assert!(handle == headset.space.volumes().controllers().handle());
    assert!(handle.is_enabled());
    assert_eq!(handle.model(Hand::Left), ControllerModel::Runtime);
    handle.set_model(Hand::Right, ControllerModel::Plain);
    assert_eq!(handle.model(Hand::Right), ControllerModel::Plain);
    assert_eq!(handle.model(Hand::Left), ControllerModel::Runtime);
}
