//! The composition locals of a headset's content.
//!
//! One thread-local struct holds them all. On Android every `thread_local!` static takes a
//! pthread key that is never freed, and a process has few keys.

use cranpose_core::{CompositionLocal, compositionLocalOf};

use crate::{
    Anchors, EntitlementCheck, Haptics, HeadsetProperties, Planes, SpatialCapabilities,
    SpatialEnvironment, TrackingCapabilities,
    frame_effects::FrameEffects,
    space::{Origin, SUBSPACE_ORIGIN, Space},
    spatial_curved_row::Curve,
};

pub(crate) struct Locals {
    pub(crate) space: CompositionLocal<Option<Space>>,
    pub(crate) capabilities: CompositionLocal<SpatialCapabilities>,
    pub(crate) environment: CompositionLocal<Option<SpatialEnvironment>>,
    pub(crate) curve: CompositionLocal<Option<Curve>>,
    pub(crate) origin: CompositionLocal<Origin>,
    pub(crate) frame_effects: CompositionLocal<Option<FrameEffects>>,
    pub(crate) haptics: CompositionLocal<Option<Haptics>>,
    pub(crate) headset_properties: CompositionLocal<Option<HeadsetProperties>>,
    pub(crate) tracking_capabilities: CompositionLocal<TrackingCapabilities>,
    pub(crate) entitlement_check: CompositionLocal<Option<EntitlementCheck>>,
    pub(crate) anchors: CompositionLocal<Option<Anchors>>,
    pub(crate) planes: CompositionLocal<Option<Planes>>,
}

thread_local! {
    static LOCALS: Locals = Locals {
        space: compositionLocalOf(|| None),
        capabilities: compositionLocalOf(SpatialCapabilities::default),
        environment: compositionLocalOf(|| None),
        curve: compositionLocalOf(|| None),
        origin: compositionLocalOf(|| Origin::from(SUBSPACE_ORIGIN)),
        frame_effects: compositionLocalOf(|| None),
        haptics: compositionLocalOf(|| None),
        headset_properties: compositionLocalOf(|| None),
        tracking_capabilities: compositionLocalOf(TrackingCapabilities::default),
        entitlement_check: compositionLocalOf(|| None),
        anchors: compositionLocalOf(|| None),
        planes: compositionLocalOf(|| None),
    };
}

pub(crate) fn with_locals<R>(read: impl FnOnce(&Locals) -> R) -> R {
    LOCALS.with(read)
}
