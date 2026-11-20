//! The connection to the OpenXR runtime: the loader, the instance and the headset's system.
#![expect(unsafe_code, reason = "OpenXR's loader, found and loaded at run time")]

use openxr as xr;

use super::{
    graphics::Graphics,
    host::Host,
    tracking::{self, TrackingSetup},
};
use crate::{TrackingCapabilities, tracking_request::TrackingRequest};

// An OpenXR instance on a system with a headset.
pub(crate) struct Runtime {
    pub(crate) instance: xr::Instance,
    pub(crate) system: xr::SystemId,
    // Whether layers can say that their images start with the top row
    pub(crate) flips_layers: bool,
    // Whether the runtime shows cylinder layers, which curved panels are
    pub(crate) cylinders: bool,
    // Whether a projection layer can carry the depth of its pictures
    // (`XR_KHR_composition_layer_depth`)
    pub(crate) layer_depth: bool,
    // Whether the layers over a projection layer can test against its depth
    // (`XR_FB_composition_layer_depth_test`)
    pub(crate) depth_test: bool,
    // The tracking that the app asked for, and what the runtime offers of it
    pub(crate) tracking: TrackingSetup,
}

impl Runtime {
    // Loads the OpenXR loader, makes an instance and finds the headset. A machine with no
    // loader, no runtime or no headset gets an error that says which. The instance enables the
    // extensions of the tracking that `request` asks for, and no others.
    pub(crate) fn connect(host: &Host, request: TrackingRequest) -> Result<Self, String> {
        // SAFETY: the library is the Khronos OpenXR loader: the `xr` service of the Gradle plugin
        // packs it into an APK, and a desktop has it installed with its runtime.
        let entry = unsafe { xr::Entry::load(&host.platform()) }
            .map_err(|error| format!("the OpenXR loader: {error}"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(|error| format!("OpenXR extensions: {error}"))?;
        let mut extensions = wanted_extensions(&available);
        let listed = tracking::listed(&available);
        let enabled = listed.enabled_for(request);
        tracking::enable(&mut extensions, enabled);
        // The prompt comes before the session, and the instance is made after it
        tracking::ask_before_session(host, enabled);
        Graphics::require(&available, &mut extensions)?;
        host.require(&available, &mut extensions);
        if !host.await_activity(&available) {
            return Err("the activity ended before the runtime started".into());
        }
        log::info!(
            "cranpose xr: cylinder layers {}, passthrough extension {}, layer depth {}, depth test {}, image layout {}",
            extensions.khr_composition_layer_cylinder,
            extensions.fb_passthrough,
            extensions.khr_composition_layer_depth,
            extensions.fb_composition_layer_depth_test,
            extensions.fb_composition_layer_image_layout,
        );
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
        log_runtime(&instance, system);
        let support = tracking::query(&instance, system, enabled);
        let capabilities = TrackingCapabilities::from_runtime(request, listed, support);
        log::info!(
            "cranpose xr: the runtime lists {listed:?}; asked for {request:?}; tracking {capabilities:?}"
        );
        Ok(Self {
            instance,
            system,
            flips_layers: available.fb_composition_layer_image_layout,
            cylinders: extensions.khr_composition_layer_cylinder,
            layer_depth: extensions.khr_composition_layer_depth,
            depth_test: extensions.fb_composition_layer_depth_test,
            tracking: TrackingSetup {
                request,
                enabled,
                capabilities,
            },
        })
    }
}

// The extensions the app asks for, from those the runtime has.
fn wanted_extensions(available: &xr::ExtensionSet) -> xr::ExtensionSet {
    let mut extensions = xr::ExtensionSet::default();
    extensions.fb_composition_layer_image_layout = available.fb_composition_layer_image_layout;
    // Curved panels show on cylinder layers
    extensions.khr_composition_layer_cylinder = available.khr_composition_layer_cylinder;
    // The 3D content's projection layer carries its depth, and the layers over it test against it
    extensions.khr_composition_layer_depth = available.khr_composition_layer_depth;
    extensions.fb_composition_layer_depth_test = available.fb_composition_layer_depth_test;
    // The room around the panels shows through a passthrough layer
    extensions.fb_passthrough = available.fb_passthrough;
    // The runtime's times on the platform's monotonic clock
    #[cfg(unix)]
    {
        extensions.khr_convert_timespec_time = available.khr_convert_timespec_time;
    }
    #[cfg(windows)]
    {
        extensions.khr_win32_convert_performance_counter_time =
            available.khr_win32_convert_performance_counter_time;
    }
    // The highest refresh rate the runtime offers
    extensions.fb_display_refresh_rate = available.fb_display_refresh_rate;
    // Hands tracked by cameras point and pinch as controllers do. The controllers of other
    // makers bind through their own profiles, each behind an extension.
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

// Names the runtime and the headset once in the log, so a report says which runtime it came from.
fn log_runtime(instance: &xr::Instance, system: xr::SystemId) {
    match instance.properties() {
        Ok(runtime) => log::info!(
            "cranpose xr: runtime \"{}\" {}",
            runtime.runtime_name,
            runtime.runtime_version
        ),
        Err(error) => log::warn!("cranpose xr: runtime properties: {error}"),
    }
    match instance.system_properties(system) {
        Ok(headset) => log::info!(
            "cranpose xr: system \"{}\", vendor {:#x}, swapchains up to {} by {} pixels, {} layers",
            headset.system_name,
            headset.vendor_id,
            headset.graphics_properties.max_swapchain_image_width,
            headset.graphics_properties.max_swapchain_image_height,
            headset.graphics_properties.max_layer_count
        ),
        Err(error) => log::warn!("cranpose xr: system properties: {error}"),
    }
}
