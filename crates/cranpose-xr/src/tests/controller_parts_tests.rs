// The parts of the test controller (scripts/dev/make_gltf_models.py), which keeps its parts' poses
// at the keys of a Meta Quest's runtime models, posed by the inputs of a controller.

use std::path::PathBuf;

use super::*;
use crate::{
    Button, Thumbstick, Trigger, gltf_import::import, gltf_model::GltfModel,
    spatial_gltf_model_source::SpatialGltfModelSource,
};

fn load(name: &str) -> GltfModel {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/models")
        .join(name);
    import(&SpatialGltfModelSource::from_path(path)).expect("the test controller loads")
}

fn node(model: &GltfModel, name: &str) -> usize {
    model
        .names
        .iter()
        .position(|named| named.as_deref() == Some(name))
        .expect("the node")
}

// The transform of each node when `controller` poses the parts.
fn posed(model: &GltfModel, controller: &Controller) -> Vec<Trs> {
    let mut locals = model.rest.clone();
    model.parts.pose(&PartInput::of(controller), &mut locals);
    locals
}

fn turn(axis: [f32; 3], degrees: f32) -> [f32; 4] {
    let (sin, cos) = (degrees.to_radians() / 2.0).sin_cos();
    let length = axis.iter().map(|a| a * a).sum::<f32>().sqrt();
    [
        axis[0] / length * sin,
        axis[1] / length * sin,
        axis[2] / length * sin,
        cos,
    ]
}

fn near<const N: usize>(a: [f32; N], b: [f32; N]) {
    assert!(
        a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5),
        "{a:?} is not {b:?}"
    );
}

fn pulled(value: f32) -> Controller {
    Controller {
        trigger: Trigger {
            value,
            ..Trigger::default()
        },
        ..Controller::default()
    }
}

fn leaning(x: f32, y: f32) -> Controller {
    Controller {
        thumbstick: Thumbstick {
            x,
            y,
            ..Thumbstick::default()
        },
        ..Controller::default()
    }
}

#[test]
fn the_model_names_six_parts_and_a_model_without_such_nodes_has_none() {
    let model = load("controller_parts.glb");
    let parts: Vec<&str> = model.parts.describe().collect();
    assert_eq!(
        parts,
        [
            "trigger",
            "grip",
            "first button",
            "second button",
            "menu button",
            "thumbstick"
        ]
    );
    assert!(load("tower.glb").parts.is_empty());
}

#[test]
fn the_trigger_turns_from_its_rest_to_the_pose_at_its_key_as_it_is_pulled() {
    let model = load("controller_parts.glb");
    let trigger = node(&model, "b_trigger_front");
    near(
        posed(&model, &pulled(0.0))[trigger].rotation,
        [0.0, 0.0, 0.0, 1.0],
    );
    // The file holds 20 degrees at key 16 and 7 degrees at key 17: the pull takes key 16
    near(
        posed(&model, &pulled(0.5))[trigger].rotation,
        turn([1.0, 0.0, 0.0], 10.0),
    );
    near(
        posed(&model, &pulled(1.0))[trigger].rotation,
        turn([1.0, 0.0, 0.0], 20.0),
    );
    // The pull leaves the trigger's place and every other part as they are
    let full = posed(&model, &pulled(1.0));
    assert_eq!(full[trigger].translation, model.rest[trigger].translation);
    for (index, (posed, rest)) in full.iter().zip(&model.rest).enumerate() {
        if index != trigger {
            assert_eq!(posed, rest, "node {index} moved");
        }
    }
}

#[test]
fn a_button_goes_down_while_pressed_and_comes_back_when_released() {
    let model = load("controller_parts.glb");
    let first = node(&model, "b_button_a");
    let second = node(&model, "b_button_b");
    let pressed = Controller {
        primary: Button {
            pressed: true,
            touched: true,
        },
        ..Controller::default()
    };
    let down = posed(&model, &pressed);
    near(down[first].translation, [0.01, 0.018, 0.0]);
    near(down[second].translation, [-0.01, 0.02, 0.0]);
    // A thumb that rests on the button presses nothing
    let touched = Controller {
        primary: Button {
            pressed: false,
            touched: true,
        },
        ..Controller::default()
    };
    near(
        posed(&model, &touched)[first].translation,
        [0.01, 0.02, 0.0],
    );
    let released = posed(&model, &Controller::default());
    near(released[first].translation, [0.01, 0.02, 0.0]);
}

#[test]
fn the_thumbstick_takes_the_lean_of_each_extreme_from_its_key() {
    let model = load("controller_parts.glb");
    let stick = node(&model, "b_thumbstick");
    // Forward leans the cap toward -z, to the right toward +x: the turn's axis is up crossed with
    // the lean
    for ((x, y), axis) in [
        ((0.0, 1.0), [-1.0, 0.0, 0.0]),
        ((1.0, 0.0), [0.0, 0.0, -1.0]),
        ((0.0, -1.0), [1.0, 0.0, 0.0]),
        ((-1.0, 0.0), [0.0, 0.0, 1.0]),
    ] {
        near(
            posed(&model, &leaning(x, y))[stick].rotation,
            turn(axis, 15.0),
        );
    }
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    near(
        posed(&model, &leaning(diagonal, diagonal))[stick].rotation,
        turn([-1.0, 0.0, -1.0], 15.0),
    );
    // A half lean goes halfway, a lean past the edge stops at the edge, and the middle is the rest
    near(
        posed(&model, &leaning(0.0, 0.5))[stick].rotation,
        turn([-1.0, 0.0, 0.0], 7.5),
    );
    near(
        posed(&model, &leaning(0.0, 1.4))[stick].rotation,
        turn([-1.0, 0.0, 0.0], 15.0),
    );
    near(
        posed(&model, &leaning(0.0, 0.0))[stick].rotation,
        [0.0, 0.0, 0.0, 1.0],
    );
}

#[test]
fn the_grip_and_the_menu_button_follow_their_inputs() {
    let model = load("controller_parts.glb");
    let controller = Controller {
        squeeze: crate::Squeeze {
            value: 1.0,
            pressed: true,
        },
        menu: true,
        ..Controller::default()
    };
    let locals = posed(&model, &controller);
    near(
        locals[node(&model, "b_trigger_grip")].rotation,
        turn([0.0, 0.0, 1.0], -12.0),
    );
    near(
        locals[node(&model, "b_button_oculus")].translation,
        [0.0, 0.019, 0.02],
    );
}
