//! The runtime's models of the controllers, through `XR_FB_render_model`.
//!
//! The runtime hands out a glTF binary for each controller. The backend asks once for each hand,
//! before the first frame that the runtime waits for, and again when a hand's controller comes
//! back after the runtime had no model for it. It copies the bytes out of the runtime and leaves
//! the decoding to the loader, which runs on a worker thread.

use std::sync::Arc;

use cranpose::capabilities::{Capabilities, Service};
use openxr as xr;

use super::session::XrSession;
use crate::{
    Hand, InteractionProfile, controller_look::Offer, controller_states::ControllerStates,
    model_path::controller_path,
};

// Whether the app declared `Use::controller_models()` in its build, which writes the manifest
// entries that make the Horizon OS runtime list the extension to the app.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ModelRequest {
    asked: bool,
}

impl ModelRequest {
    pub(crate) fn of(capabilities: &Capabilities<'_>) -> Self {
        Self {
            asked: capabilities.has(Service::ControllerModels),
        }
    }

    // Whether the instance enables the extension, if the runtime lists it. A build with no glTF
    // loader has no use for the runtime's glTF files.
    pub(crate) const fn enables(self) -> bool {
        self.asked && cfg!(feature = "gltf")
    }
}

// How far the query of one hand has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    // The runtime has not been asked
    Untried,
    // The runtime had no model that it could give: it listed no path, said that the model is not
    // available yet, or failed. Another controller in the hand makes it ask again.
    Waiting,
    // The runtime gave its model
    Done,
}

pub(crate) struct RenderModels {
    // Whether the instance has the extension
    enabled: bool,
    steps: [Step; 2],
    // Whether each hand held a controller when the backend last looked
    held: [bool; 2],
}

impl RenderModels {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            steps: [Step::Untried; 2],
            held: [false; 2],
        }
    }

    // Tells the scene what the runtime offers before any question: nothing where the instance lacks
    // the extension, and an answer to come where it has it.
    pub(crate) fn announce(&self, states: ControllerStates) {
        let first = if self.enabled {
            Offer::Pending
        } else {
            Offer::Absent
        };
        for hand in Hand::BOTH {
            states.set_offer(hand, first.clone());
        }
    }

    // Asks the runtime for the model of each hand that is due, given the interaction profile of each
    // hand's controller as the last frame reported it. Returns the new offer of each hand that the
    // question answered. A call with nothing due touches no runtime.
    pub(crate) fn query(
        &mut self,
        xr_session: &XrSession,
        profiles: [Option<InteractionProfile>; 2],
    ) -> [Option<Offer>; 2] {
        let mut offers = [None, None];
        if !self.enabled {
            return offers;
        }
        for hand in Hand::BOTH {
            let index = hand.index();
            let held = profiles[index].is_some_and(|profile| !profile.is_hand());
            let due = match self.steps[index] {
                Step::Untried => true,
                Step::Waiting => held && !self.held[index],
                Step::Done => false,
            };
            self.held[index] = held;
            if !due {
                continue;
            }
            let (step, offer) = match ask(xr_session, hand) {
                Ok(Some(bytes)) => (Step::Done, Offer::Bytes(bytes)),
                Ok(None) => (Step::Waiting, Offer::Absent),
                Err(error) => {
                    log::warn!("cranpose xr: the {hand:?} controller's model: {error}");
                    (Step::Waiting, Offer::Absent)
                }
            };
            self.steps[index] = step;
            offers[index] = Some(offer);
        }
        offers
    }
}

// The glTF binary of `hand`'s controller, or `None` while the runtime has none to give.
fn ask(xr_session: &XrSession, hand: Hand) -> Result<Option<Arc<[u8]>>, String> {
    let session = &xr_session.session;
    let instance = xr_session.instance();
    let listed = session
        .enumerate_render_model_paths_fb()
        .map_err(|error| format!("render model paths: {error}"))?;
    let paths: Vec<String> = listed
        .iter()
        .map(|path| instance.path_to_string(*path).unwrap_or_default())
        .collect();
    log::info!("cranpose xr: the runtime lists render models {paths:?}");
    let Some(chosen) = controller_path(&paths, hand) else {
        return Ok(None);
    };
    // The plain subset first: a model that needs nothing of the loader's but the glTF core. A
    // runtime that holds only the richer subset gets the second request.
    let plain = xr::RenderModelFlagsFB::SUPPORTS_GLTF_2_0_SUBSET_1;
    let both = plain | xr::RenderModelFlagsFB::SUPPORTS_GLTF_2_0_SUBSET_2;
    for flags in [plain, both] {
        let properties = session
            .get_render_model_properties_fb(listed[chosen], flags)
            .map_err(|error| format!("render model properties: {error}"))?;
        if properties.model_key == xr::RenderModelKeyFB::NULL {
            continue;
        }
        let bytes = session
            .load_render_model_fb(properties.model_key)
            .map_err(|error| format!("loading the render model: {error}"))?;
        if bytes.is_empty() {
            continue;
        }
        log::info!(
            "cranpose xr: the {hand:?} controller's model \"{}\", vendor {:#x}, version {}, flags {:?} for {flags:?}, {} bytes",
            properties.model_name,
            properties.vendor_id,
            properties.model_version,
            properties.flags,
            bytes.len()
        );
        return Ok(Some(Arc::from(bytes)));
    }
    Ok(None)
}
