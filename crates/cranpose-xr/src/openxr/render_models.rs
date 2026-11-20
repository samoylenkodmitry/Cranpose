//! The runtime's models of the controllers, through `XR_FB_render_model`.
//!
//! The runtime hands out a glTF binary for each controller. The backend asks once for each hand,
//! before the first frame that the runtime waits for. It asks again when a hand's controller comes
//! back after the runtime had no model for it. It copies the bytes out of the runtime. The loader
//! decodes them on a worker thread.

use std::sync::Arc;

use cranpose::capabilities::{Capabilities, Service};
use openxr as xr;

use super::session::XrSession;
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
        #[cfg(feature = "gltf")]
        log_summary(hand, &bytes);
        return Ok(Some(Arc::from(bytes)));
    }
    Ok(None)
}

// TEMPORARY: what a runtime's model holds, for the first report from a headset.
#[cfg(feature = "gltf")]
fn log_summary(hand: Hand, bytes: &[u8]) {
    let gltf = match gltf::Gltf::from_slice(bytes) {
        Ok(gltf) => gltf,
        Err(error) => {
            log::warn!("cranpose xr: model {hand:?} does not parse: {error}");
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2000)]).into_owned();
            log::info!("cranpose xr: model {hand:?} head: {head:?}");
            return;
        }
    };
    let document = &gltf.document;
    log::info!(
        "cranpose xr: model {hand:?}: {} nodes, {} meshes, {} materials, {} images, {} skins, {} animations, {} textures, extensions used {:?}, required {:?}",
        document.nodes().count(),
        document.meshes().count(),
        document.materials().count(),
        document.images().count(),
        document.skins().count(),
        document.animations().count(),
        document.textures().count(),
        document.extensions_used().collect::<Vec<_>>(),
        document.extensions_required().collect::<Vec<_>>(),
    );
    for node in document.nodes() {
        log::info!(
            "cranpose xr: model {hand:?} node {} {:?} mesh {:?} skin {} children {:?} t {:?}",
            node.index(),
            node.name(),
            node.mesh().map(|mesh| mesh.index()),
            node.skin().is_some(),
            node.children()
                .map(|child| child.index())
                .collect::<Vec<_>>(),
            node.transform().decomposed().0,
        );
    }
    for animation in document.animations() {
        log::info!(
            "cranpose xr: model {hand:?} animation {:?}: {} channels",
            animation.name(),
            animation.channels().count()
        );
    }
    for image in document.images() {
        match image.source() {
            gltf::image::Source::View { mime_type, .. } => {
                log::info!(
                    "cranpose xr: model {hand:?} image {} {mime_type}",
                    image.index()
                );
            }
            gltf::image::Source::Uri { uri, mime_type } => {
                log::info!(
                    "cranpose xr: model {hand:?} image {} {mime_type:?} uri {}",
                    image.index(),
                    &uri[..uri.len().min(40)]
                );
            }
        }
    }
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        log::info!(
            "cranpose xr: model {hand:?} material {:?} color {:?} texture {}",
            material.name(),
            pbr.base_color_factor(),
            pbr.base_color_texture().is_some()
        );
    }
    for mesh in document.meshes() {
        for primitive in mesh.primitives() {
            let (low, high) = (primitive.bounding_box().min, primitive.bounding_box().max);
            log::info!(
                "cranpose xr: model {hand:?} mesh {} primitive {} box {low:?} {high:?} attributes {:?}",
                mesh.index(),
                primitive.index(),
                primitive
                    .attributes()
                    .map(|(semantic, _)| format!("{semantic:?}"))
                    .collect::<Vec<_>>(),
            );
        }
    }
}
