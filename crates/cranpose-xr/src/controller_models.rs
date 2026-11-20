//! The app's choices about the controllers' models.

use cranpose::{MutableState, mutableStateOf};
use cranpose_core::CompositionLocal;

use crate::{ControllerModel, Hand, locals::with_locals};

// What the app chose.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    // The model of each hand, left then right
    pub(crate) models: [ControllerModel; 2],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            models: [ControllerModel::Runtime, ControllerModel::Runtime],
        }
    }
}

/// Whether the headset draws the controllers' models, and which model draws each hand's
/// controller. An app reads the handle from [`local_controller_models`].
///
/// The models are on by default. A hand shows its model while the runtime tracks a controller in
/// it. A hand that cameras track, with no controller in it, shows no model, and the hand's own
/// ray stays as it is. Jetpack XR has no controller models. Its `Entity.setEnabled` and
/// `isEnabled` name the switch.
///
/// The handle is a state of the composition: a composable that reads it composes again when the
/// app changes it. Use it on the composition's thread.
#[derive(Clone, Copy, PartialEq)]
pub struct ControllerModels {
    settings: MutableState<Settings>,
}

impl ControllerModels {
    // The handle over `settings`, which the scene's states share.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn over(settings: MutableState<Settings>) -> Self {
        Self { settings }
    }

    // The settings of a new composition: the models on, each hand with the runtime's model.
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    pub(crate) fn new_settings() -> MutableState<Settings> {
        mutableStateOf(Settings::default())
    }

    /// Whether the headset draws the models. Jetpack XR's `Entity.isEnabled`.
    pub fn is_enabled(&self) -> bool {
        self.settings.read(|settings| settings.enabled)
    }

    /// Turns the models of both hands on or off. Off, no hand shows a model and no frame draws a
    /// scene for them. The setting persists while the app runs. Jetpack XR's `Entity.setEnabled`.
    pub fn set_enabled(&self, enabled: bool) {
        self.settings.update(|settings| settings.enabled = enabled);
    }

    /// The model that draws `hand`'s controller.
    pub fn model(&self, hand: Hand) -> ControllerModel {
        self.settings
            .read(|settings| settings.models[hand.index()].clone())
    }

    /// Draws `hand`'s controller with `model` in place of the runtime's. A glTF model loads on a
    /// worker thread, and the hand shows nothing until it has loaded. A model that fails to load
    /// shows the [`ControllerModel::Plain`] one.
    pub fn set_model(&self, hand: Hand, model: ControllerModel) {
        self.settings
            .update(|settings| settings.models[hand.index()] = model);
    }
}

/// The app's handle on the controllers' models, or `None` outside a headset, where no controller
/// has a model.
pub fn local_controller_models() -> CompositionLocal<Option<ControllerModels>> {
    with_locals(|locals| locals.controller_models.clone())
}
