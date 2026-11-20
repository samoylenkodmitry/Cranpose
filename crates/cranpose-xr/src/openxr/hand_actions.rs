//! The actions of one hand: its poses, its inputs and its motor.

use openxr as xr;

use super::{graphics::Api, session::XrSession, tracked_pose::tracked};
use crate::{Button, Controller, Squeeze, Thumbstick, Trigger, controller_bindings::Input};

pub(crate) fn failed(what: &str) -> impl Fn(xr::sys::Result) -> String + use<> {
    let what = what.to_owned();
    move |error| format!("OpenXR {what}: {error}")
}

// The actions of one hand. The session attaches them before it makes the spaces of the poses.
pub(crate) struct HandActions {
    aim: xr::Action<xr::Posef>,
    grip: xr::Action<xr::Posef>,
    haptic: xr::Action<xr::Haptic>,
    trigger: xr::Action<f32>,
    squeeze: xr::Action<f32>,
    stick_x: xr::Action<f32>,
    stick_y: xr::Action<f32>,
    trigger_touch: xr::Action<bool>,
    stick_click: xr::Action<bool>,
    stick_touch: xr::Action<bool>,
    primary_click: xr::Action<bool>,
    primary_touch: xr::Action<bool>,
    secondary_click: xr::Action<bool>,
    secondary_touch: xr::Action<bool>,
    menu: xr::Action<bool>,
    thumbrest_touch: xr::Action<bool>,
}

impl HandActions {
    // The actions of the hand `side` ("left" or "right"), in `set`.
    pub(crate) fn new(set: &xr::ActionSet, side: &str) -> Result<Self, String> {
        let make = |name: &str| {
            let title = format!("{side} {}", name.replace('_', " "));
            (format!("{side}_{name}"), title)
        };
        fn action<T: xr::ActionTy>(
            set: &xr::ActionSet,
            (name, title): (String, String),
        ) -> Result<xr::Action<T>, String> {
            set.create_action::<T>(&name, &title, &[])
                .map_err(failed("action"))
        }
        Ok(Self {
            aim: action(set, make("aim"))?,
            grip: action(set, make("grip"))?,
            haptic: action(set, make("haptic"))?,
            trigger: action(set, make("trigger"))?,
            squeeze: action(set, make("squeeze"))?,
            stick_x: action(set, make("stick_x"))?,
            stick_y: action(set, make("stick_y"))?,
            trigger_touch: action(set, make("trigger_touch"))?,
            stick_click: action(set, make("stick_click"))?,
            stick_touch: action(set, make("stick_touch"))?,
            primary_click: action(set, make("primary_click"))?,
            primary_touch: action(set, make("primary_touch"))?,
            secondary_click: action(set, make("secondary_click"))?,
            secondary_touch: action(set, make("secondary_touch"))?,
            menu: action(set, make("menu"))?,
            thumbrest_touch: action(set, make("thumbrest_touch"))?,
        })
    }

    // The binding of `input` to `path`. A float action binds to a path that holds a click, for
    // which the runtime gives 0 or 1.
    pub(crate) fn binding(&self, input: Input, path: xr::Path) -> xr::Binding<'_> {
        match input {
            Input::AimPose => xr::Binding::new(&self.aim, path),
            Input::GripPose => xr::Binding::new(&self.grip, path),
            Input::Haptic => xr::Binding::new(&self.haptic, path),
            Input::Trigger => xr::Binding::new(&self.trigger, path),
            Input::Squeeze => xr::Binding::new(&self.squeeze, path),
            Input::StickX => xr::Binding::new(&self.stick_x, path),
            Input::StickY => xr::Binding::new(&self.stick_y, path),
            Input::TriggerTouch => xr::Binding::new(&self.trigger_touch, path),
            Input::StickClick => xr::Binding::new(&self.stick_click, path),
            Input::StickTouch => xr::Binding::new(&self.stick_touch, path),
            Input::PrimaryClick => xr::Binding::new(&self.primary_click, path),
            Input::PrimaryTouch => xr::Binding::new(&self.primary_touch, path),
            Input::SecondaryClick => xr::Binding::new(&self.secondary_click, path),
            Input::SecondaryTouch => xr::Binding::new(&self.secondary_touch, path),
            Input::Menu => xr::Binding::new(&self.menu, path),
            Input::ThumbrestTouch => xr::Binding::new(&self.thumbrest_touch, path),
        }
    }
}

// An attached hand: its actions and the spaces of its aim and grip poses.
pub(crate) struct HandInput {
    actions: HandActions,
    aim_space: xr::Space,
    grip_space: xr::Space,
}

impl HandInput {
    // The hand's spaces, made after the session attached its actions.
    pub(crate) fn new(actions: HandActions, session: &xr::Session<Api>) -> Result<Self, String> {
        let space = |pose: &xr::Action<xr::Posef>| {
            pose.create_space(session, xr::Path::NULL, xr::Posef::IDENTITY)
                .map_err(failed("pose space"))
        };
        Ok(Self {
            aim_space: space(&actions.aim)?,
            grip_space: space(&actions.grip)?,
            actions,
        })
    }

    // Writes what the hand reports at `time` into `controller`, except its profile and its
    // presses. The actions must have been synced.
    pub(crate) fn read(&self, xr_session: &XrSession, time: xr::Time, controller: &mut Controller) {
        let session = &xr_session.session;
        let actions = &self.actions;
        let float = |action: &xr::Action<f32>| {
            action
                .state(session, xr::Path::NULL)
                .ok()
                .filter(|state| state.is_active)
                .map_or(0.0, |state| state.current_state)
        };
        let on = |action: &xr::Action<bool>| {
            action
                .state(session, xr::Path::NULL)
                .ok()
                .is_some_and(|state| state.is_active && state.current_state)
        };
        controller.aim = tracked(&self.aim_space, &xr_session.local, time);
        controller.grip = tracked(&self.grip_space, &xr_session.local, time);
        controller.trigger = Trigger {
            value: float(&actions.trigger),
            touched: on(&actions.trigger_touch),
            ..Trigger::default()
        };
        controller.squeeze = Squeeze {
            value: float(&actions.squeeze),
            ..Squeeze::default()
        };
        controller.thumbstick = Thumbstick {
            x: float(&actions.stick_x),
            y: float(&actions.stick_y),
            pressed: on(&actions.stick_click),
            touched: on(&actions.stick_touch),
        };
        controller.primary = Button {
            pressed: on(&actions.primary_click),
            touched: on(&actions.primary_touch),
        };
        controller.secondary = Button {
            pressed: on(&actions.secondary_click),
            touched: on(&actions.secondary_touch),
        };
        controller.menu = on(&actions.menu);
        controller.thumbrest_touched = on(&actions.thumbrest_touch);
    }

    // Plays `vibration` on the hand's motor, or stops the motor with `None`.
    pub(crate) fn vibrate(
        &self,
        session: &xr::Session<Api>,
        vibration: Option<&crate::Vibration>,
    ) -> Result<(), xr::sys::Result> {
        match vibration {
            Some(vibration) => self.actions.haptic.apply_feedback(
                session,
                xr::Path::NULL,
                &xr::HapticVibration::new()
                    .amplitude(vibration.amplitude())
                    .frequency(vibration.frequency().unwrap_or(0.0))
                    .duration(xr::Duration::from_nanos(
                        i64::try_from(vibration.duration().as_nanos()).unwrap_or(i64::MAX),
                    )),
            ),
            None => self.actions.haptic.stop_feedback(session, xr::Path::NULL),
        }
    }
}
