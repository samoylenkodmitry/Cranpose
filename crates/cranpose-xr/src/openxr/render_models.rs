//! The runtime's models of the controllers, through `XR_FB_render_model`.
//!
//! The runtime hands out a glTF binary for each controller. The backend asks once for each hand,
//! before the first frame that the runtime waits for. It asks again when a hand's controller comes
//! back after the runtime had no model for it. It copies the bytes out of the runtime. The loader
//! decodes them on a worker thread.

use std::sync::Arc;

use cranpose::capabilities::{Capabilities, Service};
use openxr as xr;

use super::{render_model_calls, session::XrSession};
use crate::{
    Hand, InteractionProfile, controller_look::Offer, controller_states::ControllerStates,
    model_asks::ModelAsks, model_path::controller_path,
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

pub(crate) struct RenderModels {
    // Whether the instance has the extension
    enabled: bool,
    asks: ModelAsks,
}

impl RenderModels {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            enabled,
            asks: ModelAsks::new(),
        }
    }

    // Tells the scene what the runtime offers before any question. The instance without the
    // extension offers nothing. The instance with it has an answer to come.
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
        let due = self.asks.due(profiles);
        for hand in Hand::BOTH {
            if !due[hand.index()] {
                continue;
            }
            let offer = match ask(xr_session, hand) {
                Ok(Some(bytes)) => Offer::Bytes(bytes),
                Ok(None) => Offer::Absent,
                Err(error) => {
                    log::warn!("cranpose xr: the {hand:?} controller's model: {error}");
                    Offer::Absent
                }
            };
            self.asks.answered(hand, &offer);
            offers[hand.index()] = Some(offer);
        }
        offers
    }
}

// The glTF binary of `hand`'s controller, or `None` while the runtime has none to give.
fn ask(xr_session: &XrSession, hand: Hand) -> Result<Option<Arc<[u8]>>, String> {
    let session = &xr_session.session;
    let instance = xr_session.instance();
    let listed = render_model_calls::paths(session, instance)?;
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
        let found = render_model_calls::properties(session, instance, listed[chosen], flags)?;
        if found.key == xr::RenderModelKeyFB::NULL {
            continue;
        }
        let bytes = session
            .load_render_model_fb(found.key)
            .map_err(|error| format!("loading the render model: {error}"))?;
        if bytes.is_empty() {
            continue;
        }
        log::info!(
            "cranpose xr: the {hand:?} controller's model \"{}\", vendor {:#x}, version {}, flags {:?} for {flags:?}, {} bytes",
            found.name,
            found.vendor,
            found.version,
            found.flags,
            bytes.len()
        );
        return Ok(Some(Arc::from(bytes)));
    }
    Ok(None)
}
