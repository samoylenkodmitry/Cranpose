// What the demo shows of the controllers' models. The headset draws a model at each controller in
// use. The counter panel has a button that cycles the choice of both hands: the runtime's own model,
// Cranpose's plain model, and none.
//
// `XR_DEMO_CONTROLLERS` builds the demo for a measurement. It holds names that a comma separates:
// `plain` and `off` start with that choice, `only` shows nothing but the models, so that a screenshot
// holds the models alone, and `panel` adds a panel that stands through the left controller's grip, so
// that the part of the model that lies behind the panel's plane hides and the part in front shows.
// A build with `only` or `panel` logs the eyes and the grips once a second, which the pixel check of
// `scripts/dev/xr_controller_check.py` reads. The environment variable `XR_DEMO_TRACKING=none`
// builds an app that declares no `Use::controller_models()`, which shows the plain model on any
// runtime.

use std::{cell::Cell, rc::Rc};

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;
use cranpose_xr::{
    ControllerModel, ControllerModels, FollowMode, FollowTarget, FollowingSubspace, Hand, Pose,
    RenderViewpoint, SpatialFrameEffect, SpatialPanel, Subspace, SubspaceModifier,
    local_controller_models,
};
use player_demo::sized;

const CONTROLLERS_BUILD: Option<&str> = option_env!("XR_DEMO_CONTROLLERS");

// Whether the build's list holds `name`.
fn built(name: &str) -> bool {
    CONTROLLERS_BUILD.is_some_and(|list| list.split(',').any(|item| item == name))
}

// Whether the build shows the models and nothing else.
pub(crate) fn models_only() -> bool {
    built("only") || built("panel")
}

// The choice that both hands start with.
#[composable]
pub(crate) fn StartingChoice() {
    let Some(models) = local_controller_models().current() else {
        return;
    };
    remember(move || {
        if built("plain") {
            set_both(&models, ControllerModel::Plain);
        }
        if built("off") {
            models.set_enabled(false);
        }
    });
}

fn set_both(models: &ControllerModels, model: ControllerModel) {
    models.set_model(Hand::Left, model.clone());
    models.set_model(Hand::Right, model);
}

// A build with nothing but the models. `panel` adds a magenta panel of 40 cm that stands in the
// vertical plane through the left controller's grip, along the heading of the controller.
#[composable]
pub(crate) fn ModelsOnly() {
    StartingChoice();
    LogFrames();
    // A subspace with nothing in it: the app still composes and runs its frames
    Subspace(|| {});
    if !built("panel") {
        return;
    }
    FollowingSubspace(
        FollowTarget::Controller(Hand::Left, FollowMode::Tight),
        || {
            SpatialPanel(
                SubspaceModifier::new()
                    .size(400.0, 400.0)
                    .rotate(0.0, 90.0, 0.0),
                || {
                    Box(
                        Modifier::empty()
                            .fill_max_size()
                            .background(Color(0.9, 0.1, 0.8, 1.0)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

// Logs the eyes' poses and fields of view, the controllers' grips and the frames of the last second,
// once a second, in the units that `scripts/dev/xr_controller_check.py` reads: the position in
// meters, the orientation as a quaternion x, y, z, w, and the field of view as the four angles of
// left, right, up and down in radians.
#[composable]
fn LogFrames() {
    let state = remember(|| Rc::new(Cell::new((u64::MAX, 0_u32)))).with(Rc::clone);
    SpatialFrameEffect(move |frame| {
        let (second, frames) = state.get();
        let now = frame.predicted_display_time.as_secs();
        if second == now {
            state.set((second, frames + 1));
            return;
        }
        state.set((now, 1));
        let pose = |pose: Pose| {
            let [x, y, z] = pose.position;
            let [qx, qy, qz, qw] = pose.orientation;
            format!("{x} {y} {z} {qx} {qy} {qz} {qw}")
        };
        let eye = |eye: Option<RenderViewpoint>| {
            eye.map_or_else(
                || "none".to_owned(),
                |view| {
                    let fov = view.field_of_view;
                    format!(
                        "{} fov {} {} {} {}",
                        pose(view.pose),
                        fov.angle_left,
                        fov.angle_right,
                        fov.angle_up,
                        fov.angle_down
                    )
                },
            )
        };
        let grip = |hand: Hand| {
            frame
                .controller(hand)
                .grip
                .map_or_else(|| "none".to_owned(), |grip| pose(grip.pose))
        };
        log::info!(
            "xr demo: controllers: left_eye {}; right_eye {}; left_grip {}; right_grip {}; profiles {:?} {:?}; frames {frames}",
            eye(frame.left_eye),
            eye(frame.right_eye),
            grip(Hand::Left),
            grip(Hand::Right),
            frame.left_controller.profile,
            frame.right_controller.profile
        );
    });
}

// The state of the choice, in the words of the button.
fn label(models: &ControllerModels) -> &'static str {
    if !models.is_enabled() {
        return "Controller models: none";
    }
    match models.model(Hand::Left) {
        ControllerModel::Plain => "Controller models: plain",
        _ => "Controller models: the runtime's",
    }
}

// The button of the counter panel: the runtime's model, then the plain one, then none.
#[composable]
pub(crate) fn ControllerModelsButton() {
    let Some(models) = local_controller_models().current() else {
        return;
    };
    Button(
        Modifier::empty()
            .background(Color(0.45, 0.3, 0.7, 1.0))
            .rounded_corners(16.0)
            .padding_symmetric(32.0, 18.0),
        ButtonSpec::default(),
        move || {
            if !models.is_enabled() {
                models.set_enabled(true);
                set_both(&models, ControllerModel::Runtime);
            } else if models.model(Hand::Left) == ControllerModel::Runtime {
                set_both(&models, ControllerModel::Plain);
            } else {
                models.set_enabled(false);
            }
        },
        move || {
            Text(
                label(&models),
                Modifier::empty(),
                sized(24.0, Some(FontWeight::BOLD)),
            );
        },
    );
}
