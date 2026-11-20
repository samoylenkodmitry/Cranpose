//! What a hand shows in place of its controller.
//!
//! The choice follows from the app's setting and the runtime's offer. The state of the load of the
//! model that the hand wants also counts. A controller in use never goes without a picture: a model
//! that cannot be had ends at the plain one. Only a model that is still on its way leaves the hand
//! with nothing for a moment.

use std::sync::Arc;

use crate::ControllerModel;

// The runtime's model of one hand's controller, as the backend found it.
#[derive(Clone, Debug, Default)]
pub(crate) enum Offer {
    // The backend has not asked the runtime yet
    #[default]
    Pending,
    // The runtime has no model for the hand's controller, or the app did not ask for the runtime's
    // models, or the build has no glTF loader to read one with
    Absent,
    // The runtime's model: the bytes of a glTF binary
    Bytes(Arc<[u8]>),
}

// Two offers are the same when they have the same bytes: the backend reads a model once.
impl PartialEq for Offer {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Pending, Self::Pending) | (Self::Absent, Self::Absent) => true,
            (Self::Bytes(a), Self::Bytes(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

// How far the load of the model that a hand wants has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(any(feature = "gltf", test)),
    expect(dead_code, reason = "only a glTF model has a load")
)]
pub(crate) enum Load {
    Loading,
    Ready,
    Failed,
}

// What a hand's controller shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    // No picture: the model is on its way, or nothing asks for one
    Nothing,
    // The plain model that Cranpose builds
    Plain,
    // The glTF model that the hand wanted, which has loaded
    Model,
}

// What `setting` shows for a hand whose runtime model is `offer`. `load` is the state of the load
// of the glTF source that the hand wants, or `None` while none has begun.
pub(crate) fn look(setting: &ControllerModel, offer: &Offer, load: Option<Load>) -> Look {
    match setting {
        ControllerModel::Plain => Look::Plain,
        ControllerModel::Runtime => match offer {
            Offer::Pending => Look::Nothing,
            Offer::Absent => Look::Plain,
            Offer::Bytes(_) => of_load(load),
        },
        #[cfg(feature = "gltf")]
        ControllerModel::Gltf(_) => of_load(load),
    }
}

fn of_load(load: Option<Load>) -> Look {
    match load {
        None | Some(Load::Loading) => Look::Nothing,
        Some(Load::Ready) => Look::Model,
        Some(Load::Failed) => Look::Plain,
    }
}

// The glTF source that a hand loads: the runtime's model while the hand takes it and the runtime
// offered it, or the app's own model.
#[cfg(feature = "gltf")]
pub(crate) fn source(
    setting: &ControllerModel,
    offer: &Offer,
) -> Option<crate::SpatialGltfModelSource> {
    match (setting, offer) {
        (ControllerModel::Runtime, Offer::Bytes(bytes)) => Some(
            // The same bytes make an equal source, so the state that loads them stays
            crate::SpatialGltfModelSource::from_bytes(Arc::clone(bytes)),
        ),
        (ControllerModel::Gltf(source), _) => Some(source.clone()),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/controller_look_tests.rs"]
mod tests;
