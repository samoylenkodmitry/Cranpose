//! The loads of the glTF models that the controllers show. The runtime's models and the app's own
//! models load here.
//!
//! A load starts when a hand wants a source. It stays while the source stays, whether or not a
//! controller is in use. A controller that leaves and comes back therefore finds its model loaded.
//! The work runs on a worker thread, as `rememberSpatialGltfModelState` runs it.

use cranpose::composable;
use cranpose_core::{SideEffect, key};

use crate::{
    Hand, SpatialGltfModelSource, controller_look::source, controller_states::ControllerStates,
    rememberSpatialGltfModelState,
};

// Loads what each hand wants and tells the states how far each load has come.
#[composable]
pub(crate) fn ControllerLoaders(states: ControllerStates) {
    for hand in Hand::BOTH {
        key(hand.index(), move || HandLoad(states, hand));
    }
}

#[composable]
fn HandLoad(states: ControllerStates, hand: Hand) {
    match source(&states.setting(hand), &states.offer(hand)) {
        Some(source) => Loading(states, hand, source),
        None => SideEffect(move || states.set_load(hand, None)),
    }
}

#[composable]
fn Loading(states: ControllerStates, hand: Hand, source: SpatialGltfModelSource) {
    let state = rememberSpatialGltfModelState(source);
    SideEffect(move || states.set_load(hand, Some(state)));
}
