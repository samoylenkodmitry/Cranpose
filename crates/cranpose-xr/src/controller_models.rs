//! The app's choices about the controllers' models.

use cranpose::MutableState;
use cranpose_core::CompositionLocal;

use crate::{ControllerModel, Hand, HandModel, locals::with_locals};

// What the app chose.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    // Whether each hand shows its model, left then right
    pub(crate) enabled: [bool; 2],
    // The model of each hand, left then right
    pub(crate) models: [ControllerModel; 2],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: [true; 2],
            models: [ControllerModel::Runtime, ControllerModel::Runtime],
        }
    }
}

/// Whether the headset draws each hand's controller model, and which model draws it. An app reads
/// the handle from [`local_controller_models`].
///
/// The models are on by default. A hand shows its model while the runtime tracks a controller in
/// it and the app left the hand's model on ([`hand`](Self::hand)). A hand that cameras track, with
/// no controller in it, shows no model. The hand's own ray stays as it is. Each hand has its own
/// switch, so an app can show the model of the hand that points at its panels alone. Jetpack XR
/// has no controller models.
///
/// The handle is a state of the composition. A composable that reads it composes again when the app
/// changes it. Use it on the composition's thread.
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

    /// The model of `hand`'s controller, which the app turns on or off.
    pub fn hand(&self, hand: Hand) -> HandModel {
        HandModel::over(self.settings, hand)
    }

    /// The model that draws `hand`'s controller.
    pub fn model(&self, hand: Hand) -> ControllerModel {
        self.settings
            .read(|settings| settings.models[hand.index()].clone())
    }

    /// Draws `hand`'s controller with `model` in place of the runtime's. A glTF model loads on a
    /// worker thread. The hand shows nothing until it has loaded. A model that fails to load shows
    /// the [`ControllerModel::Plain`] one.
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
