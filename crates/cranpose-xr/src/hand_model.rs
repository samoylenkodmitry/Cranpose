//! The switch of one hand's controller model.

use cranpose::MutableState;

use crate::{Hand, controller_models::Settings};

/// The model of one hand's controller, which the app turns on or off. A
/// [`ControllerModels::hand`](crate::ControllerModels::hand) gives it. Jetpack XR has no controller
/// models. Its `Entity.setEnabled` and `isEnabled` name the switch, which each model has as an
/// entity of its own.
///
/// The handle is a state of the composition. A composable that reads it composes again when the app
/// changes it. Use it on the composition's thread.
#[derive(Clone, Copy, PartialEq)]
pub struct HandModel {
    settings: MutableState<Settings>,
    hand: Hand,
}

impl HandModel {
    pub(crate) fn over(settings: MutableState<Settings>, hand: Hand) -> Self {
        Self { settings, hand }
    }

    /// Whether the headset draws the model. Jetpack XR's `Entity.isEnabled`.
    pub fn is_enabled(&self) -> bool {
        self.settings
            .read(|settings| settings.enabled[self.hand.index()])
    }

    /// Turns the model on or off. With both hands' models off no frame draws a scene for them. The
    /// setting lasts while the app runs. Jetpack XR's `Entity.setEnabled`.
    pub fn set_enabled(&self, enabled: bool) {
        self.settings
            .update(|settings| settings.enabled[self.hand.index()] = enabled);
    }
}
