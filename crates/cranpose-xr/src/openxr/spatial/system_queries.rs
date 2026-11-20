//! What the headset's system says of the anchor extensions that the instance enabled.
#![expect(
    unsafe_code,
    reason = "xrGetSystemProperties with a chain of property structs on the stack, because the safe wrapper reads the basic properties only"
)]

use std::{ffi::c_void, ptr};

use openxr as xr;

use super::super::tracking::fetch;
use crate::{spatial_offer::SpatialOffer, spatial_support::SpatialSupport};

// Asks the system whether it supports each enabled extension's feature. A runtime answers the
// property structure of an extension only while that extension is enabled, so the chain holds the
// enabled ones, and the others stay `None`. A runtime that fails the call leaves every answer
// `None`, and the list of extensions decides alone.
pub(crate) fn query(
    instance: &xr::Instance,
    system: xr::SystemId,
    enabled: SpatialOffer,
) -> SpatialSupport {
    let mut meta = xr::sys::SystemSpatialEntityPropertiesFB {
        ty: xr::sys::SystemSpatialEntityPropertiesFB::TYPE,
        next: ptr::null_mut(),
        supports_spatial_entity: false.into(),
    };
    let mut head: *mut c_void = ptr::null_mut();
    if enabled.fb_spatial_entity {
        head = ptr::from_mut(&mut meta).cast();
    }
    // SAFETY: the struct of the chain is initialized with its own type, outlives the call and
    // belongs to an extension that the instance enabled.
    let result = unsafe { fetch(instance, system, head) };
    if result.into_raw() < 0 {
        log::warn!("cranpose xr: the system's anchor properties: {result:?}");
        return SpatialSupport::default();
    }
    let android = android::query(instance, system, enabled);
    SpatialSupport {
        meta_anchors: enabled
            .fb_spatial_entity
            .then(|| meta.supports_spatial_entity.into()),
        ..android
    }
}

#[cfg(target_os = "android")]
mod android {
    use std::ptr;

    use openxr as xr;

    use super::{SpatialOffer, SpatialSupport, fetch};

    // Android XR's answers: whether it creates anchors, how many at a time and whether it keeps
    // them. Each structure goes in a call of its own.
    pub(super) fn query(
        instance: &xr::Instance,
        system: xr::SystemId,
        enabled: SpatialOffer,
    ) -> SpatialSupport {
        let mut support = SpatialSupport::default();
        if enabled.android_trackables {
            let mut trackables = xr::sys::SystemTrackablesPropertiesANDROID {
                ty: xr::sys::SystemTrackablesPropertiesANDROID::TYPE,
                next: ptr::null(),
                supports_anchor: false.into(),
                max_anchors: 0,
            };
            // SAFETY: the struct is initialized with its type, outlives the call and belongs to an
            // extension that the instance enabled.
            let result = unsafe { fetch(instance, system, ptr::from_mut(&mut trackables).cast()) };
            if result.into_raw() >= 0 {
                support.android_anchors = Some(trackables.supports_anchor.into());
                support.android_max_anchors = Some(trackables.max_anchors);
            }
        }
        if enabled.android_device_anchor_persistence {
            let mut persistence = xr::sys::SystemDeviceAnchorPersistencePropertiesANDROID {
                ty: xr::sys::SystemDeviceAnchorPersistencePropertiesANDROID::TYPE,
                next: ptr::null_mut(),
                supports_anchor_persistence: false.into(),
            };
            // SAFETY: as above, for the persistence extension.
            let result = unsafe { fetch(instance, system, ptr::from_mut(&mut persistence).cast()) };
            if result.into_raw() >= 0 {
                support.android_persistence = Some(persistence.supports_anchor_persistence.into());
            }
        }
        support
    }
}

#[cfg(not(target_os = "android"))]
mod android {
    use openxr as xr;

    use super::{SpatialOffer, SpatialSupport};

    // Only an Android runtime answers these.
    pub(super) fn query(
        _instance: &xr::Instance,
        _system: xr::SystemId,
        _enabled: SpatialOffer,
    ) -> SpatialSupport {
        SpatialSupport::default()
    }
}
