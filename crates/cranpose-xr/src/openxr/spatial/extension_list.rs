//! The anchor and plane extensions of the instance, as plain flags.

use openxr as xr;

use crate::spatial_offer::SpatialOffer;

// The anchor and plane extensions that the runtime lists.
pub(crate) fn listed(available: &xr::ExtensionSet) -> SpatialOffer {
    SpatialOffer {
        fb_spatial_entity: available.fb_spatial_entity,
        fb_spatial_entity_query: available.fb_spatial_entity_query,
        fb_spatial_entity_storage: available.fb_spatial_entity_storage,
        fb_scene: available.fb_scene,
        android_trackables: android_trackables(available),
        android_device_anchor_persistence: android_persistence(available),
    }
}

// Turns on the anchor and plane extensions of `enabled`. It leaves the others as they are, so an
// app that asked for nothing enables none.
pub(crate) fn enable(extensions: &mut xr::ExtensionSet, enabled: SpatialOffer) {
    extensions.fb_spatial_entity = enabled.fb_spatial_entity;
    extensions.fb_spatial_entity_query = enabled.fb_spatial_entity_query;
    extensions.fb_spatial_entity_storage = enabled.fb_spatial_entity_storage;
    extensions.fb_scene = enabled.fb_scene;
    enable_android(extensions, enabled);
}

// Only an Android runtime has the Android extensions, and the binding has the fields only there.
#[cfg(target_os = "android")]
fn android_trackables(available: &xr::ExtensionSet) -> bool {
    available.android_trackables
}

#[cfg(not(target_os = "android"))]
fn android_trackables(_available: &xr::ExtensionSet) -> bool {
    false
}

#[cfg(target_os = "android")]
fn android_persistence(available: &xr::ExtensionSet) -> bool {
    available.android_device_anchor_persistence
}

#[cfg(not(target_os = "android"))]
fn android_persistence(_available: &xr::ExtensionSet) -> bool {
    false
}

#[cfg(target_os = "android")]
fn enable_android(extensions: &mut xr::ExtensionSet, enabled: SpatialOffer) {
    extensions.android_trackables = enabled.android_trackables;
    extensions.android_device_anchor_persistence = enabled.android_device_anchor_persistence;
}

#[cfg(not(target_os = "android"))]
fn enable_android(_extensions: &mut xr::ExtensionSet, _enabled: SpatialOffer) {}
