//! What the headset's system says of the tracking extensions that the instance enabled.
#![expect(
    unsafe_code,
    reason = "xrGetSystemProperties with a chain of property structs on the stack, because the safe wrapper reads the basic properties only"
)]

use std::{ffi::c_void, ptr};

use openxr as xr;

use crate::{extension_offer::ExtensionOffer, system_support::SystemSupport};

// Asks the system whether it supports each enabled extension's feature. A runtime answers the
// property structure of an extension only while that extension is enabled, so the chain holds the
// enabled ones, and the others stay `None`. A runtime that fails the call leaves every answer
// `None`, and the list of extensions decides alone.
pub(crate) fn query(
    instance: &xr::Instance,
    system: xr::SystemId,
    enabled: ExtensionOffer,
) -> SystemSupport {
    let mut hand = xr::sys::SystemHandTrackingPropertiesEXT {
        ty: xr::sys::SystemHandTrackingPropertiesEXT::TYPE,
        next: ptr::null_mut(),
        supports_hand_tracking: false.into(),
    };
    let mut body = xr::sys::SystemBodyTrackingPropertiesFB {
        ty: xr::sys::SystemBodyTrackingPropertiesFB::TYPE,
        next: ptr::null_mut(),
        supports_body_tracking: false.into(),
    };
    let mut full_body = xr::sys::SystemPropertiesBodyTrackingFullBodyMETA {
        ty: xr::sys::SystemPropertiesBodyTrackingFullBodyMETA::TYPE,
        next: ptr::null_mut(),
        supports_full_body_tracking: false.into(),
    };
    let mut meta_face = xr::sys::SystemFaceTrackingProperties2FB {
        ty: xr::sys::SystemFaceTrackingProperties2FB::TYPE,
        next: ptr::null_mut(),
        supports_visual_face_tracking: false.into(),
        supports_audio_face_tracking: false.into(),
    };
    let mut gaze = xr::sys::SystemEyeGazeInteractionPropertiesEXT {
        ty: xr::sys::SystemEyeGazeInteractionPropertiesEXT::TYPE,
        next: ptr::null_mut(),
        supports_eye_gaze_interaction: false.into(),
    };
    let mut social = xr::sys::SystemEyeTrackingPropertiesFB {
        ty: xr::sys::SystemEyeTrackingPropertiesFB::TYPE,
        next: ptr::null_mut(),
        supports_eye_tracking: false.into(),
    };
    // A struct joins at the front: its `next` takes the old head, and it becomes the head
    let mut head: *mut c_void = ptr::null_mut();
    if enabled.hand_tracking {
        hand.next = head;
        head = ptr::from_mut(&mut hand).cast();
    }
    if enabled.body_tracking {
        body.next = head;
        head = ptr::from_mut(&mut body).cast();
    }
    if enabled.full_body_tracking {
        full_body.next = head;
        head = ptr::from_mut(&mut full_body).cast();
    }
    if enabled.face_tracking2 {
        meta_face.next = head;
        head = ptr::from_mut(&mut meta_face).cast();
    }
    if enabled.eye_gaze_interaction {
        gaze.next = head;
        head = ptr::from_mut(&mut gaze).cast();
    }
    if enabled.eye_tracking_social {
        social.next = head;
        head = ptr::from_mut(&mut social).cast();
    }
    // SAFETY: every struct of the chain is initialized with its own type, outlives the call and
    // belongs to an extension that the instance enabled.
    let result = unsafe { fetch(instance, system, head) };
    if result.into_raw() < 0 {
        log::warn!("cranpose xr: the system's tracking properties: {result:?}");
        return SystemSupport::default();
    }
    let said = |enabled: bool, value: bool| enabled.then_some(value);
    SystemSupport {
        hand_tracking: said(enabled.hand_tracking, hand.supports_hand_tracking.into()),
        body_tracking: said(enabled.body_tracking, body.supports_body_tracking.into()),
        full_body_tracking: said(
            enabled.full_body_tracking,
            full_body.supports_full_body_tracking.into(),
        ),
        face_tracking2: said(
            enabled.face_tracking2,
            bool::from(meta_face.supports_visual_face_tracking)
                || bool::from(meta_face.supports_audio_face_tracking),
        ),
        android_face_tracking: android_face_support(
            instance,
            system,
            enabled.android_face_tracking,
        ),
        eye_gaze_interaction: said(
            enabled.eye_gaze_interaction,
            gaze.supports_eye_gaze_interaction.into(),
        ),
        eye_tracking_social: said(
            enabled.eye_tracking_social,
            social.supports_eye_tracking.into(),
        ),
    }
}

// Calls xrGetSystemProperties with `next` as the chain of property structs behind the basic ones.
//
// # Safety
//
// `next` is null, or the head of a chain of structs that are initialized with their types, outlive
// the call and belong to extensions that the instance enabled.
pub(in crate::openxr) unsafe fn fetch(
    instance: &xr::Instance,
    system: xr::SystemId,
    next: *mut c_void,
) -> xr::sys::Result {
    let mut properties = xr::sys::SystemProperties {
        ty: xr::sys::SystemProperties::TYPE,
        next,
        system_id: system,
        vendor_id: 0,
        system_name: [0; xr::sys::MAX_SYSTEM_NAME_SIZE],
        graphics_properties: xr::sys::SystemGraphicsProperties {
            max_swapchain_image_height: 0,
            max_swapchain_image_width: 0,
            max_layer_count: 0,
        },
        tracking_properties: xr::sys::SystemTrackingProperties {
            orientation_tracking: false.into(),
            position_tracking: false.into(),
        },
    };
    // SAFETY: the instance handle is live, `properties` is initialized with its type, and the
    // caller vouches for the chain.
    unsafe { (instance.fp().get_system_properties)(instance.as_raw(), system, &mut properties) }
}

// Whether the system supports Android XR's face tracking, in a call of its own because the binding
// has the structure on Android only.
#[cfg(target_os = "android")]
fn android_face_support(
    instance: &xr::Instance,
    system: xr::SystemId,
    enabled: bool,
) -> Option<bool> {
    if !enabled {
        return None;
    }
    let mut face = xr::sys::SystemFaceTrackingPropertiesANDROID {
        ty: xr::sys::SystemFaceTrackingPropertiesANDROID::TYPE,
        next: ptr::null_mut(),
        supports_face_tracking: false.into(),
    };
    // SAFETY: `face` is initialized with its type, outlives the call and belongs to an extension
    // that the instance enabled.
    let result = unsafe { fetch(instance, system, ptr::from_mut(&mut face).cast()) };
    (result.into_raw() >= 0).then(|| face.supports_face_tracking.into())
}

#[cfg(not(target_os = "android"))]
fn android_face_support(
    _instance: &xr::Instance,
    _system: xr::SystemId,
    _enabled: bool,
) -> Option<bool> {
    None
}
