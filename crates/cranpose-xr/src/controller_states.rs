//! The states that the controllers' models share: the app's settings, what the runtime offers, which
//! controllers are in use and where they stand.
//!
//! The frame loop writes them and the scene reads them. A state that the scene reads while it
//! composes, such as which hands show a model, changes when a controller comes or goes. A state that
//! it reads while it draws, such as the map that carries a model to its controller, changes in every
//! frame and composes nothing.

use cranpose::{MutableState, mutableStateOf, space::Mat4};

use crate::{
    Controller, ControllerModel, ControllerModels, Hand, Pose, controller_grips,
    controller_look::{Load, Look, Offer, look},
    controller_models::Settings,
    controller_placement::scene_from_grip,
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ControllerStates {
    settings: MutableState<Settings>,
    // The runtime's model of each hand's controller
    offers: MutableState<[Offer; 2]>,
    // Which hands show a model now
    shown: MutableState<[bool; 2]>,
    // The map from each controller's model to the scene, while the hand shows one
    maps: [MutableState<Mat4>; 2],
    // The state of the glTF model that each hand loads, once its loader composed it
    #[cfg(feature = "gltf")]
    loads: MutableState<[Option<crate::SpatialGltfModelState>; 2]>,
}

impl ControllerStates {
    // The states of a composition: the models on, each hand with the runtime's model, no offer yet
    // and no controller in use. A composition makes them, because the states belong to its
    // runtime.
    pub(crate) fn new() -> Self {
        Self {
            settings: mutableStateOf(Settings::default()),
            offers: mutableStateOf([Offer::Pending, Offer::Pending]),
            shown: mutableStateOf([false; 2]),
            maps: [
                mutableStateOf(Mat4::IDENTITY),
                mutableStateOf(Mat4::IDENTITY),
            ],
            #[cfg(feature = "gltf")]
            loads: mutableStateOf([None, None]),
        }
    }

    // The app's handle on the settings.
    pub(crate) fn handle(&self) -> ControllerModels {
        ControllerModels::over(self.settings)
    }

    // Records the runtime's model of `hand`'s controller.
    pub(crate) fn set_offer(&self, hand: Hand, offer: Offer) {
        self.offers.update(|offers| offers[hand.index()] = offer);
    }

    // Takes the controllers of a frame: each hand that holds a controller shows a model at its grip,
    // unless the app turned the hand's model off. The parts of a shown model take the controller's
    // inputs.
    pub(crate) fn track(&self, controllers: [&Controller; 2]) {
        let enabled = self.settings.get_non_reactive().enabled;
        let mut grips = controller_grips::shown(controllers);
        for (grip, enabled) in grips.iter_mut().zip(enabled) {
            if !enabled {
                *grip = None;
            }
        }
        self.show(grips);
        #[cfg(feature = "gltf")]
        self.follow(controllers, grips);
    }

    // Poses the parts of each shown model that has loaded from its controller's inputs.
    #[cfg(feature = "gltf")]
    fn follow(&self, controllers: [&Controller; 2], grips: [Option<Pose>; 2]) {
        let loads = self.loads.get_non_reactive();
        for ((load, controller), grip) in loads.iter().zip(controllers).zip(grips) {
            if grip.is_none() {
                continue;
            }
            if let Some(loaded) = load
                .as_ref()
                .and_then(crate::SpatialGltfModelState::loaded_now)
            {
                loaded.follow(controller);
            }
        }
    }

    // Moves the models to the controllers' grips, `None` for a hand that shows none. A hand that
    // comes or goes composes its model in or out. A hand that stays moves its model by one state
    // write, which the draw reads.
    pub(crate) fn show(&self, grips: [Option<Pose>; 2]) {
        let shown = grips.map(|grip| grip.is_some());
        if self.shown.get_non_reactive() != shown {
            self.shown.set(shown);
        }
        for (map, grip) in self.maps.iter().zip(grips) {
            if let Some(grip) = grip {
                map.set(scene_from_grip(&grip));
            }
        }
    }

    // How many hands show a model now, outside the composition.
    pub(crate) fn shown_count(&self) -> usize {
        self.shown
            .get_non_reactive()
            .into_iter()
            .filter(|shown| *shown)
            .count()
    }

    // Whether any hand shows a model. A composable that calls it composes again when that changes.
    pub(crate) fn any_shown(&self) -> bool {
        self.shown.get().into_iter().any(|shown| shown)
    }

    // Whether `hand` shows a model. A composable that calls it composes again when that changes.
    pub(crate) fn is_shown(&self, hand: Hand) -> bool {
        self.shown.get()[hand.index()]
    }

    // The map from `hand`'s model to the scene, a state that the draw reads.
    pub(crate) fn map(&self, hand: Hand) -> MutableState<Mat4> {
        self.maps[hand.index()]
    }

    // The model that the app set for `hand`. A composable that calls it composes again when the app
    // changes it.
    pub(crate) fn setting(&self, hand: Hand) -> ControllerModel {
        self.settings
            .read(|settings| settings.models[hand.index()].clone())
    }

    // The runtime's model of `hand`'s controller. A composable that calls it composes again when
    // the backend records another.
    pub(crate) fn offer(&self, hand: Hand) -> Offer {
        self.offers.read(|offers| offers[hand.index()].clone())
    }

    // What `hand` shows now. A composable that calls it composes again when the choice changes,
    // such as when a model has loaded.
    pub(crate) fn look(&self, hand: Hand) -> Look {
        look(&self.setting(hand), &self.offer(hand), self.load(hand))
    }

    // How far the load of the model that `hand` wants has come.
    #[cfg(feature = "gltf")]
    fn load(&self, hand: Hand) -> Option<Load> {
        use crate::SpatialGltfModelStatus as Status;
        let state = self.loads.read(|loads| loads[hand.index()].clone())?;
        Some(match state.status() {
            Status::Loading => Load::Loading,
            Status::Loaded => Load::Ready,
            Status::Failed(_) => Load::Failed,
        })
    }

    #[cfg(not(feature = "gltf"))]
    fn load(&self, _hand: Hand) -> Option<Load> {
        None
    }

    // Records the state of the glTF model that `hand` loads, or that it loads none.
    #[cfg(feature = "gltf")]
    pub(crate) fn set_load(&self, hand: Hand, state: Option<crate::SpatialGltfModelState>) {
        self.loads.update(|loads| loads[hand.index()] = state);
    }

    // The glTF model that `hand` has loaded, once it has.
    #[cfg(feature = "gltf")]
    pub(crate) fn loaded(
        &self,
        hand: Hand,
    ) -> Option<std::rc::Rc<crate::spatial_gltf_model_state::LoadedModel>> {
        self.loads
            .read(|loads| loads[hand.index()].clone())?
            .loaded()
    }
}
