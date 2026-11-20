//! The controllers' models in the scene that draws the volumes.
//!
//! A hand that shows a model has one node tree in the scene, beside the volumes' content. The
//! tree's surfaces carry the placement of the controller's grip. The frame loop moves a model by
//! writing one state, which the draw reads. A hand with no controller in use has no tree.

use cranpose::composable;
use cranpose_core::key;

use crate::{
    Hand, controller_look::Look, controller_states::ControllerStates,
    plain_controller::PlainController,
};
#[cfg(feature = "gltf")]
use crate::{gltf_placement::GltfPlacement, spatial_gltf_model::ModelLayers};

// The models of the hands that show one, in the scene's composition.
#[composable]
pub(crate) fn ControllerNodes(states: ControllerStates) {
    for hand in Hand::BOTH {
        key(hand.index(), move || ControllerNode(states, hand));
    }
}

#[composable]
fn ControllerNode(states: ControllerStates, hand: Hand) {
    if !states.is_shown(hand) {
        return;
    }
    let look = states.look(hand);
    log::info!("cranpose xr: the {hand:?} controller shows {look:?}");
    match look {
        Look::Nothing => {}
        Look::Plain => PlainController(states.map(hand)),
        Look::Model => model(states, hand),
    }
}

// The glTF model that the hand loaded, at the controller.
#[composable]
fn model(states: ControllerStates, hand: Hand) {
    #[cfg(feature = "gltf")]
    if let Some(loaded) = states.loaded(hand) {
        ModelLayers(loaded, GltfPlacement::Moving(states.map(hand)));
    }
    #[cfg(not(feature = "gltf"))]
    let _ = (states, hand);
}
