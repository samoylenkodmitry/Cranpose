// The glTF models of the controllers on the GPU: the runtime's model, which a worker thread loads
// from the bytes that the backend copied out of the runtime, and the model that the app sets.

use super::*;
use crate::SpatialGltfModelSource;

// A glTF file of one meter, facing the user, which stands where a runtime's model would. At half a
// meter from the eyes it fills the whole view.
fn quad_bytes() -> Arc<[u8]> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/models/checker_quad.gltf");
    Arc::from(std::fs::read(path).expect("the test model"))
}

// A controller whose grip faces the viewer, at half a meter.
fn facing() -> Controller {
    let mut controller = held([0.0, 0.0, -0.5]);
    for pose in [&mut controller.grip, &mut controller.aim]
        .into_iter()
        .flatten()
    {
        pose.pose.orientation = [0.0, 0.0, 0.0, 1.0];
    }
    controller
}

fn width(bounds: [u32; 4]) -> u32 {
    bounds[2] - bounds[0] + 1
}

#[test]
fn the_runtimes_model_loads_off_the_frame_loop_and_shows_in_place_of_the_plain_one() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.set_offer(Hand::Left, Offer::Bytes(quad_bytes()));
    states.track([&facing(), &Controller::default()]);
    // While the worker decodes the file the hand shows nothing, and then the model
    settle(&mut headset, |headset| {
        headset
            .space
            .volumes()
            .controllers()
            .loaded(Hand::Left)
            .is_some()
    });
    let image = Eye::new(&headset.gpu);
    let bounds = shown_in(&mut headset, &eye(0.0), &image).expect("the model shows");
    assert!(width(bounds) > 100, "the model covers {bounds:?}");
    // The plain model's tip is not there
    assert!(blue_columns(&read(&headset.gpu, &image.color, 4)).is_empty() || width(bounds) > 100);
}

#[test]
fn a_runtime_model_stays_loaded_while_its_controller_is_away_and_comes_back_with_no_wait() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.set_offer(Hand::Left, Offer::Bytes(quad_bytes()));
    states.track([&facing(), &Controller::default()]);
    settle(&mut headset, |headset| {
        headset
            .space
            .volumes()
            .controllers()
            .loaded(Hand::Left)
            .is_some()
    });
    let loaded = states.loaded(Hand::Left).expect("loaded");

    // The user puts the controller down and picks up their hands, and then the controller again
    states.track([&Controller::default(), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 0);
    states.track([&facing(), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 1);
    let again = states.loaded(Hand::Left).expect("still loaded");
    assert!(Rc::ptr_eq(&loaded, &again), "the model loaded again");
    let image = Eye::new(&headset.gpu);
    let bounds = shown_in(&mut headset, &eye(0.0), &image).expect("the model shows");
    assert!(width(bounds) > 100);
}

#[test]
fn a_runtime_model_that_fails_to_load_ends_at_the_plain_model() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.set_offer(Hand::Left, Offer::Bytes(Arc::from(&b"not a glTF file"[..])));
    states.track([&held([0.0, 0.0, -0.5]), &Controller::default()]);
    settle(&mut headset, |headset| scene_count(&headset.space) == 1);
    let image = Eye::new(&headset.gpu);
    let bounds = shown_in(&mut headset, &eye(0.0), &image).expect("the plain model shows");
    assert!(width(bounds) < 30, "the plain model covers {bounds:?}");
    assert!(!blue_columns(&read(&headset.gpu, &image.color, 4)).is_empty());
}

#[test]
fn the_apps_model_replaces_the_runtimes_for_the_hand_that_it_names() {
    let mut headset = headset_without_volumes();
    let states = headset.space.volumes().controllers();
    states.set_offer(Hand::Left, Offer::Absent);
    states.set_offer(Hand::Right, Offer::Absent);
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/models/checker_quad.gltf");
    states.handle().set_model(
        Hand::Right,
        ControllerModel::Gltf(SpatialGltfModelSource::from_path(path)),
    );
    let mut right = facing();
    right.grip.as_mut().expect("a grip").pose.position = [0.0, 0.0, -0.5];
    states.track([&Controller::default(), &right]);
    settle(&mut headset, |headset| {
        headset
            .space
            .volumes()
            .controllers()
            .loaded(Hand::Right)
            .is_some()
    });
    let image = Eye::new(&headset.gpu);
    let bounds = shown_in(&mut headset, &eye(0.0), &image).expect("the app's model shows");
    assert!(width(bounds) > 100, "the model covers {bounds:?}");
    // The left hand keeps the choice that it had
    assert_eq!(states.handle().model(Hand::Left), ControllerModel::Runtime);
}
