//! The connection to the OpenXR runtime: the loader, the instance and the headset's system.
#![expect(unsafe_code, reason = "OpenXR's loader, found and loaded at run time")]

use openxr as xr;

use super::{graphics::Graphics, host::Host};

// An OpenXR instance on a system with a headset.
pub(crate) struct Runtime {
    pub(crate) instance: xr::Instance,
    pub(crate) system: xr::SystemId,
    // Whether layers can say that their images start with the top row
    pub(crate) flips_layers: bool,
}

impl Runtime {
    // Loads the OpenXR loader, makes an instance and finds the headset. A machine with no
    // loader, no runtime or no headset gets an error that says which.
    pub(crate) fn connect(host: &Host) -> Result<Self, String> {
        // SAFETY: the library is the Khronos OpenXR loader: the `xr` service of the Gradle plugin
        // packs it into an APK, and a desktop has it installed with its runtime.
        let entry = unsafe { xr::Entry::load(&host.platform()) }
            .map_err(|error| format!("the OpenXR loader: {error}"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(|error| format!("OpenXR extensions: {error}"))?;
        let mut extensions = input_extensions(&available);
        extensions.fb_composition_layer_image_layout = available.fb_composition_layer_image_layout;
        Graphics::require(&available, &mut extensions)?;
        host.require(&mut extensions);
        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    application_name: "cranpose",
                    application_version: 0,
                    engine_name: "Cranpose",
                    engine_version: 0,
                    api_version: xr::Version::new(1, 0, 0),
                },
                &extensions,
                &[],
                &host.platform(),
            )
            .map_err(|error| format!("OpenXR instance: {error}"))?;
        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|error| format!("no headset: {error}"))?;
        Ok(Self {
            instance,
            system,
            flips_layers: available.fb_composition_layer_image_layout,
        })
    }
}

// The extensions that the controllers of each maker bind through, where the runtime has them.
// Hands tracked by cameras point and pinch as controllers do. The controllers of other makers
// bind through their own profiles, each behind an extension.
fn input_extensions(available: &xr::ExtensionSet) -> xr::ExtensionSet {
    let mut extensions = xr::ExtensionSet::default();
    extensions.ext_hand_interaction = available.ext_hand_interaction;
    extensions.khr_generic_controller = available.khr_generic_controller;
    extensions.htc_vive_focus3_controller_interaction =
        available.htc_vive_focus3_controller_interaction;
    extensions.htc_vive_cosmos_controller_interaction =
        available.htc_vive_cosmos_controller_interaction;
    extensions.ext_hp_mixed_reality_controller = available.ext_hp_mixed_reality_controller;
    extensions.ext_samsung_odyssey_controller = available.ext_samsung_odyssey_controller;
    extensions.bd_controller_interaction = available.bd_controller_interaction;
    extensions.ml_ml2_controller_interaction = available.ml_ml2_controller_interaction;
    extensions.meta_touch_controller_plus = available.meta_touch_controller_plus;
    extensions.fb_touch_controller_pro = available.fb_touch_controller_pro;
    extensions
}
